use super::propagation::plain_return;
use crate::EmitEffects;
use crate::Planner;
use crate::ReturnContext;
use crate::abi::transition;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FalliblePlanner};
use crate::definitions::functions::is_go_never;
use crate::plan::bodies::{LoweredBlock, LoweredStatement};
use crate::plan::placement::is_unit_call;
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    /// `try { ... }` → `ClosureBind` over `func() T { ... }()`; value is the
    /// bound result var.
    pub(crate) fn lower_try_block(
        &mut self,
        items: &[Expression],
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        fx.require_stdlib();

        let return_ctx = self.return_ctx();
        let effective_ty = resolve_fallible_block_type(items, ty, Some(return_ctx.as_ref()));
        let fallible = Fallible::from_type(&effective_ty)
            .expect("`try` block must have Result or Option type");

        let result_var = self.fresh_var(Some("tryResult"));
        self.declare(&result_var);
        let full_ty = {
            let mut fe = FalliblePlanner::new(self, &fallible, fx);
            fe.full_type_string()
        };

        let body_ctx = ReturnContext::TaggedBlock(effective_ty);
        self.push_return_ctx(body_ctx.clone());
        let body = self.with_fresh_scope(|planner| planner.lower_try_body(items, &fallible, fx));
        self.pop_return_ctx();

        let setup = vec![LoweredStatement::ClosureBind {
            name: result_var.clone(),
            closure_open: format!("func() {} {{\n", full_ty),
            body,
            closure_close: "}()\n".to_string(),
        }];
        (setup, result_var)
    }

    fn lower_try_body(
        &mut self,
        items: &[Expression],
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        let Some((last, rest)) = items.split_last() else {
            return LoweredBlock {
                statements: vec![self.lower_try_unit_return(fallible, fx)],
            };
        };
        let mut statements: Vec<LoweredStatement> = rest
            .iter()
            .map(|item| self.lower_statement(item, fx))
            .collect();
        statements.extend(self.lower_try_tail(last, fallible, fx));
        LoweredBlock { statements }
    }

    /// Tail of a `try` block: never tail (statement + unreachable panic),
    /// statement-only/unit-call tail (statement + success unit return), or a
    /// value tail (success-wrapped return; unit return when the value is empty).
    fn lower_try_tail(
        &mut self,
        last: &Expression,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        if last.diverges().is_some() || last.get_type().is_never() {
            let mut statements = vec![self.lower_statement(last, fx)];
            if !is_go_never(last) {
                statements.push(LoweredStatement::RawGo(
                    "panic(\"unreachable\")\n".to_string(),
                ));
            }
            return statements;
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
            return vec![
                self.lower_statement(last, fx),
                self.lower_try_unit_return(fallible, fx),
            ];
        }

        let mut statements = Vec::new();
        let mut buffer = String::new();
        let final_expression = self.emit_value(&mut buffer, last, ExpressionContext::value(), fx);
        if !buffer.is_empty() {
            statements.push(LoweredStatement::RawGo(buffer));
        }
        if final_expression.is_empty() {
            statements.push(self.lower_try_unit_return(fallible, fx));
        } else {
            statements.push(self.lower_try_success_return(&final_expression, fallible, fx));
        }
        statements
    }

    fn lower_try_unit_return(
        &mut self,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let (unit_val, effects) = self.zero_value(fallible.ok_ty());
        fx.extend(&effects);
        self.lower_try_success_return(&unit_val, fallible, fx)
    }

    fn lower_try_success_return(
        &mut self,
        value: &str,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let ok_return = {
            let mut fe = FalliblePlanner::new(self, fallible, fx);
            fe.emit_success(value)
        };
        plain_return(ok_return)
    }

    /// `Err(...)?` and `None?` short-circuit directly into a return. `None`
    /// when the expression is not a failure-constructor `?`.
    pub(super) fn try_lower_error_constructor(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> Option<Vec<LoweredStatement>> {
        let mut statements: Vec<LoweredStatement> = Vec::new();
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
                    let (setup, value) = self.lower_value(&args[0], ExpressionContext::value(), fx);
                    statements.extend(setup);
                    Some(value)
                } else {
                    Some(String::new())
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(expression) != Some(ConstructorKind::Failure) {
                    return None;
                }
                Some(String::new())
            }
            _ => return None,
        };

        let return_ctx = self.return_ctx();
        if let Some(shape) = return_ctx.lowered_shape() {
            let return_ty = return_ctx.expect_ty();
            let values = if fallible.is_result() {
                let err_expr = err_arg.as_deref().unwrap_or("");
                transition::lowered_err_values(self, &shape, &return_ty, err_expr, fx)
            } else {
                transition::lowered_none_values(self, &shape, &return_ty, fx)
            };
            statements.push(LoweredStatement::RawGo(format!(
                "return {}\n",
                values.join(", ")
            )));
        } else {
            fx.require_stdlib();
            let err_return = {
                let mut fe = FalliblePlanner::new(self, fallible, fx);
                fe.emit_contextual_failure(err_arg.as_deref())
            };
            statements.push(plain_return(err_return));
        }
        Some(statements)
    }

    /// `recover { ... }` → `ClosureBind` over
    /// `lisette.RecoverBlock(func() T { ... })`.
    pub(crate) fn lower_recover_block(
        &mut self,
        items: &[Expression],
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        fx.require_stdlib();

        let return_ctx = self.return_ctx();
        let effective_ty = resolve_fallible_block_type(items, ty, Some(return_ctx.as_ref()));
        let fallible = Fallible::from_type(&effective_ty)
            .expect("recover block type must be Result<T, PanicValue>");

        let result_var = self.fresh_var(Some("recoverResult"));
        self.declare(&result_var);
        let inner_ty_str = self.go_type_string(fallible.ok_ty(), fx);

        let body_return_ctx = self.return_context_for_type(fallible.ok_ty().clone());
        self.push_return_ctx(body_return_ctx.clone());
        let body =
            self.with_fresh_scope(|planner| planner.lower_recover_body_block(items, &fallible, fx));
        self.pop_return_ctx();

        let setup = vec![LoweredStatement::ClosureBind {
            name: result_var.clone(),
            closure_open: format!("lisette.RecoverBlock(func() {} {{\n", inner_ty_str),
            body,
            closure_close: "})\n".to_string(),
        }];
        (setup, result_var)
    }

    fn lower_recover_body_block(
        &mut self,
        items: &[Expression],
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        let Some((last, rest)) = items.split_last() else {
            return LoweredBlock {
                statements: vec![self.lower_zero_return(fallible.ok_ty(), fx)],
            };
        };
        let mut statements: Vec<LoweredStatement> = rest
            .iter()
            .map(|item| self.lower_statement(item, fx))
            .collect();
        statements.extend(self.lower_recover_tail(last, fallible, fx));
        LoweredBlock { statements }
    }

    /// Tail of a `recover` block: never tail (statement + unreachable panic),
    /// unit/type-variable tail (statement + zero-value return), or a value tail
    /// (plain return of the value).
    fn lower_recover_tail(
        &mut self,
        last: &Expression,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let item_ty = last.get_type();
        if item_ty.is_never() {
            let mut statements = vec![self.lower_statement(last, fx)];
            if !is_go_never(last) {
                statements.push(LoweredStatement::RawGo(
                    "panic(\"unreachable\")\n".to_string(),
                ));
            }
            return statements;
        }
        if item_ty.is_unit() || item_ty.is_variable() {
            return vec![
                self.lower_statement(last, fx),
                self.lower_zero_return(fallible.ok_ty(), fx),
            ];
        }
        let mut statements = Vec::new();
        let mut buffer = String::new();
        let expression = self.emit_value(&mut buffer, last, ExpressionContext::value(), fx);
        if !buffer.is_empty() {
            statements.push(LoweredStatement::RawGo(buffer));
        }
        statements.push(plain_return(expression));
        statements
    }

    /// A structured zero-value return for a `recover` block's inner type.
    fn lower_zero_return(&mut self, ty: &Type, fx: &mut EmitEffects) -> LoweredStatement {
        let (zero, effects) = self.zero_value(ty);
        fx.extend(&effects);
        plain_return(zero)
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
        || base
            .as_ref()
            .is_some_and(|f| f.ok_ty().is_variable() || f.ok_ty().is_never());
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
