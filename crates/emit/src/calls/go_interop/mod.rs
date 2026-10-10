mod nullable;
mod wrappers;

use wrappers::WrapperOutcome;
pub(crate) use wrappers::{NilGuard, WrapperTarget, is_nil, non_nil, unexpected_nil_error};

use crate::Planner;
use crate::abi::callable::{CallableAbi, CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::coercion::{CoercionPlan, LayoutBridge, resolve_layout_bridge};
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::tuple_element_types;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{Statement, assign, define_many};
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
        let bridge = self.go_result_bridge(abi, result_ty);
        let bridged = bridge.is_some();
        let call_plan = self.lower_call(call_expression, None, ExpressionContext::value());
        let mut wrapped_in_place = false;
        let mut plan = call_plan.map_expression(|setup, call| {
            let shortcut = match abi.result {
                CallableReturnAbi::Lowered(LoweredReturnAbi::Result { payload }) if !bridged => {
                    self.result_from_pair(call, result_ty, payload, None)
                }
                _ => Err(call),
            };
            let (wrap, value) = match shortcut {
                Ok(value) => (Vec::new(), value),
                Err(call) => {
                    let (wrap, outcome) = self.lower_abi_wrapping(
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
            return match abi.result {
                CallableReturnAbi::Direct => self
                    .whole_result_bridge(abi, result_ty)
                    .map(GoResultBridge::Whole),
                _ => None,
            };
        };
        match lowered {
            LoweredReturnAbi::BareError
            | LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(_)) => None,
            LoweredReturnAbi::Tuple { .. } => self
                .tuple_result_bridges(abi, result_ty)
                .map(GoResultBridge::Tuple),
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable) => self
                .whole_result_bridge(abi, result_ty)
                .map(GoResultBridge::Whole),
            LoweredReturnAbi::Result { .. }
            | LoweredReturnAbi::Partial { .. }
            | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => self
                .payload_result_bridge(abi, result_ty)
                .map(GoResultBridge::Payload),
        }
    }

    fn whole_result_bridge(&self, abi: &CallableAbi, result_ty: &Type) -> Option<CoercionPlan> {
        let target = self.value_layout(result_ty, SlotOrigin::Lisette);
        match abi.result {
            CallableReturnAbi::Direct => {}
            CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                let source_payload = abi.return_layout.payload()?;
                let target_payload = target.payload()?;
                if source_payload.same_representation(target_payload) {
                    return None;
                }
            }
            _ => return None,
        }
        let bridge = CoercionPlan::bridge(self, &abi.return_layout, &target);
        (!bridge.is_identity()).then_some(bridge)
    }

    fn tuple_result_bridges(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<Vec<LayoutBridge>> {
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

    /// Convert a callable's Go result into its Lisette value and deliver it to `target`.
    pub(crate) fn lower_abi_wrapping(
        &mut self,
        call: GoExpression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        bridge: Option<GoResultBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, Option<GoExpression>) {
        let result_ty = &self.facts.peel_alias(result_ty);
        let (payload_bridge, bridge) = match bridge {
            Some(GoResultBridge::Payload(bridge)) => (Some(bridge), None),
            bridge => (None, bridge),
        };
        let payload_bridge = payload_bridge.as_ref();
        let named = |(statements, outcome): (Vec<Statement>, WrapperOutcome)| {
            (statements, outcome.map(GoExpression::name))
        };
        let (mut statements, value) = match (abi.lowered(), bridge) {
            (_, Some(GoResultBridge::Whole(bridge))) => bridge.lower(self, call),
            (None, _) => (Vec::new(), call),
            (Some(LoweredReturnAbi::Tuple { arity }), bridge) => {
                let slot_bridges = match bridge {
                    Some(GoResultBridge::Tuple(bridges)) => Some(bridges),
                    _ => None,
                };
                self.lower_tuple_result(call, *arity, result_ty, slot_bridges.as_deref())
            }
            (Some(LoweredReturnAbi::BareError), _) => {
                return named(self.lower_bare_error_wrapping(call, result_ty, target));
            }
            (Some(LoweredReturnAbi::Result { payload }), _) => {
                return named(self.lower_result_wrapping(
                    call,
                    result_ty,
                    *payload,
                    payload_bridge,
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Partial { payload }), _) => {
                return named(self.lower_partial_wrapping(
                    call,
                    result_ty,
                    *payload,
                    payload_bridge,
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { payload })), _) => {
                return named(self.lower_comma_ok_wrapping(
                    call,
                    result_ty,
                    *payload,
                    payload_bridge,
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)), _) => {
                let mut statements = Vec::new();
                let raw_var = self.hoist_tmp_value_statement(&mut statements, "raw", call);
                let (wrap, outcome) = self.lower_nil_check_option_wrap(
                    GoExpression::name(raw_var),
                    result_ty,
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
                let slot = GoExpression::name(name.to_string());
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

    fn lower_tuple_result(
        &mut self,
        call: GoExpression,
        arity: usize,
        result_ty: &Type,
        slot_bridges: Option<&[LayoutBridge]>,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let temps = self.create_temp_vars("ret", arity);
        statements.push(define_many(temps.clone(), call));
        let slot_tys = tuple_element_types(result_ty);
        let values: Vec<GoExpression> = temps
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let value = GoExpression::name(value);
                if let Some(bridges) = slot_bridges {
                    return self.plan_layout_bridge(&mut statements, value, &bridges[index]);
                }
                match slot_tys
                    .get(index)
                    .filter(|slot_ty| self.facts.is_nullable_option(slot_ty))
                {
                    Some(slot_ty) => {
                        self.plan_nil_check_option_wrap(&mut statements, value, slot_ty)
                    }
                    None => value,
                }
            })
            .collect();
        let tuple = self.plan_tuple_from_vars(&mut statements, values);
        (statements, tuple)
    }

    pub(crate) fn lower_abi_wrapped_call_to(
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
            .lower_call(expression, None, ExpressionContext::value())
            .into_parts();
        let (wrap, _) = self.lower_abi_wrapping(call, &abi.result, result_ty, bridge, target);
        statements.extend(wrap);
        Some(statements)
    }

    fn payload_result_bridge(&self, abi: &CallableAbi, result_ty: &Type) -> Option<LayoutBridge> {
        let source = abi.return_layout.payload()?;
        let target_type = self.facts.peel_alias(result_ty).ok_type();
        let target = self.value_layout(&target_type, SlotOrigin::Lisette);
        let bridge = resolve_layout_bridge(self, source, &target);
        (!bridge.is_identity()).then_some(bridge)
    }

    pub(crate) fn emit_go_call_discarded(
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
        statements: &mut Vec<Statement>,
        values: Vec<GoExpression>,
    ) -> GoExpression {
        let constructor = build_tuple_literal(values);
        GoExpression::name(self.hoist_tmp_value_statement(statements, "tup", constructor))
    }
}

pub(crate) fn build_tuple_literal(values: Vec<GoExpression>) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(
            GeneratedPackage::Prelude,
            format!("MakeTuple{}", values.len()),
        ),
        values,
    )
}

pub(crate) enum GoResultBridge {
    Tuple(Vec<LayoutBridge>),
    Whole(CoercionPlan),
    Payload(LayoutBridge),
}

impl GoResultBridge {
    pub(crate) fn into_payload(self) -> Option<LayoutBridge> {
        match self {
            Self::Payload(bridge) => Some(bridge),
            Self::Tuple(_) | Self::Whole(_) => None,
        }
    }
}

pub(crate) struct LoweredCall<'a> {
    pub(crate) call: &'a Expression,
    pub(crate) wraps: Vec<&'a Expression>,
    pub(crate) shape: LoweredReturnAbi,
    pub(crate) origin: CallableOrigin,
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
        self.bridge.is_some()
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
        let ok_ty = self.facts.peel_alias(&ty).ok_type();
        let nil_guard = match &shape {
            LoweredReturnAbi::Result { .. } => self.result_nil_guard(&ok_ty),
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value)) => {
                Some(NilGuard::Sentinel(*value))
            }
            LoweredReturnAbi::Option(_) => self.nullable_option_nil_guard(&ty),
            _ => None,
        };
        let bridge = self.go_result_bridge(&plan.resolved.abi, &ty);
        Some(LoweredCall {
            call,
            wraps,
            shape,
            origin: plan.resolved.origin,
            ok_ty,
            nil_guard,
            bridge,
        })
    }
}
