use crate::lowering::calls::arguments::CallArgumentsContext;
use crate::lowering::calls::direct_calls::receiver_type_binding;
use crate::lowering::calls::type_arguments::{CallArgumentShape, CallTypeArgumentsRequest};
use crate::lowering::expressions::access::dot_access_site::DotAccessSite;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::names::go_name::GoExport;

use crate::Planner;
use crate::lowering::abi::coercion_plan::CoercionPlan;
use crate::lowering::calls::call_plan::{CallPlan, ResolvedCallee};
use crate::lowering::expressions::operand_order::SpreadSequenceOptions;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::Statement;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::values::{CaptureBoundary, GoExpression, Stability};
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, Literal};
use syntax::program::ReceiverCoercion;
use syntax::types::Type;

impl Planner<'_> {
    pub(super) fn plan_ufcs_call(
        &mut self,
        callee: DotAccessSite<'_>,
        call_plan: &CallPlan<'_, '_>,
    ) -> ValuePlan {
        let function = callee.expression;
        let receiver = callee.receiver;
        let member = callee.member;
        let args = call_plan.site.args;
        let spread = call_plan.site.spread;
        let written_type_arguments = call_plan.site.written_type_arguments();

        let type_args_string = self
            .queries()
            .call_type_arguments(CallTypeArgumentsRequest {
                function,
                callee: &call_plan.resolved,
                written_type_arguments,
                call_ty: None,
                arg_shape: CallArgumentShape {
                    value_count: args.len(),
                    has_spread: spread.is_some(),
                },
                slot: None,
                receiver_passed: true,
            });

        let receiver_operand = self.plan_ufcs_receiver(callee, &call_plan.resolved);
        let args_ctx = CallArgumentsContext {
            plan: call_plan,
            spread,
            capture_boundary: CaptureBoundary::SiblingSequence,
            assigned_place: None,
            callee_is_builtin: false,
            callee_pins_type_args: type_args_string.is_some(),
            receiver_binding: (!call_plan.resolved.is_prelude_dispatch)
                .then(|| receiver_type_binding(&self.facts, function, &call_plan.resolved))
                .flatten(),
        };

        let (setup, receiver_arg, emitted_args) =
            self.lower_ufcs_call_args(&args_ctx, receiver_operand, args);

        let mut new_args = vec![receiver_arg];
        new_args.extend(emitted_args);

        let callee = self.queries().build_ufcs_qualified_call(
            &call_plan.resolved,
            &receiver.get_type(),
            member,
            type_args_string,
        );

        let expression = GoExpression::call(callee, new_args);
        if self.queries().callee_lowers_to_type_construction(function) {
            ValuePlan::computed(setup, expression)
        } else {
            ValuePlan::plain_call(setup, expression)
        }
    }

    fn plan_ufcs_receiver(
        &mut self,
        callee: DotAccessSite<'_>,
        resolved: &ResolvedCallee<'_>,
    ) -> ValuePlan {
        let receiver = callee.receiver;
        let mut coercion = self.facts.dot_receiver_coercion(receiver, callee.member);
        let mut receiver_operand = self.plan_operand(receiver, ExpressionContext::value());
        if resolved.is_prelude_dispatch && coercion != Some(ReceiverCoercion::AutoAddress) {
            let source = self.facts.dereferenced_layout(&receiver.get_type());
            let target = self.facts.receiver_layout(&receiver.get_type());
            let change = CoercionPlan::change(&self.queries(), &source, &target);
            if change.changes_layout() {
                receiver_operand = receiver_operand.map_expression(|setup, value| {
                    let value = if coercion == Some(ReceiverCoercion::AutoDeref) {
                        GoExpression::dereference(value)
                    } else {
                        value
                    };
                    let (layout_change_setup, value) = change.lower(self, value);
                    setup.extend(layout_change_setup);
                    value
                });
                coercion = None;
            }
        }
        match coercion {
            Some(ReceiverCoercion::AutoAddress) => {
                self.coerce_receiver_address_operand(receiver, receiver_operand)
            }
            Some(ReceiverCoercion::AutoDeref) => receiver_operand.dereference(),
            None => receiver_operand,
        }
    }

    fn lower_ufcs_call_args(
        &mut self,
        ctx: &CallArgumentsContext<'_, '_>,
        receiver_operand: ValuePlan,
        args: &[Expression],
    ) -> (Vec<Statement>, GoExpression, Vec<GoExpression>) {
        let callee = &ctx.plan.resolved;
        let spread = ctx.spread;
        let sequenced = if callee.is_prelude_dispatch {
            let mut operands = Vec::with_capacity(1 + args.len() + spread.is_some() as usize);
            operands.push(receiver_operand);
            for (i, arg) in args.iter().enumerate() {
                let param = callee.abi.param(i);
                operands.push(self.plan_prelude_arg(
                    arg,
                    param.and_then(|param| param.declared.as_ref()),
                    param.map(|param| &param.instantiated),
                ));
            }
            let spread_operand =
                spread.map(|spread| self.plan_operand(spread, ExpressionContext::value()));
            self.sequence_with_spread_values(
                operands,
                spread_operand,
                SpreadSequenceOptions {
                    wrap_to_any: callee.abi.spread_wraps_to_any(&self.facts, spread),
                    combine: callee.abi.variadic_combine(1),
                    boundary: CaptureBoundary::SiblingSequence,
                },
            )
        } else {
            self.plan_call_args(ctx, Some(receiver_operand))
        };

        let mut all_values = sequenced.values;
        let receiver_arg = all_values.remove(0);
        (sequenced.setup, receiver_arg, all_values)
    }

    fn coerce_receiver_address_operand(
        &mut self,
        receiver: &Expression,
        operand: ValuePlan,
    ) -> ValuePlan {
        if matches!(receiver, Expression::Call { .. }) {
            return operand
                .map_expression(|setup, value| {
                    let temp = self.hoist_temp(setup, "ref", value);
                    GoExpression::address_of(GoExpression::identifier(temp))
                })
                .with_stability(Stability::StableAcrossCalls);
        }
        if matches!(receiver, Expression::Identifier { .. }) {
            return operand
                .map_expression(|_, value| GoExpression::address_of(value))
                .with_stability(Stability::StableAcrossCalls);
        }
        if operand.setup().is_empty() {
            return operand.map_observable_expression(|_, value| GoExpression::address_of(value));
        }
        let (mut setup, value) = operand.into_parts();
        let addressed = GoExpression::address_of(value);
        let temp = self.hoist_temp(&mut setup, "ref", addressed);
        ValuePlan::captured(setup, temp)
    }

    pub(super) fn plan_receiver_method_ufcs(
        &mut self,
        export: GoExport,
        call_plan: &CallPlan<'_, '_>,
    ) -> ValuePlan {
        let args = call_plan.site.args;
        let go_method =
            self.queries()
                .receiver_method_ufcs_go_name(call_plan.site.callee, call_plan, export);
        let args_ctx = CallArgumentsContext {
            plan: call_plan,
            spread: call_plan.site.spread,
            capture_boundary: CaptureBoundary::SiblingSequence,
            assigned_place: None,
            callee_is_builtin: false,
            callee_pins_type_args: false,
            receiver_binding: None,
        };

        let sequenced = self.plan_call_args(&args_ctx, None);
        let mut emitted_all = sequenced.values;
        let receiver = emitted_all.remove(0);

        let receiver = if let GoExpressionNode::AddressOf(inner) = receiver.node()
            && !is_address_of_composite_literal(args.first())
        {
            receiver.peeled(inner)
        } else {
            receiver
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
    }) = arg
    else {
        return false;
    };

    matches!(
        &**inner,
        Expression::StructCall { .. }
            | Expression::Literal {
                literal: Literal::Slice(_),
                ..
            }
    )
}

impl PlannerQueries<'_, '_> {
    fn build_ufcs_qualified_call(
        &self,
        callee: &ResolvedCallee<'_>,
        receiver_ty: &Type,
        member: &str,
        type_args_string: Option<GoType>,
    ) -> GoExpression {
        let qualified_name = self
            .facts
            .receiver_nominal_id(receiver_ty)
            .unwrap_or_else(|| unreachable!("UFCS receiver must be a constructor type"));
        let declaration = callee
            .declaration
            .expect("a UFCS callee names a declaration");
        let export = GoExport::from_visibility(*declaration.visibility());

        let qualified_method_name = self.qualify_method_call(&qualified_name, member, export);
        GoExpression::instantiation(qualified_method_name, type_args_string)
    }
}
