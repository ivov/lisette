use super::propagation::plain_return;
use crate::Planner;
use crate::lookup::state::enclosing::ReturnContext;
use crate::lowering::abi::coercion_plan::CoercionPlan;
use crate::lowering::calls::bound_value::BoundValue;
use crate::lowering::calls::comma_ok::{CommaOkValueSlot, PairKind};
use crate::lowering::statements::blocks::{BlockTail, TailTarget};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::output::finish::function_literals::function_literal;
use crate::output::finish::function_literals::immediate_call;
use crate::target::ir::bodies::{LoweredBlock, Statement, define};
use crate::target::ir::go_expression::FunctionLiteralLayout;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{ConstructorCall, ConstructorKind, Fallible};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::return_abi::{
    CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout,
};
use syntax::ast::Expression;
use syntax::program::SlotOrigin;
use syntax::types::Type;

pub(in crate::lowering) struct TryBlockPairPlan<'e> {
    items: &'e [Expression],
    effective_ty: Type,
    fallible: Fallible,
    body_ctx: ReturnContext,
    shape: LoweredReturnAbi,
    kind: PairKind,
}

impl TryBlockPairPlan<'_> {
    pub(in crate::lowering) fn bind(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        let go_return = planner.queries().return_go_type(
            &CallableReturnAbi::Lowered(self.shape.clone()),
            &self.effective_ty,
        );
        let body = planner.with_isolated_function(self.body_ctx, |planner| LoweredBlock {
            statements: planner.lower_try_items(self.items, &self.fallible),
        });
        let call = immediate_call(go_return, body, FunctionLiteralLayout::MultiLine);
        planner.bind_pair(Vec::new(), call, slot, self.kind, None)
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_try_block(
        &mut self,
        items: &[Expression],
        ty: &Type,
    ) -> ValuePlan {
        let result_var = self.fresh_local("tryResult");
        self.plan_try_block_as(items, ty, ty, result_var)
    }

    pub(in crate::lowering) fn lower_try_block_into(
        &mut self,
        items: &[Expression],
        ty: &Type,
        target_ty: &Type,
        name: &GoIdentifier,
    ) -> Vec<Statement> {
        self.plan_try_block_as(items, ty, target_ty, name.clone())
            .into_parts()
            .0
    }

    pub(in crate::lowering) fn try_block_pair_plan<'e>(
        &self,
        items: &'e [Expression],
        ty: &Type,
    ) -> Option<TryBlockPairPlan<'e>> {
        let fallible = self.facts.fallible(ty)?;
        let body_ctx = self.return_context_for_type(ty.clone());
        let shape = body_ctx.lowered_shape()?;
        let kind = match shape {
            LoweredReturnAbi::Result {
                payload: PayloadLayout::Packed,
            } => PairKind::Result { nil_guard: None },
            LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
                payload: PayloadLayout::Packed,
            }) => PairKind::CommaOk { nil_guard: None },
            LoweredReturnAbi::BareError
            | LoweredReturnAbi::Result { .. }
            | LoweredReturnAbi::Partial { .. }
            | LoweredReturnAbi::Option(_)
            | LoweredReturnAbi::Tuple { .. } => return None,
        };

        Some(TryBlockPairPlan {
            items,
            effective_ty: ty.clone(),
            fallible,
            body_ctx,
            shape,
            kind,
        })
    }

    fn plan_try_block_as(
        &mut self,
        items: &[Expression],
        ty: &Type,
        target_ty: &Type,
        result_var: GoIdentifier,
    ) -> ValuePlan {
        let fallible = self
            .facts
            .fallible(ty)
            .expect("`try` block must have Result or Option type");

        self.scope.declare_go_name(result_var.spelling());
        let full_ty = self.queries().go_type(ty);
        let source = self.facts.value_layout(ty, SlotOrigin::Lisette);
        let target = self.facts.value_layout(target_ty, SlotOrigin::Lisette);
        let coercion = CoercionPlan::change(&self.queries(), &source, &target);

        let body_ctx = ReturnContext::Tagged(ty.clone());
        let body = self
            .with_isolated_function(body_ctx, |planner| planner.lower_try_body(items, &fallible));

        let value = immediate_call(Some(full_ty), body, FunctionLiteralLayout::MultiLine);
        let (mut setup, value) = coercion.lower(self, value);
        setup.push(define(result_var.clone(), value));
        ValuePlan::captured(setup, result_var)
    }

    fn lower_try_body(&mut self, items: &[Expression], fallible: &Fallible) -> LoweredBlock {
        LoweredBlock {
            statements: self.lower_try_items(items, fallible),
        }
    }

    pub(in crate::lowering) fn try_lower_try_tail_in_place(
        &mut self,
        block: &Expression,
    ) -> Option<Vec<Statement>> {
        let Expression::TryBlock { items, ty, .. } = block else {
            return None;
        };
        if self.scope.current_return_ctx().ty().is_none() || self.facts.converts(block) {
            return None;
        }
        let fallible = self.facts.fallible(ty)?;
        Some(self.with_binding_frame(|planner| planner.lower_try_items(items, &fallible)))
    }

    fn lower_try_items(&mut self, items: &[Expression], fallible: &Fallible) -> Vec<Statement> {
        let Some((last, rest)) = items.split_last() else {
            return self.lower_try_unit_return(fallible);
        };
        self.lower_block_items(rest, last, BlockTail::Value(TailTarget::Try(fallible)))
    }

    pub(in crate::lowering) fn lower_try_value_tail(
        &mut self,
        last: &Expression,
        fallible: &Fallible,
    ) -> Vec<Statement> {
        let (mut statements, final_expression) = self
            .plan_value(last, ExpressionContext::value())
            .into_parts();
        let final_expression = self.apply_type_coercion(
            &mut statements,
            Some(fallible.ok_ty()),
            last,
            final_expression,
        );
        statements.extend(self.success_return(fallible, final_expression));
        statements
    }

    pub(in crate::lowering) fn lower_try_unit_return(
        &mut self,
        fallible: &Fallible,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        let unit = self.unit_value(None, Some(fallible.ok_ty()));
        statements.extend(self.success_return(fallible, unit));
        statements
    }

    pub(super) fn try_lower_error_constructor(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
    ) -> Option<Vec<Statement>> {
        let Some(ConstructorCall {
            kind: ConstructorKind::Failure,
            args,
        }) = fallible.constructor_call(expression)
        else {
            return None;
        };
        let mut statements: Vec<Statement> = Vec::new();
        let err_arg = args.first().map(|argument| {
            let (setup, value) = self
                .plan_value(argument, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            value
        });

        let error = err_arg.map(|value| {
            self.convert_error_to_return_context(&mut statements, expression, value, fallible)
        });
        statements.push(self.failure_return(fallible, error));
        Some(statements)
    }

    pub(in crate::lowering) fn plan_recover_block(
        &mut self,
        items: &[Expression],
        ty: &Type,
    ) -> ValuePlan {
        let fallible = self
            .facts
            .fallible(ty)
            .expect("recover block type must be Result<T, PanicValue>");

        let result_var = self.fresh_declared("recoverResult");
        let inner_ty_str = self.queries().go_type(fallible.ok_ty());

        let ok_ty = fallible.ok_ty().clone();
        let body_return_ctx = if ok_ty.is_never() {
            ReturnContext::Void(ok_ty)
        } else {
            ReturnContext::Tagged(ok_ty)
        };
        let body = self.with_isolated_function(body_return_ctx, |planner| {
            planner.lower_recover_body_block(items, &fallible)
        });

        let setup = vec![define(
            result_var.clone(),
            GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "RecoverBlock"),
                vec![function_literal(
                    Vec::new(),
                    Some(inner_ty_str),
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
                statements: self.lower_unit_return(fallible.ok_ty()),
            };
        };
        LoweredBlock {
            statements: self.lower_block_items(rest, last, BlockTail::Value(TailTarget::Return)),
        }
    }

    pub(in crate::lowering) fn lower_unit_return(&self, ok_ty: &Type) -> Vec<Statement> {
        let mut statements = Vec::new();
        let unit = self.unit_value(None, Some(ok_ty));
        statements.push(plain_return(unit));
        statements
    }
}
