use crate::abi::bridge::GoResultBridge;
use crate::abi::wrapping::WrapperOutcome;
use crate::abi::wrapping::{NilGuard, WrapperTarget};

use crate::Planner;
use crate::abi::callable::{CallableAbi, CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::coercion::{CoercionPlan, LayoutBridge};
use crate::abi::layout::ValueLayout;
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::{Statement, assign, define_many};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{GoExpression, ValuePlan};
use crate::names::go_name::GeneratedPackage;
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::state::expression::ExpressionContext;
use syntax::ast::Expression;
use syntax::program::SlotOrigin;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn plan_go_abi_wrapped_call(
        &mut self,
        call_expression: &Expression,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> ValuePlan {
        let bridge = self.go_result_bridge(abi, result_ty);
        let bridged = bridge.as_ref().is_some_and(|bridge| !bridge.is_identity());
        let call_plan =
            self.plan_call_expression(call_expression, None, ExpressionContext::value());
        let mut wrapped_in_place = false;
        let mut plan = call_plan.map_expression(|setup, call| {
            let shortcut = match abi.result {
                CallableReturnAbi::Lowered(LoweredReturnAbi::Result { payload }) if !bridged => {
                    let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                    self.result_from_pair(call, &layout, payload, None)
                }
                CallableReturnAbi::Lowered(LoweredReturnAbi::Option(
                    OptionReturnAbi::CommaOk { payload },
                )) if !bridged => {
                    let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                    self.option_from_comma_ok(call, &layout, payload, None)
                }
                _ => Err(call),
            };
            let (wrap, value) = match shortcut {
                Ok(value) => (Vec::new(), value),
                Err(call) => {
                    let (wrap, outcome) = self.plan_abi_wrapping(
                        call,
                        &abi.result,
                        result_ty,
                        bridge,
                        WrapperTarget::FreshSlot,
                    );
                    (wrap, outcome.expect("wrapper produced no slot"))
                }
            };
            wrapped_in_place = wrap.is_empty() && !bridged;
            setup.extend(wrap);
            value
        });
        if !wrapped_in_place {
            plan.make_observable();
        }
        plan
    }

    pub(crate) fn go_result_bridge(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<GoResultBridge> {
        let CallableReturnAbi::Lowered(lowered) = &abi.result else {
            return self
                .whole_result_bridge(abi, result_ty)
                .map(GoResultBridge::Whole);
        };
        match lowered {
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(_)) => None,
            LoweredReturnAbi::Tuple { .. } => Some(GoResultBridge::Tuple(
                self.tuple_result_bridges(abi, result_ty),
            )),
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable) => self
                .whole_result_bridge(abi, result_ty)
                .map(GoResultBridge::Whole),
            LoweredReturnAbi::BareError
            | LoweredReturnAbi::Result { .. }
            | LoweredReturnAbi::Partial { .. }
            | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => self
                .payload_result_bridge(
                    &abi.return_layout,
                    &self.value_layout(result_ty, SlotOrigin::Lisette),
                ),
        }
    }

    fn whole_result_bridge(&self, abi: &CallableAbi, result_ty: &Type) -> Option<CoercionPlan> {
        let target = self.value_layout(result_ty, SlotOrigin::Lisette);
        match abi.result {
            CallableReturnAbi::Value => {}
            CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                let source_payload = abi.return_layout.payload()?;
                let target_payload = target.payload()?;
                if source_payload.same_representation(target_payload) {
                    return None;
                }
                let source = ValueLayout::NullableOption {
                    option_type: result_ty.clone(),
                    payload: Box::new(source_payload.clone()),
                };
                let bridge = CoercionPlan::bridge(self, &source, &target);
                return (!bridge.is_identity()).then_some(bridge);
            }
            _ => return None,
        }
        let bridge = CoercionPlan::bridge(self, &abi.return_layout, &target);
        (!bridge.is_identity()).then_some(bridge)
    }

    fn tuple_result_bridges(&self, abi: &CallableAbi, result_ty: &Type) -> Vec<LayoutBridge> {
        let source = abi.function_layout(self).results;
        let target = self.value_layout(result_ty, SlotOrigin::Lisette);
        let target = target
            .tuple_elements()
            .expect("a tuple result ABI has a tuple value layout");
        assert_eq!(
            source.len(),
            target.len(),
            "tuple result slot arity differs"
        );
        source
            .iter()
            .zip(target)
            .map(|(source, target)| self.resolve_layout_bridge(source, target))
            .collect()
    }

    pub(crate) fn plan_abi_wrapping(
        &mut self,
        call: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        bridge: Option<GoResultBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, Option<GoExpression>) {
        let (payload_bridge, error_bridge, bridge) = match bridge {
            Some(GoResultBridge::Payload { value, error }) => (Some(value), Some(error), None),
            bridge => (None, None, bridge),
        };
        let payload_bridge = payload_bridge.as_ref();
        let error_bridge = error_bridge.as_ref();
        let named = |(statements, outcome): (Vec<Statement>, WrapperOutcome)| {
            (statements, outcome.map(GoExpression::identifier))
        };
        let (mut statements, value) = match (abi.lowered(), bridge) {
            (_, Some(GoResultBridge::Whole(bridge))) => bridge.lower(self, call),
            (None, _) => (Vec::new(), call),
            (Some(LoweredReturnAbi::Tuple { arity }), bridge) => {
                let slot_bridges = match bridge {
                    Some(GoResultBridge::Tuple(bridges)) => Some(bridges),
                    _ => None,
                };
                self.plan_tuple_result(call, *arity, result_ty, slot_bridges.as_deref())
            }
            (Some(LoweredReturnAbi::BareError), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_bare_error_wrapping(call, &layout, error_bridge, target));
            }
            (Some(LoweredReturnAbi::Result { payload }), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_result_wrapping(
                    call,
                    &layout,
                    *payload,
                    payload_bridge.zip(error_bridge),
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Partial { payload }), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_partial_wrapping(
                    call,
                    &layout,
                    *payload,
                    payload_bridge.zip(error_bridge),
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { payload })), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_comma_ok_wrapping(
                    call,
                    &layout,
                    *payload,
                    payload_bridge,
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)), _) => {
                let mut statements = Vec::new();
                let raw_var = self.hoist_tmp_value_statement(&mut statements, "raw", call);
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                let (wrap, outcome) = self.lower_nil_check_option_wrap(
                    GoExpression::identifier(raw_var),
                    &layout,
                    target,
                );
                statements.extend(wrap);
                return named((statements, outcome));
            }
            (Some(LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value))), _) => {
                return named(self.lower_sentinel_wrapping(call, result_ty, *value, target));
            }
        };
        let outcome = match target {
            WrapperTarget::FreshSlot => Some(value),
            WrapperTarget::Slot(name) => {
                let slot = GoExpression::identifier(name.clone());
                statements.push(assign(slot.clone(), value));
                Some(slot)
            }
            WrapperTarget::Return => {
                statements.push(plain_return(value));
                None
            }
        };
        (statements, outcome)
    }

    fn plan_tuple_result(
        &mut self,
        call: GoExpression,
        arity: usize,
        result_ty: &Type,
        slot_bridges: Option<&[LayoutBridge]>,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let temps = self.create_temp_vars("ret", arity);
        statements.push(define_many(temps.clone(), call));
        let slot_tys = self.facts.tuple_element_types(result_ty);
        let values: Vec<GoExpression> = temps
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let value = GoExpression::identifier(value);
                if let Some(bridges) = slot_bridges {
                    return self.plan_layout_bridge(&mut statements, value, &bridges[index], true);
                }
                match slot_tys
                    .get(index)
                    .filter(|slot_ty| self.facts.is_nullable_option(slot_ty))
                {
                    Some(slot_ty) => {
                        let layout = self.value_layout(slot_ty, SlotOrigin::Lisette);
                        let (wrapping, outcome) = self.lower_nil_check_option_wrap(
                            value,
                            &layout,
                            WrapperTarget::FreshSlot,
                        );
                        statements.extend(wrapping);
                        GoExpression::identifier(outcome.expect("FreshSlot produces a slot"))
                    }
                    None => value,
                }
            })
            .collect();
        let tuple = self.plan_tuple_from_vars(&mut statements, values);
        (statements, tuple)
    }

    pub(crate) fn try_lower_abi_wrapped_call_to(
        &mut self,
        expression: &Expression,
        abi: &CallableAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> Option<Vec<Statement>> {
        if matches!(
            abi.result.lowered(),
            None | Some(LoweredReturnAbi::Tuple { .. })
        ) {
            return None;
        }
        let bridge = self.go_result_bridge(abi, result_ty);
        let (mut statements, call) = self
            .plan_call_expression(expression, None, ExpressionContext::value())
            .into_parts();
        let (wrap, _) = self.plan_abi_wrapping(call, &abi.result, result_ty, bridge, target);
        statements.extend(wrap);
        Some(statements)
    }

    pub(crate) fn unbridged_tuple_arity(&self, plan: &CallPlan<'_>, ty: &Type) -> Option<usize> {
        let CallableReturnAbi::Lowered(LoweredReturnAbi::Tuple { arity }) =
            plan.resolved.abi.result
        else {
            return None;
        };
        self.go_result_bridge(&plan.resolved.abi, ty)
            .is_none_or(|bridge| bridge.is_identity())
            .then_some(arity)
    }

    pub(crate) fn try_plan_go_call_discarded(
        &mut self,
        setup: &mut Vec<Statement>,
        call_expression: &Expression,
    ) -> Option<GoExpression> {
        let plan = self.plan_call(call_expression)?;
        if !plan.resolved.abi.result.is_lowered() {
            match plan.resolved.origin {
                CallableOrigin::GoInterop
                    if self
                        .go_result_bridge(&plan.resolved.abi, &call_expression.get_type())
                        .is_some() => {}
                _ => return None,
            }
        }

        let (call_setup, call) = self
            .plan_call_expression(call_expression, None, ExpressionContext::value())
            .into_parts();
        setup.extend(call_setup);

        Some(call)
    }

    pub(crate) fn create_temp_vars(
        &mut self,
        hint: &'static str,
        count: usize,
    ) -> Vec<GoIdentifier> {
        (0..count).map(|_| self.fresh_declared(hint)).collect()
    }

    pub(crate) fn plan_tuple_from_vars(
        &mut self,
        statements: &mut Vec<Statement>,
        values: Vec<GoExpression>,
    ) -> GoExpression {
        let constructor = build_tuple_literal(values);
        GoExpression::identifier(self.hoist_tmp_value_statement(statements, "tup", constructor))
    }

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
        let shape = plan.resolved.abi.result.lowered()?.clone();
        if !matches!(
            shape,
            LoweredReturnAbi::Result { .. }
                | LoweredReturnAbi::BareError
                | LoweredReturnAbi::Option(_)
        ) {
            return None;
        }
        let ty = call.get_type();
        let ok_ty = self.facts.ok_payload(&ty);
        let nil_guard = match &shape {
            LoweredReturnAbi::Result { .. }
                if matches!(plan.resolved.origin, CallableOrigin::GoInterop) =>
            {
                self.result_nil_guard(&ok_ty)
            }
            LoweredReturnAbi::Result { .. } => None,
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value)) => {
                Some(NilGuard::Sentinel(*value))
            }
            LoweredReturnAbi::Option(_) => self.facts.nullable_option_nil_guard(&ty),
            _ => None,
        };

        let bridge = self.go_result_bridge(&plan.resolved.abi, &ty);
        Some(LoweredCall {
            call,
            wraps,
            shape,
            from_go: matches!(plan.resolved.origin, CallableOrigin::GoInterop),
            ok_ty,
            nil_guard,
            bridge,
        })
    }
}

pub(crate) fn build_tuple_literal(values: Vec<GoExpression>) -> GoExpression {
    GoExpression::pure_call(
        GoExpression::generated(
            GeneratedPackage::Prelude,
            format!("MakeTuple{}", values.len()),
        ),
        values,
    )
}

#[derive(Debug, Clone)]

pub(crate) struct LoweredCall<'a> {
    pub(crate) call: &'a Expression,
    pub(crate) wraps: Vec<&'a Expression>,
    pub(crate) shape: LoweredReturnAbi,
    pub(crate) from_go: bool,
    pub(crate) ok_ty: Type,
    pub(crate) nil_guard: Option<NilGuard>,
    pub(crate) bridge: Option<GoResultBridge>,
}

impl LoweredCall<'_> {
    pub(crate) fn is_result(&self) -> bool {
        matches!(
            self.shape,
            LoweredReturnAbi::Result { .. } | LoweredReturnAbi::BareError
        )
    }

    pub(crate) fn is_bridged(&self) -> bool {
        self.bridge
            .as_ref()
            .is_some_and(|bridge| !bridge.is_identity())
    }

    pub(crate) fn has_tuple_payload(&self, planner: &Planner<'_>) -> bool {
        planner.facts.has_tuple_payload(&self.call.get_type())
    }
}
