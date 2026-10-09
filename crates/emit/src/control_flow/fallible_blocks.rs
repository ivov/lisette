use super::propagation::plain_return;
use crate::Planner;
use crate::ReturnContext;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::calls::bound_value::BoundValue;
use crate::calls::comma_ok::{CommaOkValueSlot, PairKind};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FalliblePlanner};
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{LoweredBlock, Statement, define};
use crate::plan::go_expression::FunctionLiteralLayout;
use crate::plan::placement::is_unit_call;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::types::Type;

pub(crate) struct TryBlockPairPlan<'e> {
    items: &'e [Expression],
    effective_ty: Type,
    fallible: Fallible,
    body_ctx: ReturnContext,
    shape: CallableReturnAbi,
    kind: PairKind,
}

impl TryBlockPairPlan<'_> {
    pub(crate) fn bind(self, planner: &mut Planner<'_>, slot: CommaOkValueSlot) -> BoundValue {
        let go_return = planner.render_lowered_return_ty(&self.shape, &self.effective_ty);
        let body = planner.with_isolated_function(self.body_ctx, |planner| LoweredBlock {
            statements: planner.lower_try_items(self.items, &self.fallible),
        });
        let call = GoExpression::immediate_call(go_return, body, FunctionLiteralLayout::MultiLine);
        planner.bind_pair(Vec::new(), call, slot, self.kind, None)
    }
}

impl Planner<'_> {
    /// `try { ... }` → `result := func() T { ... }()`; value is the bound result var.
    pub(crate) fn lower_try_block(&mut self, items: &[Expression], ty: &Type) -> ValuePlan {
        let result_var = self.fresh_var(Some("tryResult"));
        self.lower_try_block_as(items, ty, result_var)
    }

    pub(crate) fn lower_try_block_into(
        &mut self,
        items: &[Expression],
        ty: &Type,
        name: &str,
    ) -> Vec<Statement> {
        self.lower_try_block_as(items, ty, name.to_string())
            .into_parts()
            .0
    }

    pub(crate) fn try_block_pair_plan<'e>(
        &self,
        items: &'e [Expression],
        ty: &Type,
    ) -> Option<TryBlockPairPlan<'e>> {
        let return_ctx = self.return_ctx();
        let ty = self.facts.peel_alias(ty);
        let effective_ty = resolve_fallible_block_type(items, &ty, Some(&return_ctx));
        let fallible = Fallible::from_type(&effective_ty)?;
        let body_ctx = self.return_context_for_type(effective_ty.clone());
        let shape = body_ctx.lowered_shape()?;
        let kind = match shape {
            CallableReturnAbi::Result {
                payload: PayloadLayout::Packed,
            } => PairKind::Result { nil_guard: None },
            CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
                payload: PayloadLayout::Packed,
            }) => PairKind::CommaOk { nil_guard: None },
            _ => return None,
        };
        Some(TryBlockPairPlan {
            items,
            effective_ty,
            fallible,
            body_ctx,
            shape,
            kind,
        })
    }

    fn lower_try_block_as(
        &mut self,
        items: &[Expression],
        ty: &Type,
        result_var: String,
    ) -> ValuePlan {
        let return_ctx = self.return_ctx();
        let ty = self.facts.peel_alias(ty);
        let effective_ty = resolve_fallible_block_type(items, &ty, Some(&return_ctx));
        let fallible = Fallible::from_type(&effective_ty)
            .expect("`try` block must have Result or Option type");

        self.declare(&result_var);
        let full_ty = {
            let mut fe = FalliblePlanner::new(self, &fallible);
            fe.full_type_string()
        };

        let body_ctx = ReturnContext::Tagged(effective_ty);
        let body = self
            .with_isolated_function(body_ctx, |planner| planner.lower_try_body(items, &fallible));

        let setup = vec![define(
            result_var.clone(),
            GoExpression::immediate_call(full_ty, body, FunctionLiteralLayout::MultiLine),
        )];
        ValuePlan::captured(setup, result_var)
    }

    fn lower_try_body(&mut self, items: &[Expression], fallible: &Fallible) -> LoweredBlock {
        LoweredBlock {
            statements: self.lower_try_items(items, fallible),
        }
    }

    /// A `try` block ending a function of its own type lowers without the closure.
    pub(crate) fn lower_try_tail_in_place(
        &mut self,
        items: &[Expression],
        ty: &Type,
    ) -> Option<Vec<Statement>> {
        let return_ctx = self.return_ctx();
        let return_ty = self.facts.peel_alias(return_ctx.ty()?);
        let effective_ty =
            resolve_fallible_block_type(items, &self.facts.peel_alias(ty), Some(&return_ctx));
        if return_ty.demoted() != effective_ty.demoted() {
            return None;
        }
        let fallible = Fallible::from_type(&effective_ty)?;
        Some(self.with_binding_frame(|planner| planner.lower_try_items(items, &fallible)))
    }

    fn lower_try_items(&mut self, items: &[Expression], fallible: &Fallible) -> Vec<Statement> {
        let Some((last, rest)) = items.split_last() else {
            return self.lower_try_unit_return(fallible);
        };
        let mut statements: Vec<Statement> = rest
            .iter()
            .enumerate()
            .map(|(index, item)| self.lower_block_item(item, &items[index + 1..]))
            .collect();
        statements.extend(self.lower_try_tail(last, fallible));
        statements
    }

    fn lower_try_tail(&mut self, last: &Expression, fallible: &Fallible) -> Vec<Statement> {
        if last.diverges().is_some() || last.get_type().is_never() {
            return vec![self.lower_statement(last)];
        }

        let is_statement_only = matches!(
            last,
            Expression::Let { .. }
                | Expression::Const { .. }
                | Expression::Assignment { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::For { .. }
                | Expression::Loop { .. }
        );
        if is_statement_only || is_unit_call(last) {
            let mut statements = vec![self.lower_statement(last)];
            statements.extend(self.lower_try_unit_return(fallible));
            return statements;
        }

        let (mut statements, final_expression) = self
            .lower_value(last, ExpressionContext::value())
            .into_parts();
        if final_expression.is_empty() {
            statements.extend(self.lower_try_unit_return(fallible));
        } else {
            statements.extend(self.success_return(fallible, final_expression));
        }
        statements
    }

    fn lower_try_unit_return(&mut self, fallible: &Fallible) -> Vec<Statement> {
        let unit_val = self.zero_value_expression(fallible.ok_ty());
        self.success_return(fallible, unit_val)
    }

    /// `Err(...)?` and `None?` short-circuit directly into a return. `None`
    /// when the expression is not a failure-constructor `?`.
    pub(super) fn try_lower_error_constructor(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
    ) -> Option<Vec<Statement>> {
        let mut statements: Vec<Statement> = Vec::new();
        let err_arg = match expression {
            Expression::Call {
                expression: func,
                args,
                ..
            } => {
                if fallible.classify_constructor(func) != Some(ConstructorKind::Failure) {
                    return None;
                }
                if !args.is_empty() {
                    let (setup, value) = self
                        .lower_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                    Some(value)
                } else {
                    None
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(expression) != Some(ConstructorKind::Failure) {
                    return None;
                }
                None
            }
            _ => return None,
        };

        let error = err_arg
            .map(|value| self.convert_error_to_return_context(&mut statements, value, fallible));
        statements.push(self.failure_return(fallible, error));
        Some(statements)
    }

    /// `recover { ... }` → `result := lisette.RecoverBlock(func() T { ... })`.
    pub(crate) fn lower_recover_block(&mut self, items: &[Expression], ty: &Type) -> ValuePlan {
        let return_ctx = self.return_ctx();
        let ty = self.facts.peel_alias(ty);
        let effective_ty = resolve_fallible_block_type(items, &ty, Some(&return_ctx));
        let fallible = Fallible::from_type(&effective_ty)
            .expect("recover block type must be Result<T, PanicValue>");

        let result_var = self.fresh_var(Some("recoverResult"));
        self.declare(&result_var);
        let inner_ty_str = self.use_go_type(fallible.ok_ty());

        let body_return_ctx = self.return_context_for_type(fallible.ok_ty().clone());
        let body = self.with_isolated_function(body_return_ctx, |planner| {
            planner.lower_recover_body_block(items, &fallible)
        });

        let setup = vec![define(
            result_var.clone(),
            GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "RecoverBlock"),
                vec![GoExpression::function_literal(
                    Vec::new(),
                    inner_ty_str,
                    body,
                    FunctionLiteralLayout::MultiLine,
                )],
            ),
        )];
        ValuePlan::captured(setup, result_var)
    }

    fn lower_recover_body_block(
        &mut self,
        items: &[Expression],
        fallible: &Fallible,
    ) -> LoweredBlock {
        let Some((last, rest)) = items.split_last() else {
            return LoweredBlock {
                statements: vec![self.lower_zero_return(fallible.ok_ty())],
            };
        };
        let mut statements: Vec<Statement> = rest
            .iter()
            .enumerate()
            .map(|(index, item)| self.lower_block_item(item, &items[index + 1..]))
            .collect();
        statements.extend(self.lower_recover_tail(last, fallible));
        LoweredBlock { statements }
    }

    fn lower_recover_tail(&mut self, last: &Expression, fallible: &Fallible) -> Vec<Statement> {
        let item_ty = last.get_type();
        if item_ty.is_never() {
            return vec![self.lower_statement(last)];
        }
        if item_ty.is_unit() || item_ty.is_ignored() || item_ty.is_variable() {
            return vec![
                self.lower_statement(last),
                self.lower_zero_return(fallible.ok_ty()),
            ];
        }
        let (mut statements, expression) = self
            .lower_value(last, ExpressionContext::value())
            .into_parts();
        statements.push(plain_return(expression));
        statements
    }

    /// A structured zero-value return for a `recover` block's inner type.
    fn lower_zero_return(&mut self, ty: &Type) -> Statement {
        plain_return(self.zero_value_expression(ty))
    }
}

/// Prefer the function's return context type when the block's own ok_ty
/// is a type variable (e.g. `Result[any, ...]` when tail is a statement),
/// or when the tail is Never-typed (ok_ty resolves to unit/Never because
/// nothing constrains it).
fn resolve_fallible_block_type(
    items: &[Expression],
    ty: &Type,
    outer: Option<&ReturnContext>,
) -> Type {
    let tail_is_never = items.last().is_some_and(|last| {
        let t = last.get_type();
        t.is_never() || last.diverges().is_some()
    });
    let base = Fallible::from_type(ty);
    let needs_return_context = tail_is_never
        || base.as_ref().is_some_and(|f| {
            f.ok_ty().is_variable() || f.ok_ty().is_placeholder() || f.ok_ty().is_never()
        });
    if !needs_return_context {
        return ty.clone();
    }
    let resolved = outer
        .expect("fallible block type resolution requires a threaded outer return context")
        .clone();
    resolved
        .ty()
        .filter(|ty| Fallible::from_type(ty).is_some())
        .cloned()
        .unwrap_or_else(|| ty.clone())
}
