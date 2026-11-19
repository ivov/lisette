use crate::calls::arguments::CallArgsContext;
use crate::calls::dispatch::{CallArgShape, all_type_params_inferrable};
use crate::calls::regular::receiver_type_binding;

use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::SpreadSequenceOptions;
use crate::plan::bodies::Statement;
use crate::plan::calls::{CallPlan, ResolvedCallee};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{CaptureBoundary, GoExpression, Stability, ValuePlan};
use crate::types::go_type::GoType;
use syntax::ast::{Expression, Literal};
use syntax::program::ReceiverCoercion;
use syntax::program::SlotOrigin;
use syntax::types::Type;

impl Planner<'_> {
    fn ufcs_type_args(
        &mut self,
        function: &Expression,
        callee: &ResolvedCallee<'_>,
        written_type_arguments: usize,
        arg_shape: CallArgShape,
    ) -> Option<GoType> {
        let definition_ty = callee.declared_type()?;

        let Type::Forall { vars, body } = &definition_ty else {
            return None;
        };
        let Type::Function(f) = body.as_ref() else {
            return None;
        };

        let receiver_count = callee.receiver_offset.min(1);
        if written_type_arguments == 0
            && all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
        {
            return None;
        }
        let type_arguments = &self.facts.checked(function).callee().type_arguments;
        self.format_generic_instantiation(definition_ty, type_arguments)
    }

    pub(super) fn lower_ufcs_call(
        &mut self,
        function: &Expression,
        args: &[Expression],
        written_type_arguments: usize,
        spread: Option<&Expression>,
        call_plan: &CallPlan<'_>,
    ) -> ValuePlan {
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = function
        else {
            unreachable!("lower_ufcs_call called on non-DotAccess");
        };

        let mut coercion = self.facts.checked(receiver).receiver_change();
        let receiver_ty = self.facts.strip_and_peel(&receiver.get_type());
        let type_args_string = self
            .ufcs_type_args(
                function,
                &call_plan.resolved,
                written_type_arguments,
                CallArgShape {
                    value_count: args.len(),
                    has_spread: spread.is_some(),
                },
            )
            .unwrap_or_default();

        let mut receiver_stage = self.plan_operand(receiver, ExpressionContext::value());
        if call_plan.resolved.is_prelude_dispatch && coercion != Some(ReceiverCoercion::AutoAddress)
        {
            let source = self.value_layout(&receiver.get_type().strip_refs(), SlotOrigin::Lisette);
            let target = self.value_layout(&receiver_ty, SlotOrigin::Lisette);
            let bridge = CoercionPlan::bridge(self, &source, &target);
            if !bridge.is_identity() {
                receiver_stage = receiver_stage.map_expression(|setup, value| {
                    let value = if coercion == Some(ReceiverCoercion::AutoDeref) {
                        GoExpression::dereference(value)
                    } else {
                        value
                    };
                    let (bridge_setup, value) = bridge.lower(self, value);
                    setup.extend(bridge_setup);
                    value
                });
                coercion = None;
            }
        }
        if coercion == Some(ReceiverCoercion::AutoAddress) {
            receiver_stage = self.coerce_receiver_address_stage(receiver, receiver_stage);
        } else if coercion == Some(ReceiverCoercion::AutoDeref) {
            receiver_stage = receiver_stage.dereference();
        }
        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
            callee_is_builtin: false,
            callee_pins_type_args: !type_args_string.is_empty(),
            receiver_binding: (!call_plan.resolved.is_prelude_dispatch)
                .then(|| receiver_type_binding(function, &call_plan.resolved))
                .flatten(),
        };
        let (setup, receiver_arg, emitted_args) =
            self.lower_ufcs_call_args(&args_ctx, receiver_stage, args);

        let mut new_args = vec![receiver_arg];
        new_args.extend(emitted_args);

        let callee = self.build_ufcs_qualified_call(
            &call_plan.resolved,
            &receiver_ty,
            member,
            type_args_string,
        );
        let expression = GoExpression::call(callee, new_args);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(setup, expression)
        } else {
            ValuePlan::plain_call(setup, expression)
        }
    }

    fn lower_ufcs_call_args(
        &mut self,
        ctx: &CallArgsContext<'_, '_>,
        receiver_stage: ValuePlan,
        args: &[Expression],
    ) -> (Vec<Statement>, GoExpression, Vec<GoExpression>) {
        let callee = &ctx.plan.resolved;
        let spread = ctx.spread;
        let sequenced = if callee.is_prelude_dispatch {
            let mut stages = Vec::with_capacity(1 + args.len() + spread.is_some() as usize);
            stages.push(receiver_stage);
            for (i, arg) in args.iter().enumerate() {
                let param = callee.abi.param(i);
                stages.push(self.stage_prelude_arg(
                    arg,
                    param.and_then(|param| param.declared.as_ref()),
                    param.map(|param| &param.instantiated),
                ));
            }
            let spread_stage =
                spread.map(|spread| self.plan_operand(spread, ExpressionContext::value()));
            self.sequence_with_spread_values(
                stages,
                spread_stage,
                SpreadSequenceOptions {
                    wrap_to_any: callee.abi.spread_wraps_to_any(&self.facts, spread),
                    combine: callee.abi.variadic_combine(1),
                    boundary: CaptureBoundary::SiblingSequence,
                },
            )
        } else {
            self.emit_call_args(args, ctx, Some(receiver_stage))
        };
        let mut all_values = sequenced.values;
        let receiver_arg = all_values.remove(0);
        (sequenced.setup, receiver_arg, all_values)
    }

    fn build_ufcs_qualified_call(
        &mut self,
        callee: &ResolvedCallee<'_>,
        receiver_ty: &Type,
        member: &str,
        type_args_string: GoType,
    ) -> GoExpression {
        let Type::Nominal {
            id: qualified_name, ..
        } = receiver_ty
        else {
            unreachable!("UFCS receiver must be a constructor type");
        };
        let is_public = callee
            .declaration
            .map(|declaration| declaration.visibility().is_public())
            .unwrap_or(false);

        let qualified_method_name = self.qualify_method_call(qualified_name, member, is_public);
        GoExpression::instantiation(qualified_method_name, type_args_string)
    }

    fn coerce_receiver_address_stage(
        &mut self,
        receiver: &Expression,
        stage: ValuePlan,
    ) -> ValuePlan {
        match receiver.unwrap_parens() {
            Expression::Call { .. } => stage
                .map_expression(|setup, value| {
                    let temp = self.hoist_tmp_value_statement(setup, "ref", value);
                    GoExpression::address_of(GoExpression::identifier(temp))
                })
                .with_stability(Stability::StableAcrossCalls),
            Expression::Identifier { .. } => stage
                .map_expression(|_, value| GoExpression::address_of(value))
                .with_stability(Stability::StableAcrossCalls),
            _ if stage.setup().is_empty() => {
                stage.map_observable_expression(|_, value| GoExpression::address_of(value))
            }
            _ => {
                let (mut setup, value) = stage.into_parts();
                let addressed = GoExpression::address_of(value);
                let temp = self.hoist_tmp_value_statement(&mut setup, "ref", addressed);
                ValuePlan::captured(setup, temp)
            }
        }
    }

    pub(super) fn lower_receiver_method_ufcs(
        &mut self,
        args: &[Expression],
        go_method: String,
        spread: Option<&Expression>,
        call_plan: &CallPlan<'_>,
    ) -> ValuePlan {
        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
            callee_is_builtin: false,
            callee_pins_type_args: false,
            receiver_binding: None,
        };
        let sequenced = self.emit_call_args(args, &args_ctx, None);
        let mut emitted_all = sequenced.values;
        let receiver = emitted_all.remove(0);

        let receiver = match receiver.node() {
            GoExpressionNode::AddressOf(inner)
                if !is_address_of_composite_literal(args.first()) =>
            {
                receiver.peeled(inner)
            }
            _ => receiver,
        };

        ValuePlan::computed(
            sequenced.setup,
            GoExpression::call(GoExpression::selector(receiver, go_method), emitted_all),
        )
    }
}

fn is_address_of_composite_literal(arg: Option<&Expression>) -> bool {
    let Some(Expression::Reference {
        expression: inner, ..
    }) = arg.map(Expression::unwrap_parens)
    else {
        return false;
    };
    matches!(
        inner.unwrap_parens(),
        Expression::StructCall { .. }
            | Expression::Literal {
                literal: Literal::Slice(_),
                ..
            }
    )
}
