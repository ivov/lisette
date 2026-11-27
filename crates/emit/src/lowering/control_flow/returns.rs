use crate::Planner;
use crate::lookup::state::enclosing::ReturnContext;
use crate::lowering::abi::fallible_producer::OptionFusePlan;
use crate::lowering::abi::go_returns;
use crate::lowering::calls::call_plan::CallPlan;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::comma_ok::StatementSlot;
use crate::lowering::calls::go_interop::LoweredProducer;
use crate::lowering::calls::wrap_err::WrapChain;
use crate::lowering::control_flow::branches::Branching;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::expressions::tuple_literal::TupleLiteral;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::blocks::{BlockTail, TailKind, TailTarget};
use crate::lowering::statements::placement::ElidableTail;
use crate::lowering::statements::placement::try_elide_tail_let;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::target::ir::bodies::{LoweredBlock, LoweredStatement, Statement};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{ConstructorCall, ConstructorKind, Fallible, prelude_call};
use crate::target::ir::termination::list_terminates;
use crate::target::ir::values::GoExpression;
use crate::target::types::return_abi::{
    LoweredFailure, LoweredReturnAbi, OptionReturnAbi, PayloadLayout,
};
use std::slice;
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn lower_return(&mut self, expression: &Expression) -> Vec<Statement> {
        let expression = if let Expression::Return { expression, .. } = expression {
            expression.as_ref()
        } else {
            expression
        };
        match self.queries().tail_kind(expression) {
            TailKind::Diverges => self.lower_never_value(expression),
            TailKind::Unit => self.lower_unit_value_return(expression),
            TailKind::Value => self.lower_return_value_tail(expression),
        }
    }

    fn lower_unit_value_return(&mut self, expression: &Expression) -> Vec<Statement> {
        let mut statements = self.lower_unit_effect(expression);
        if list_terminates(&statements) {
            return statements;
        }
        let return_ctx = self.scope.current_return_ctx();
        statements.push(if matches!(return_ctx, ReturnContext::Void(_)) {
            LoweredStatement::Return(Vec::new()).into()
        } else {
            plain_return(self.unit_value(Some(expression), return_ctx.ty()))
        });
        statements
    }

    pub(in crate::lowering) fn try_lower_wrapped_return(
        &mut self,
        expression: &Expression,
    ) -> Option<Vec<Statement>> {
        let return_ctx = self.scope.current_return_ctx();
        let written_ty = match return_ctx.ty() {
            Some(ty) => ty.clone(),
            None => expression.get_type(),
        };
        let fallible = self.facts.fallible(&written_ty)?;

        if let Some(ConstructorCall {
            kind: ConstructorKind::Failure,
            args: [],
        }) = fallible.constructor_call(expression)
        {
            return Some(self.lower_failure_constructor_return(
                &[],
                &fallible,
                &WrapChain::default(),
            ));
        }

        if let Some(branching) = Branching::of(expression)
            && !matches!(branching, Branching::Select { .. })
        {
            return Some(
                self.lower_branching_to_block(branching, &PlacePlan::Return)
                    .statements,
            );
        }

        let coercion = self.value_slot_coercion(expression, &written_ty);
        if coercion.is_identity()
            && let Some(site) = CallSite::of(expression)
        {
            return Some(self.lower_wrapped_call_return(site, &fallible, &written_ty));
        }

        let (mut statements, value) = if let Expression::Propagate {
            expression: inner, ..
        } = expression
            && coercion.is_identity()
        {
            self.plan_propagate(inner)
        } else {
            self.plan_value(expression, ExpressionContext::value())
                .into_parts()
        };

        let (coercion_setup, value) = coercion.lower(self, value);
        statements.extend(coercion_setup);
        statements.extend(self.wrapped_value_return(value, &written_ty));
        Some(statements)
    }

    fn wrapped_value_return(&mut self, value: GoExpression, return_ty: &Type) -> Vec<Statement> {
        let Some(shape) = self.scope.current_return_ctx().lowered_shape() else {
            return vec![plain_return(value)];
        };
        self.lower_result_return(value, "v", return_ty, &shape)
    }

    fn lower_wrapped_call_return(
        &mut self,
        site: CallSite<'_>,
        fallible: &Fallible,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let (inner, wraps) = self.queries().peel_wrap_err(site.expression);
        if let Some(ConstructorCall {
            kind: ConstructorKind::Failure,
            args,
        }) = fallible.constructor_call(inner)
            && !wraps.is_empty()
        {
            return self.lower_failure_constructor_return(args, fallible, &wraps);
        }
        match fallible.constructor_call(site.expression) {
            Some(ConstructorCall {
                kind: ConstructorKind::Success,
                args,
            }) => self.lower_success_constructor_return(args, fallible),
            Some(ConstructorCall {
                kind: ConstructorKind::Failure,
                args,
            }) => self.lower_failure_constructor_return(args, fallible, &WrapChain::default()),
            None => self.lower_wrapped_passthrough_return(site, fallible, return_ty),
        }
    }

    fn lower_success_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        match self.scope.current_return_ctx().lowered_shape().as_ref() {
            Some(LoweredReturnAbi::BareError) => {
                if !args.is_empty() {
                    let (setup, _) = self
                        .plan_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                }
                statements.push(go_returns::multi_value_return(
                    LoweredReturnAbi::BareError.ok_values(Vec::new()),
                ));
            }
            Some(shape) if args.is_empty() => {
                statements.push(go_returns::multi_value_return(
                    shape.ok_values(vec![self.unit_value(None, None)]),
                ));
            }
            Some(shape)
                if shape.has_flattened_payload()
                    && let Some(literal) = TupleLiteral::of(&args[0]) =>
            {
                let (setup, parts) = self.lowered_tuple_literal_values(literal, fallible.ok_ty());
                statements.extend(setup);
                statements.push(go_returns::multi_value_return(shape.ok_values(parts)));
            }
            _ => {
                let (setup, value) = self
                    .plan_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                let value = self.apply_type_coercion(
                    &mut statements,
                    Some(fallible.ok_ty()),
                    &args[0],
                    value,
                );

                statements.extend(self.success_return(fallible, value));
            }
        }
        statements
    }

    fn lower_failure_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
        wraps: &WrapChain<'_>,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        let error = fallible.err_ty().zip(args.first()).map(|(err_ty, arg)| {
            let error = self.plan_composite_value(arg, ExpressionContext::value());
            self.wrap_planned_error(&mut statements, wraps, error, |this, statements, error| {
                let to = this.queries().contextual_err_ty(err_ty);
                this.apply_type_coercion(statements, Some(&to), arg, error)
            })
        });

        statements.push(self.failure_return(fallible, error));
        statements
    }

    fn lower_propagated_return(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
    ) -> Vec<Statement> {
        let (mut statements, value) = self.plan_propagate(expression);
        statements.extend(self.success_return(fallible, value));
        statements
    }

    fn lower_wrapped_passthrough_return(
        &mut self,
        site: CallSite<'_>,
        fallible: &Fallible,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let expression = site.expression;
        let lowered = self.scope.current_return_ctx().lowered_shape();
        let lowered = lowered.as_ref();
        let mut statements = Vec::new();
        if self.queries().returns_by_propagation(expression, lowered) {
            return self.lower_propagated_return(expression, fallible);
        }
        if let Some(shape) = lowered
            && self.queries().callee_matches_lowered_shape(site, shape)
        {
            let (setup, call) = self
                .plan_call_expression(site, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.push(plain_return(call));
            return statements;
        }
        if let Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        })) = lowered
            && let Some(statements) = self.try_lower_comma_ok_return(site)
        {
            return statements;
        }
        let plan = self.queries().plan_call(site);
        if plan.resolved.abi.result.is_lowered() {
            return self.lower_lowered_call_return(&plan, lowered, return_ty);
        }
        if let Some(shape) = lowered {
            let (setup, value) = self
                .plan_value(expression, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.extend(self.lower_result_return(value, "v", return_ty, shape));
            return statements;
        }
        let (setup, call) = self
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        statements.push(plain_return(call));
        statements
    }

    fn try_lower_comma_ok_return(&mut self, site: CallSite<'_>) -> Option<Vec<Statement>> {
        let source = self.queries().comma_ok_source(site)?.plain()?;
        let pair = self.bind_plain_comma_ok_pair(site, source, StatementSlot::Temp);
        let value = pair.value.unwrap_or_else(GoIdentifier::blank);
        let mut statements = pair.statements;
        statements.push(go_returns::multi_value_return(vec![
            GoExpression::identifier(value),
            GoExpression::identifier(pair.status),
        ]));
        Some(statements)
    }

    fn lower_lowered_call_return(
        &mut self,
        plan: &CallPlan<'_, '_>,
        lowered: Option<&LoweredReturnAbi>,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered {
            let (setup, result_var) = self.plan_go_abi_wrapped_call(plan, return_ty).into_parts();

            statements.extend(setup);
            statements.extend(self.lower_result_return(result_var, "res", return_ty, shape));
        } else {
            let abi = &plan.resolved.abi;
            if self
                .queries()
                .go_result_layout_change(abi, return_ty)
                .is_none()
            {
                let (setup, call) = self
                    .plan_call_expression(plan.site, None, ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                statements.extend(self.lower_abi_to_tagged_return(
                    call,
                    LoweredProducer::of_call(abi, plan.resolved.origin),
                    return_ty,
                ));
            } else {
                let (setup, result_var) =
                    self.plan_go_abi_wrapped_call(plan, return_ty).into_parts();
                statements.extend(setup);
                statements.push(plain_return(result_var));
            }
        }
        statements
    }

    pub(in crate::lowering) fn convert_error_to_return_context(
        &mut self,
        statements: &mut Vec<Statement>,
        operand: &Expression,
        value: GoExpression,
        fallible: &Fallible,
    ) -> GoExpression {
        let Some(from) = fallible.err_ty() else {
            return value;
        };
        let to = self.queries().contextual_err_ty(from);
        let (setup, value) = self
            .propagated_error_coercion(operand, from, &to)
            .lower(self, value);
        statements.extend(setup);
        value
    }

    pub(in crate::lowering) fn failure_return_values(
        &self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> Vec<GoExpression> {
        let return_ctx = self.scope.current_return_ctx();
        if let Some((return_ty, shape)) = return_ctx.lowered() {
            return match (shape.failure(), error) {
                (LoweredFailure::Error { payload }, Some(error)) => {
                    self.lowered_err_values(payload, return_ty, error)
                }
                (LoweredFailure::Absent(option), None) => {
                    self.lowered_none_values(option, return_ty)
                }
                (failure, error) => unreachable!(
                    "the failure {failure:?} of the enclosing return does not fit the error {error:?}"
                ),
            };
        }
        vec![self.queries().contextual_failure(fallible, error)]
    }

    pub(in crate::lowering) fn failure_return(
        &self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> Statement {
        let lowered = self.scope.current_return_ctx().lowered_shape().is_some();
        let mut values = self.failure_return_values(fallible, error);
        if lowered {
            go_returns::multi_value_return(values)
        } else {
            plain_return(values.remove(0))
        }
    }

    pub(in crate::lowering) fn success_return(
        &mut self,
        fallible: &Fallible,
        value: GoExpression,
    ) -> Vec<Statement> {
        let Some(shape) = self.scope.current_return_ctx().lowered_shape() else {
            let success = self.queries().fallible_success(fallible, value);
            return vec![plain_return(success)];
        };
        let (mut statements, payload) =
            self.lowered_payload_values(&shape, fallible.ok_ty(), value);
        statements.push(go_returns::multi_value_return(shape.ok_values(payload)));
        statements
    }

    pub(in crate::lowering) fn lower_block_to_return(
        &mut self,
        expression: &Expression,
    ) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };

        let Some((last, rest)) = try_elide_tail_let(&self.facts, items, ElidableTail::Branching)
            .or_else(|| items.split_last())
        else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        LoweredBlock {
            statements: self.lower_block_items(rest, last, BlockTail::Value(TailTarget::Return)),
        }
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    pub(in crate::lowering) fn lower_return_value_tail(
        &mut self,
        last: &Expression,
    ) -> Vec<Statement> {
        match last {
            _ if let Some(branching) = Branching::of(last) => {
                self.lower_branching_to_block(branching, &PlacePlan::Return)
                    .statements
            }
            Expression::TryBlock { .. }
                if let Some(inlined) = self.try_lower_try_tail_in_place(last) =>
            {
                inlined
            }
            _ => {
                if let Some(tail) = self.try_lower_layout_return(last) {
                    tail
                } else if let Some(wrapped) = self.try_lower_wrapped_return(last) {
                    wrapped
                } else {
                    self.lower_plain_return_tail(last)
                }
            }
        }
    }

    fn lower_plain_return_tail(&mut self, last: &Expression) -> Vec<Statement> {
        let (mut statements, expression) = self.plan_tail_value(last);
        let expression = if self.queries().tail_tuple_return_slots(last).is_some() {
            expression
        } else {
            let return_ctx = self.scope.current_return_ctx();
            self.apply_type_coercion(&mut statements, return_ctx.ty(), last, expression)
        };
        statements.push(plain_return(expression));
        statements
    }
}

impl PlannerQueries<'_, '_> {
    fn returns_by_propagation(
        &self,
        expression: &Expression,
        lowered: Option<&LoweredReturnAbi>,
    ) -> bool {
        match lowered {
            Some(LoweredReturnAbi::Option(_)) => {
                !self.facts.converts(expression)
                    && matches!(
                        self.option_fuse_plan(expression),
                        Some(OptionFusePlan::Found { .. })
                    )
            }
            Some(LoweredReturnAbi::Result { .. } | LoweredReturnAbi::BareError) => {
                self.result_fuse_plan(expression)
                    .is_some_and(|plan| plan.wraps_error())
                    || self
                        .map_err_call(expression)
                        .is_some_and(|(plan, _)| plan.carries_payload())
            }
            _ => false,
        }
    }

    fn callee_matches_lowered_shape(
        &self,
        site: CallSite<'_>,
        enclosing_shape: &LoweredReturnAbi,
    ) -> bool {
        let plan = self.plan_call(site);
        plan.resolved.abi.result.lowered() == Some(enclosing_shape)
            && self.call_result_keeps_layout(&plan, &site.expression.get_type())
    }

    pub(in crate::lowering) fn return_fallible(&self) -> Option<Fallible> {
        self.facts.fallible(self.scope.current_return_ctx().ty()?)
    }

    pub(in crate::lowering) fn contextual_err_ty(&self, err_ty: &Type) -> Type {
        self.return_fallible()
            .and_then(|returned| returned.err_ty().cloned())
            .unwrap_or_else(|| err_ty.clone())
    }

    pub(in crate::lowering) fn fallible_call(
        &self,
        fallible: &Fallible,
        constructor: &str,
        arguments: Vec<GoExpression>,
    ) -> GoExpression {
        let type_args = self.format_type_arg_iter(
            [Some(fallible.ok_ty()), fallible.err_ty()]
                .into_iter()
                .flatten(),
        );
        prelude_call(constructor, type_args, arguments)
    }

    fn fallible_success(&self, fallible: &Fallible, value: GoExpression) -> GoExpression {
        self.fallible_call(fallible, fallible.ok_constructor(), vec![value])
    }

    fn contextual_failure(&self, fallible: &Fallible, error: Option<GoExpression>) -> GoExpression {
        let ok_ty = match self.scope.current_return_ctx().ty() {
            Some(ty) => self.facts.ok_payload(ty),
            None => fallible.ok_ty().clone(),
        };
        let err_ty = fallible
            .err_ty()
            .map(|err_ty| self.contextual_err_ty(err_ty));

        let type_args =
            self.format_type_arg_iter([Some(&ok_ty), err_ty.as_ref()].into_iter().flatten());
        let arguments = error.filter(|_| fallible.is_result()).into_iter().collect();
        prelude_call(fallible.err_constructor(), type_args, arguments)
    }
}
