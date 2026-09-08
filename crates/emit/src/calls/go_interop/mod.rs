mod nullable;
mod wrappers;

pub(crate) use wrappers::{NilGuard, WrapperTarget, is_nil, non_nil, unexpected_nil_error};

use crate::Planner;
use crate::abi::callable::{CallableAbi, CallableReturnAbi, OptionReturnAbi};
use crate::abi::coercion::{CoercionPlan, LayoutBridge, resolve_layout_bridge};
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::context::expression::ExpressionContext;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{LoweredStatement, define_many};
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    /// Lower a raw callable result through its canonical physical ABI.
    pub(crate) fn lower_go_abi_wrapped_call(
        &mut self,
        call_expression: &Expression,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> ValuePlan {
        if let Some(bridges) = self.go_tuple_result_bridges(abi, result_ty) {
            let call = self.lower_call(call_expression, None, ExpressionContext::value());
            return call.map_observable_expression(|setup, call| {
                let values = self.create_temp_vars("ret", bridges.len());
                setup.push(define_many(values.clone(), call));
                let values = values
                    .into_iter()
                    .zip(&bridges)
                    .map(|(value, bridge)| {
                        self.plan_layout_bridge(setup, GoExpression::name(value), bridge)
                    })
                    .collect::<Vec<_>>();
                self.plan_tuple_from_vars(setup, values)
            });
        }

        if let Some(bridge) = self.go_result_layout_bridge(abi, result_ty) {
            let call = self.lower_call(call_expression, None, ExpressionContext::value());
            return call.map_observable_expression(|setup, call| {
                let (bridge_setup, value) = bridge.lower(self, call);
                setup.extend(bridge_setup);
                value
            });
        }

        let payload_bridge = self.go_return_payload_bridge(abi, result_ty);
        let call_plan = self.lower_call(call_expression, None, ExpressionContext::value());
        call_plan.map_observable_expression(|setup, call| {
            let (wrap, value) = if payload_bridge.is_some() {
                let (wrap, outcome) = self.lower_abi_wrapping_with_payload_bridge(
                    call,
                    &abi.result,
                    result_ty,
                    payload_bridge.as_ref(),
                    WrapperTarget::FreshSlot,
                );
                (
                    wrap,
                    GoExpression::name(outcome.expect("wrapper produced no slot")),
                )
            } else {
                self.lower_abi_to_tagged(call, &abi.result, result_ty)
            };
            setup.extend(wrap);
            value
        })
    }

    pub(crate) fn go_result_layout_bridge(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<CoercionPlan> {
        let target = self.value_layout(result_ty, SlotOrigin::Lisette);
        match abi.result {
            CallableReturnAbi::Direct => {}
            CallableReturnAbi::Option(OptionReturnAbi::Nullable) => {
                let source_payload = abi.return_layout.option_payload()?;
                let target_payload = target.option_payload()?;
                if source_payload.same_representation(target_payload) {
                    return None;
                }
            }
            _ => return None,
        }
        let bridge = CoercionPlan::bridge(self, &abi.return_layout, &target);
        (!bridge.is_identity()).then_some(bridge)
    }

    pub(crate) fn go_tuple_result_bridges(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<Vec<LayoutBridge>> {
        if !matches!(abi.result, CallableReturnAbi::Tuple { .. }) {
            return None;
        }
        let ValueLayout::Tuple {
            elements: source, ..
        } = &abi.return_layout
        else {
            return None;
        };
        let ValueLayout::Tuple {
            elements: target, ..
        } = self.value_layout(result_ty, SlotOrigin::Lisette)
        else {
            return None;
        };
        if source.len() != target.len() {
            return None;
        }
        let bridges = source
            .iter()
            .zip(&target)
            .map(|(source, target)| resolve_layout_bridge(self, source, target))
            .collect::<Vec<_>>();
        bridges
            .iter()
            .any(|bridge| !bridge.is_identity())
            .then_some(bridges)
    }

    pub(crate) fn lower_abi_wrapping(
        &mut self,
        call: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, Option<String>) {
        self.lower_abi_wrapping_with_payload_bridge(call, abi, result_ty, None, target)
    }

    pub(crate) fn lower_abi_wrapping_with_payload_bridge(
        &mut self,
        call: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        payload_bridge: Option<&LayoutBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, Option<String>) {
        let result_ty = &self.facts.peel_alias(result_ty);
        match abi {
            CallableReturnAbi::Tagged
            | CallableReturnAbi::Direct
            | CallableReturnAbi::Tuple { .. } => {
                unreachable!("direct and tuple results do not use a scalar wrapper")
            }
            CallableReturnAbi::BareError => self.lower_bare_error_wrapping(call, result_ty, target),
            CallableReturnAbi::Result { payload } => {
                self.lower_result_wrapping(call, result_ty, *payload, payload_bridge, target)
            }
            CallableReturnAbi::Partial { payload } => {
                self.lower_partial_wrapping(call, result_ty, *payload, payload_bridge, target)
            }
            CallableReturnAbi::Option(OptionReturnAbi::CommaOk { payload }) => {
                self.lower_comma_ok_wrapping(call, result_ty, *payload, payload_bridge, target)
            }
            CallableReturnAbi::Option(OptionReturnAbi::Nullable) => {
                let mut statements = Vec::new();
                let raw_var = self.hoist_tmp_value_statement(&mut statements, "raw", call);
                let (wrap, outcome) = self.lower_nil_check_option_wrap(
                    GoExpression::name(raw_var),
                    result_ty,
                    target,
                );
                statements.extend(wrap);
                (statements, outcome)
            }
            CallableReturnAbi::Option(OptionReturnAbi::Sentinel(value)) => {
                self.lower_sentinel_wrapping(call, result_ty, *value, target)
            }
        }
    }

    pub(crate) fn lower_abi_wrapped_call_to(
        &mut self,
        expression: &Expression,
        abi: &CallableAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> Option<Vec<LoweredStatement>> {
        if matches!(
            abi.result,
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct | CallableReturnAbi::Tuple { .. }
        ) {
            return None;
        }
        let payload_bridge = self.go_return_payload_bridge(abi, result_ty);
        let (mut statements, call) = self
            .lower_call(expression, None, ExpressionContext::value())
            .into_parts();
        let (wrap, _) = self.lower_abi_wrapping_with_payload_bridge(
            call,
            &abi.result,
            result_ty,
            payload_bridge.as_ref(),
            target,
        );
        statements.extend(wrap);
        Some(statements)
    }

    pub(crate) fn go_return_payload_bridge(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<LayoutBridge> {
        let source = abi.return_payload_layout.as_ref()?;
        let target_type = self.facts.peel_alias(result_ty).ok_type();
        let target = self.value_layout(&target_type, SlotOrigin::Lisette);
        let bridge = resolve_layout_bridge(self, source, &target);
        (!bridge.is_identity()).then_some(bridge)
    }

    pub(crate) fn emit_go_call_discarded(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        call_expression: &Expression,
    ) -> Option<GoExpression> {
        let plan = self.plan_call(call_expression)?;
        if plan.resolved.abi.result.is_passthrough() {
            match plan.resolved.origin {
                CallableOrigin::GoInterop
                    if self
                        .go_result_layout_bridge(&plan.resolved.abi, &call_expression.get_type())
                        .is_some() => {}
                _ => return None,
            }
        }

        let (call_setup, call) = self
            .lower_call(call_expression, None, ExpressionContext::value())
            .into_parts();
        setup.extend(call_setup);

        Some(call)
    }

    pub(crate) fn create_temp_vars(&mut self, hint: &str, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                let v = self.fresh_var(Some(hint));
                self.declare(&v);
                v
            })
            .collect()
    }

    pub(crate) fn plan_tuple_from_vars(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        values: Vec<GoExpression>,
    ) -> GoExpression {
        let constructor = build_tuple_literal(values);
        GoExpression::name(self.hoist_tmp_value_statement(statements, "tup", constructor))
    }
}

pub(super) fn build_tuple_literal(values: Vec<GoExpression>) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(
            GeneratedPackage::Prelude,
            format!("MakeTuple{}", values.len()),
        ),
        values,
    )
}

pub(crate) struct LoweredCall<'a> {
    pub(crate) call: &'a Expression,
    pub(crate) wraps: Vec<&'a Expression>,
    pub(crate) shape: CallableReturnAbi,
    pub(crate) origin: CallableOrigin,
    pub(crate) ok_ty: Type,
    pub(crate) nil_guard: Option<NilGuard>,
    pub(crate) payload_bridge: Option<LayoutBridge>,
    pub(crate) layout_bridge: Option<CoercionPlan>,
}

impl LoweredCall<'_> {
    pub(crate) fn is_result(&self) -> bool {
        matches!(
            self.shape,
            CallableReturnAbi::Result { .. } | CallableReturnAbi::BareError
        )
    }

    pub(crate) fn is_bridged(&self) -> bool {
        self.payload_bridge.is_some() || self.layout_bridge.is_some()
    }

    pub(crate) fn has_tuple_payload(&self, planner: &Planner<'_>) -> bool {
        matches!(planner.facts.peel_alias(&self.ok_ty), Type::Tuple(_))
    }
}

impl Planner<'_> {
    pub(crate) fn lowered_call<'a>(&self, subject: &'a Expression) -> Option<LoweredCall<'a>> {
        let (call, wraps) = self.peel_wrap_err(subject);
        let plan = self.plan_call(call)?;
        self.lowered_call_of(call, wraps, &plan)
    }

    pub(crate) fn lowered_call_of<'a>(
        &self,
        call: &'a Expression,
        wraps: Vec<&'a Expression>,
        plan: &CallPlan<'_>,
    ) -> Option<LoweredCall<'a>> {
        let shape = plan.resolved.abi.result.clone();
        if !matches!(
            shape,
            CallableReturnAbi::Result { .. }
                | CallableReturnAbi::BareError
                | CallableReturnAbi::Option(_)
        ) {
            return None;
        }
        let ty = call.get_type();
        let ok_ty = self.facts.peel_alias(&ty).ok_type();
        let nil_guard = match &shape {
            CallableReturnAbi::Result { .. } => self.result_nil_guard(&ok_ty),
            CallableReturnAbi::Option(_) => {
                if self.is_interface_option(&ty) {
                    Some(NilGuard::Interface)
                } else if self.facts.is_nullable_option(&ty) {
                    Some(NilGuard::Pointer)
                } else {
                    None
                }
            }
            _ => None,
        };
        let payload_bridge = self.go_return_payload_bridge(&plan.resolved.abi, &ty);
        let layout_bridge = self.go_result_layout_bridge(&plan.resolved.abi, &ty);
        Some(LoweredCall {
            call,
            wraps,
            shape,
            origin: plan.resolved.origin.clone(),
            ok_ty,
            nil_guard,
            payload_bridge,
            layout_bridge,
        })
    }
}
