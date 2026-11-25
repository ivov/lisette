use crate::Planner;
use crate::lookup::state::enclosing::ReturnContext;
use crate::lowering::abi::fallible_producer::OptionFusePlan;
use crate::lowering::abi::go_returns;
use crate::lowering::calls::call_plan::CallPlan;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::lowering::calls::go_interop::LoweredProducer;
use crate::lowering::calls::wrap_err::ErrorUse;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::definitions::functions::is_go_never;
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::PlacePlan;
use crate::lowering::statements::placement::try_elide_tail_let;
use crate::lowering::statements::placement::{ElidableTail, requires_temp_var};
use crate::target::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, directed_first, diverging_statement,
};
use crate::target::ir::prelude::{ConstructorKind, Fallible, prelude_call};
use crate::target::ir::values::GoExpression;
use crate::target::types::return_abi::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use std::slice;
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn build_return_plan(
        &mut self,
        expression: &Expression,
    ) -> LoweredBlock {
        let return_ctx = self.scope.current_return_ctx();
        if self.facts.is_never(&expression.get_type()) {
            let mut statements = vec![self.lower_never_value(expression)];
            if !matches!(return_ctx, ReturnContext::Void(_)) {
                statements.push(LoweredStatement::UnreachablePanic.into());
            }
            return LoweredBlock { statements };
        }
        let is_unit = return_ctx
            .ty()
            .is_some_and(|ty| self.facts.resolves_to_unit(ty))
            || self.facts.resolves_to_unit(&expression.get_type());
        if is_unit {
            let is_pure = matches!(
                expression,
                Expression::Unit { .. }
                    | Expression::Identifier { .. }
                    | Expression::Literal { .. }
            );

            let mut statements = Vec::new();
            if !is_pure {
                statements.push(self.lower_statement(expression));
            }
            statements.push(if matches!(return_ctx, ReturnContext::Void(_)) {
                LoweredStatement::Return(Vec::new()).into()
            } else {
                plain_return(self.unit_value(Some(expression), return_ctx.ty()))
            });
            return LoweredBlock { statements };
        }

        if let Some(statements) = self.try_lower_layout_return(expression) {
            return LoweredBlock { statements };
        }

        if let Some(statements) = self.try_lower_wrapped_return(expression) {
            return LoweredBlock { statements };
        }

        let (mut statements, value) = self
            .plan_value(expression, ExpressionContext::value())
            .into_parts();
        let value = self.apply_type_coercion(&mut statements, return_ctx.ty(), expression, value);
        statements.push(plain_return(value));
        LoweredBlock { statements }
    }

    pub(in crate::lowering) fn try_lower_wrapped_return(
        &mut self,
        expression: &Expression,
    ) -> Option<Vec<Statement>> {
        let return_ctx = self.scope.current_return_ctx();
        let written_ty = return_ctx
            .ty()
            .filter(|ty| self.facts.fallible(ty).is_some())
            .cloned()
            .unwrap_or_else(|| expression.get_type());
        let fallible = self.facts.fallible(&written_ty)?;

        if is_go_never(&self.facts, expression) {
            let (mut statements, call) = self
                .plan_call_expression(expression, None, ExpressionContext::value())
                .into_parts();
            statements.push(diverging_statement(call));
            return Some(statements);
        }

        if fallible.classify_constructor(expression) == Some(ConstructorKind::Failure) {
            return Some(self.lower_failure_constructor_return(&[], &fallible, &[]));
        }

        if matches!(
            expression,
            Expression::If { .. } | Expression::IfLet { .. } | Expression::Match { .. }
        ) {
            return Some(
                self.lower_branching_to_block(expression, &PlacePlan::Return)
                    .statements,
            );
        }

        let coercion = self.value_slot_coercion(expression, &written_ty);
        if coercion.is_identity() && matches!(expression, Expression::Call { .. }) {
            return Some(self.lower_wrapped_call_return(expression, &fallible, &written_ty));
        }

        let (mut statements, value) = match expression {
            Expression::Propagate {
                expression: inner, ..
            } if coercion.is_identity() => self.plan_propagate(inner),
            _ => self
                .plan_value(expression, ExpressionContext::value())
                .into_parts(),
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
        expression: &Expression,
        fallible: &Fallible,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let Expression::Call {
            expression: call_expression,
            args,
            ..
        } = expression
        else {
            unreachable!("lower_wrapped_call_return requires a Call expression");
        };

        let (inner, wraps) = self.peel_wrap_err(expression);
        if let Expression::Call {
            expression: inner_callee,
            args: inner_args,
            ..
        } = inner
            && !wraps.is_empty()
            && fallible.classify_constructor(inner_callee) == Some(ConstructorKind::Failure)
        {
            return self.lower_failure_constructor_return(inner_args, fallible, &wraps);
        }
        match fallible.classify_constructor(call_expression) {
            Some(ConstructorKind::Success) => self.lower_success_constructor_return(args, fallible),
            Some(ConstructorKind::Failure) => {
                self.lower_failure_constructor_return(args, fallible, &[])
            }
            None => self.lower_wrapped_passthrough_return(expression, return_ty),
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
                    && let tuple @ Expression::Tuple { .. } = &args[0] =>
            {
                let (setup, parts) = self.lowered_tuple_literal_values(tuple, fallible.ok_ty());
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
        wraps: &[&Expression],
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        let error = args.first().map(|arg| {
            let error = self.plan_composite_value(arg, ExpressionContext::value());
            let (message_setup, wraps) = self.prepare_wrap_messages(wraps, ErrorUse::Read);
            let error = if message_setup.is_empty() {
                error
            } else {
                self.eager_operand(error, "err")
            };
            let (setup, error) = error.into_parts();
            statements.extend(setup);
            let to = self
                .contextual_err_ty(fallible)
                .expect("Result must have error type");
            let error = self.apply_type_coercion(&mut statements, Some(&to), arg, error);
            statements.extend(message_setup);
            self.wrap_error(&wraps, error)
        });

        statements.push(self.failure_return(fallible, error));
        statements
    }

    fn lower_propagated_return(
        &mut self,
        expression: &Expression,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let (mut statements, value) = self.plan_propagate(expression);
        let fallible = self
            .facts
            .fallible(return_ty)
            .expect("a propagated return has a fallible return type");
        statements.extend(self.success_return(&fallible, value));
        statements
    }

    fn lower_wrapped_passthrough_return(
        &mut self,
        expression: &Expression,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let lowered = self.scope.current_return_ctx().lowered_shape();
        let lowered = lowered.as_ref();
        let mut statements = Vec::new();
        if let Some(LoweredReturnAbi::Option(_)) = lowered
            && !self.facts.converts(expression)
            && matches!(
                self.option_fuse_plan(expression),
                Some(OptionFusePlan::Found { .. })
            )
        {
            return self.lower_propagated_return(expression, return_ty);
        }
        if matches!(
            lowered,
            Some(LoweredReturnAbi::Result { .. } | LoweredReturnAbi::BareError)
        ) && (self
            .result_fuse_plan(expression)
            .is_some_and(|plan| plan.wraps_error())
            || self
                .map_err_call(expression)
                .is_some_and(|(plan, _)| plan.carries_payload()))
        {
            return self.lower_propagated_return(expression, return_ty);
        }
        if let Some(shape) = lowered
            && self.callee_matches_lowered_shape(expression, shape)
        {
            let (setup, call) = self
                .plan_call_expression(expression, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.push(plain_return(call));
            return statements;
        }
        if let Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        })) = lowered
            && let Some(source) = self.comma_ok_source(expression)
            && !source.has_nil_guard()
        {
            let pair = self.bind_comma_ok_pair(expression, source, CommaOkValueSlot::Temp);
            let ok = GoExpression::identifier(pair.status_identifier().clone());
            let value = GoExpression::identifier(
                pair.payload_identifier()
                    .expect("Temp slot always captures the value")
                    .clone(),
            );
            let mut statements = pair.statements;
            statements.push(go_returns::multi_value_return(vec![value, ok]));
            return statements;
        }
        if let Some(plan) = self.plan_call(expression)
            && plan.resolved.abi.result.is_lowered()
        {
            return self.lower_lowered_call_return(expression, &plan, lowered, return_ty);
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
            .plan_call_expression(expression, None, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        statements.push(plain_return(call));
        statements
    }

    fn lower_lowered_call_return(
        &mut self,
        expression: &Expression,
        plan: &CallPlan<'_>,
        lowered: Option<&LoweredReturnAbi>,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered {
            let (setup, result_var) = self
                .plan_go_abi_wrapped_call(
                    expression,
                    &plan.resolved.abi,
                    plan.resolved.origin,
                    return_ty,
                )
                .into_parts();

            statements.extend(setup);
            statements.extend(self.lower_result_return(result_var, "res", return_ty, shape));
        } else {
            let abi = &plan.resolved.abi;
            if self.go_result_layout_change(abi, return_ty).is_none() {
                let (setup, call) = self
                    .plan_call_expression(expression, None, ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                statements.extend(self.lower_abi_to_tagged_return(
                    call,
                    LoweredProducer::of_call(abi, plan.resolved.origin),
                    return_ty,
                ));
            } else {
                let (setup, result_var) = self
                    .plan_go_abi_wrapped_call(expression, abi, plan.resolved.origin, return_ty)
                    .into_parts();
                statements.extend(setup);
                statements.push(plain_return(result_var));
            }
        }
        statements
    }

    fn callee_matches_lowered_shape(
        &self,
        call_expression: &Expression,
        enclosing_shape: &LoweredReturnAbi,
    ) -> bool {
        let Some(plan) = self.plan_call(call_expression) else {
            return false;
        };
        plan.resolved.abi.result.lowered() == Some(enclosing_shape)
            && self.call_result_keeps_layout(&plan, &call_expression.get_type())
    }

    pub(in crate::lowering) fn return_fallible(&self) -> Option<Fallible> {
        self.facts.fallible(self.scope.current_return_ctx().ty()?)
    }

    pub(in crate::lowering) fn contextual_err_ty(&self, fallible: &Fallible) -> Option<Type> {
        self.return_fallible()
            .and_then(|returned| returned.err_ty().cloned())
            .or_else(|| fallible.err_ty().cloned())
    }

    pub(in crate::lowering) fn convert_error_to_return_context(
        &mut self,
        statements: &mut Vec<Statement>,
        operand: &Expression,
        value: GoExpression,
        fallible: &Fallible,
    ) -> GoExpression {
        let (Some(from), Some(to)) = (fallible.err_ty().cloned(), self.contextual_err_ty(fallible))
        else {
            return value;
        };
        let (setup, value) = self
            .propagated_error_coercion(operand, &from, &to)
            .lower(self, value);
        statements.extend(setup);
        value
    }

    pub(in crate::lowering) fn failure_return_values(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> Vec<GoExpression> {
        let return_ctx = self.scope.current_return_ctx();
        if let Some(shape) = return_ctx.lowered_shape() {
            let return_ty = return_ctx.expect_ty();
            return match error {
                Some(error) if fallible.is_result() => {
                    self.lowered_err_values(&shape, &return_ty, error)
                }
                _ => self.lowered_none_values(&shape, &return_ty),
            };
        }
        vec![self.contextual_failure(fallible, error)]
    }

    pub(in crate::lowering) fn failure_return(
        &mut self,
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
            let success = self.fallible_success(fallible, value);
            return vec![plain_return(success)];
        };
        let (mut statements, payload) =
            self.lowered_payload_values(&shape, fallible.ok_ty(), value);
        statements.push(go_returns::multi_value_return(shape.ok_values(payload)));
        statements
    }

    pub(in crate::lowering) fn fallible_call(
        &mut self,
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

    fn fallible_success(&mut self, fallible: &Fallible, value: GoExpression) -> GoExpression {
        self.fallible_call(fallible, fallible.ok_constructor(), vec![value])
    }

    fn contextual_failure(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> GoExpression {
        let ok_ty = match self.scope.current_return_ctx().ty() {
            Some(ty) => self.facts.ok_payload(ty),
            None => fallible.ok_ty().clone(),
        };
        let err_ty = fallible.is_result().then(|| {
            self.contextual_err_ty(fallible)
                .expect("Result must have error type")
        });
        let type_args =
            self.format_type_arg_iter([Some(&ok_ty), err_ty.as_ref()].into_iter().flatten());
        let arguments = error.filter(|_| fallible.is_result()).into_iter().collect();
        prelude_call(fallible.err_constructor(), type_args, arguments)
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

        let (statements, _) = self.lower_body_until_diverge(rest, last);
        LoweredBlock { statements }
    }

    pub(in crate::lowering) fn lower_return_tail(&mut self, last: &Expression) -> Vec<Statement> {
        let mut statements = Vec::new();
        let return_span = last.get_span();
        let last = if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        };

        if self.facts.resolves_to_unit(&last.get_type()) {
            if !matches!(last, Expression::Unit { .. }) {
                statements.push(self.lower_statement(last));
            }
            return statements;
        }

        if self.facts.is_never(&last.get_type()) {
            let line = self.facts.source_position(&return_span);
            return directed_first(line, vec![self.lower_never_value(last)]);
        }

        let line = self.facts.source_position(&return_span);
        match last {
            Expression::If { .. } | Expression::Select { .. } => {
                let mut block = self.lower_branching_to_block(last, &PlacePlan::Return);
                let statement = block
                    .statements
                    .pop()
                    .expect("if and select lower to one statement");
                statements.extend(directed_first(line, vec![statement]));
            }
            Expression::IfLet { .. } | Expression::Match { .. } => {
                let block = self.lower_branching_to_block(last, &PlacePlan::Return);
                statements.extend(directed_first(line, block.statements));
            }
            Expression::TryBlock { .. }
                if let Some(inlined) = self.try_lower_try_tail_in_place(last) =>
            {
                statements.extend(directed_first(line, inlined));
            }
            _ => {
                let tail = if let Some(tail) = self.try_lower_layout_return(last) {
                    tail
                } else if let Some(wrapped) = self.try_lower_wrapped_return(last) {
                    wrapped
                } else {
                    self.lower_plain_return_tail(last)
                };

                statements.extend(directed_first(line, tail));
            }
        }

        statements
    }

    fn lower_plain_return_tail(&mut self, last: &Expression) -> Vec<Statement> {
        if requires_temp_var(last) {
            let operand = self.plan_operand(last, ExpressionContext::value());
            let (mut statements, value) = operand.into_parts();
            if !value.is_empty() {
                statements.push(plain_return(value));
            }
            statements
        } else {
            let (mut statements, expression) = self.plan_tail_value(last);
            let expression = if self.tail_tuple_return_slots(last).is_some() {
                expression
            } else {
                let return_ctx = self.scope.current_return_ctx();
                self.apply_type_coercion(&mut statements, return_ctx.ty(), last, expression)
            };

            statements.push(plain_return(expression));
            statements
        }
    }
}
