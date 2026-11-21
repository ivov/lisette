use super::propagation::*;
use crate::Planner;
use crate::ReturnContext;
use crate::abi::callable::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::fallible_producer::OptionFusePlan;
use crate::abi::transition;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::calls::go_interop::LoweredProducer;
use crate::control_flow::fallible::{ConstructorKind, Fallible};
use crate::ir::bodies::{LoweredBlock, LoweredStatement, PlacePlan, Statement};
use crate::ir::values::GoExpression;
use crate::plan::calls::CallPlan;
use crate::state::expression::ExpressionContext;
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn build_return_plan(&mut self, expression: &Expression) -> LoweredBlock {
        let return_ctx = self.return_ctx();
        if self.facts.is_never(&expression.get_type()) {
            let mut statements = vec![self.lower_statement(expression)];
            if !matches!(return_ctx, ReturnContext::Void(_)) {
                statements.push(LoweredStatement::UnreachablePanic.into());
            }
            return LoweredBlock { statements };
        }
        let is_unit = return_ctx
            .ty()
            .is_some_and(|ty| self.facts.resolves_to_unit(ty));
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
                plain_return(GoExpression::unit())
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

    pub(crate) fn try_lower_wrapped_return(
        &mut self,
        expression: &Expression,
    ) -> Option<Vec<Statement>> {
        let return_ctx = self.return_ctx();
        let written_ty = return_ctx
            .ty()
            .filter(|ty| self.facts.fallible(ty).is_some())
            .cloned()
            .unwrap_or_else(|| expression.get_type());
        let fallible = self.facts.fallible(&written_ty)?;

        let mut statements = Vec::new();

        if self.facts.is_go_never(expression) {
            let (setup, call) = self
                .plan_call_expression(expression, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.push(
                LoweredStatement::ExpressionStatement {
                    expression: call,
                    diverges: true,
                }
                .into(),
            );
            return Some(statements);
        }

        if fallible.classify_constructor(expression) == Some(ConstructorKind::Failure) {
            statements.extend(self.lower_failure_constructor_return(&[], &fallible, &[]));
            return Some(statements);
        }

        if matches!(expression, Expression::Call { .. }) {
            statements.extend(self.lower_wrapped_call_return(expression, &fallible, &written_ty));
            return Some(statements);
        }

        if matches!(
            expression,
            Expression::If { .. } | Expression::IfLet { .. } | Expression::Match { .. }
        ) {
            let block = self.lower_branching_to_block(expression, &PlacePlan::Return);
            statements.extend(block.statements);
            return Some(statements);
        }

        if let Expression::Propagate {
            expression: inner, ..
        } = expression
        {
            let (setup, value) = self.plan_propagate(inner);
            statements.extend(setup);
            statements.extend(self.wrapped_value_return(value, &written_ty));
            return Some(statements);
        }

        let (setup, value) = self
            .plan_value(expression, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        statements.extend(self.wrapped_value_return(value, &written_ty));
        Some(statements)
    }

    fn wrapped_value_return(&mut self, value: GoExpression, return_ty: &Type) -> Vec<Statement> {
        let Some(shape) = self.return_ctx().lowered_shape() else {
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
        match self.return_ctx().lowered_shape().as_ref() {
            Some(LoweredReturnAbi::BareError) => {
                if !args.is_empty() {
                    let (setup, _) = self
                        .plan_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                }
                statements.push(transition::multi_value_return(
                    LoweredReturnAbi::BareError.ok_values(Vec::new()),
                ));
            }
            Some(shape) if args.is_empty() => {
                statements.push(transition::multi_value_return(
                    shape.ok_values(vec![GoExpression::unit()]),
                ));
            }
            Some(shape)
                if shape.has_flattened_payload()
                    && let tuple @ Expression::Tuple { .. } = args[0].unwrap_parens() =>
            {
                let (setup, parts) = self.lowered_tuple_literal_values(tuple, fallible.ok_ty());
                statements.extend(setup);
                statements.push(transition::multi_value_return(shape.ok_values(parts)));
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
            let (message_setup, wraps) = self.prepare_wrap_messages(wraps, true);
            let error = if message_setup.is_empty() {
                error
            } else {
                self.eager_operand(arg, error, "err")
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
        let lowered = self.return_ctx().lowered_shape();
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
            statements.push(transition::multi_value_return(vec![value, ok]));
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
            let abi = plan.resolved.abi.clone();
            if self.go_result_bridge(&abi, return_ty).is_none() {
                let (setup, call) = self
                    .plan_call_expression(expression, None, ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                statements.extend(self.lower_abi_to_tagged_return(
                    call,
                    LoweredProducer {
                        abi: &abi.result,
                        producer: plan.resolved.origin.producer(),
                    },
                    return_ty,
                ));
            } else {
                let (setup, result_var) = self
                    .plan_go_abi_wrapped_call(expression, &abi, plan.resolved.origin, return_ty)
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
            && self.call_result_unbridged(&plan, &call_expression.get_type())
    }
}
