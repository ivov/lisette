use crate::lowering::abi::go_result_wrapping::WrapperOutcome;
use crate::lowering::abi::go_result_wrapping::{NilGuard, WrapperTarget};
use crate::lowering::abi::layout_change::GoResultLayoutChange;

use crate::Planner;
use crate::lowering::abi::callable::CallableAbi;
use crate::lowering::abi::coercion_plan::{CoercionPlan, LayoutChange};
use crate::lowering::abi::layout::{FunctionLayout, ResultProducer, ValueLayout};
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::plan::calls::{CallPlan, CallableOrigin};
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::ValuePlan;
use crate::target::ir::bodies::{Statement, assign, define_many};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use syntax::ast::Expression;
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(in crate::lowering) struct LoweredProducer<'r> {
    pub(in crate::lowering) abi: &'r CallableReturnAbi,
    pub(in crate::lowering) producer: ResultProducer,
}

impl<'r> LoweredProducer<'r> {
    pub(in crate::lowering) fn of_call(abi: &'r CallableAbi, origin: CallableOrigin<'_>) -> Self {
        Self {
            abi: &abi.result,
            producer: origin.producer(),
        }
    }

    pub(in crate::lowering) fn of_layout(layout: &'r FunctionLayout) -> Self {
        Self {
            abi: &layout.return_abi,
            producer: layout.producer,
        }
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_go_abi_wrapped_call(
        &mut self,
        call_expression: &Expression,
        abi: &CallableAbi,
        origin: CallableOrigin<'_>,
        result_ty: &Type,
    ) -> ValuePlan {
        let change = self.go_result_layout_change(abi, result_ty);
        let changed = change.as_ref().is_some_and(|change| !change.is_identity());
        let call_plan =
            self.plan_call_expression(call_expression, None, ExpressionContext::value());
        let mut wrapped_in_place = false;
        let mut plan = call_plan.map_expression(|setup, call| {
            let shortcut = match &abi.result {
                CallableReturnAbi::Lowered(shape @ LoweredReturnAbi::Result { payload })
                    if !changed =>
                {
                    let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                    let nil_guard =
                        self.lowered_payload_nil_guard(shape, origin.producer(), result_ty);
                    self.result_from_pair(call, &layout, *payload, None, nil_guard)
                }
                CallableReturnAbi::Lowered(LoweredReturnAbi::Option(
                    OptionReturnAbi::CommaOk { payload },
                )) if !changed => {
                    let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                    self.option_from_comma_ok(call, &layout, *payload, None)
                }
                _ => Err(call),
            };
            let (wrap, value) = match shortcut {
                Ok(value) => (Vec::new(), value),
                Err(call) => {
                    let (wrap, outcome) = self.plan_abi_wrapping(
                        call,
                        LoweredProducer::of_call(abi, origin),
                        result_ty,
                        change,
                        WrapperTarget::FreshSlot,
                    );
                    (wrap, outcome.expect("wrapper produced no slot"))
                }
            };
            wrapped_in_place = wrap.is_empty() && !changed;
            setup.extend(wrap);
            value
        });
        if !wrapped_in_place {
            plan.make_observable();
        }
        plan
    }

    pub(in crate::lowering) fn go_result_layout_change(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<GoResultLayoutChange> {
        let CallableReturnAbi::Lowered(lowered) = &abi.result else {
            return self
                .whole_result_layout_change(abi, result_ty)
                .map(GoResultLayoutChange::Whole);
        };
        match lowered {
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(_)) => None,
            LoweredReturnAbi::Tuple { .. } => Some(GoResultLayoutChange::Tuple(
                self.tuple_result_layout_changes(abi, result_ty),
            )),
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable) => self
                .whole_result_layout_change(abi, result_ty)
                .map(GoResultLayoutChange::Whole),
            LoweredReturnAbi::BareError
            | LoweredReturnAbi::Result { .. }
            | LoweredReturnAbi::Partial { .. }
            | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => self
                .payload_result_layout_change(
                    &abi.return_layout,
                    &self.value_layout(result_ty, SlotOrigin::Lisette),
                ),
        }
    }

    fn whole_result_layout_change(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<CoercionPlan> {
        let target = self.value_layout(result_ty, SlotOrigin::Lisette);
        match abi.result {
            CallableReturnAbi::Value => {}
            CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                let source_payload = abi.return_layout.payload()?;
                let target_payload = target.payload()?;
                if self
                    .resolve_layout_change(source_payload, target_payload)
                    .is_identity()
                {
                    return None;
                }
                let source = ValueLayout::NullableOption {
                    option_type: result_ty.clone(),
                    payload: Box::new(source_payload.clone()),
                };
                let change = CoercionPlan::change(self, &source, &target);
                return (!change.is_identity()).then_some(change);
            }
            _ => return None,
        }
        let change = CoercionPlan::change(self, &abi.return_layout, &target);
        (!change.is_identity()).then_some(change)
    }

    fn tuple_result_layout_changes(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Vec<LayoutChange> {
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
            .map(|(source, target)| self.resolve_layout_change(source, target))
            .collect()
    }

    pub(in crate::lowering) fn plan_abi_wrapping(
        &mut self,
        call: GoExpression,
        producer: LoweredProducer<'_>,
        result_ty: &Type,
        change: Option<GoResultLayoutChange>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, Option<GoExpression>) {
        let (payload_layout_change, error_layout_change, change) = match change {
            Some(GoResultLayoutChange::Payload { value, error }) => {
                (Some(value), Some(error), None)
            }
            change => (None, None, change),
        };
        let payload_layout_change = payload_layout_change.as_ref();
        let error_layout_change = error_layout_change.as_ref();
        let named = |(statements, outcome): (Vec<Statement>, WrapperOutcome)| {
            (statements, outcome.map(GoExpression::identifier))
        };
        let (mut statements, value) = match (producer.abi.lowered(), change) {
            (_, Some(GoResultLayoutChange::Whole(change))) => change.lower(self, call),
            (None, _) => (Vec::new(), call),
            (Some(LoweredReturnAbi::Tuple { arity }), change) => {
                let slot_layout_changes = match change {
                    Some(GoResultLayoutChange::Tuple(changes)) => Some(changes),
                    _ => None,
                };
                self.plan_tuple_result(call, *arity, result_ty, slot_layout_changes.as_deref())
            }
            (Some(LoweredReturnAbi::BareError), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_bare_error_wrapping(
                    call,
                    &layout,
                    error_layout_change,
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Result { .. }), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_result_wrapping(
                    call,
                    &layout,
                    producer,
                    payload_layout_change.zip(error_layout_change),
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Partial { payload }), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_partial_wrapping(
                    call,
                    &layout,
                    *payload,
                    payload_layout_change.zip(error_layout_change),
                    target,
                ));
            }
            (Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { payload })), _) => {
                let layout = self.value_layout(result_ty, SlotOrigin::Lisette);
                return named(self.lower_comma_ok_wrapping(
                    call,
                    &layout,
                    *payload,
                    payload_layout_change,
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
        slot_layout_changes: Option<&[LayoutChange]>,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let temps = self.create_temp_vars("ret", arity);
        statements.push(define_many(temps.clone(), call));
        let slot_tys = self.facts.tuple_element_types(result_ty);
        let slot_layouts = self.lowered_tuple_slot_layouts(result_ty);
        let values: Vec<GoExpression> = temps
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let value = GoExpression::identifier(value);
                if let Some(changes) = slot_layout_changes {
                    return self.plan_layout_change(&mut statements, value, &changes[index], true);
                }
                match slot_tys.get(index).filter(|_| {
                    matches!(
                        slot_layouts.get(index),
                        Some(ValueLayout::NullableOption { .. })
                    )
                }) {
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

    pub(in crate::lowering) fn try_lower_abi_wrapped_call_to(
        &mut self,
        expression: &Expression,
        abi: &CallableAbi,
        origin: CallableOrigin<'_>,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> Option<Vec<Statement>> {
        if matches!(
            abi.result.lowered(),
            None | Some(LoweredReturnAbi::Tuple { .. })
        ) {
            return None;
        }
        let change = self.go_result_layout_change(abi, result_ty);
        let (mut statements, call) = self
            .plan_call_expression(expression, None, ExpressionContext::value())
            .into_parts();
        let producer = LoweredProducer::of_call(abi, origin);
        let (wrap, _) = self.plan_abi_wrapping(call, producer, result_ty, change, target);
        statements.extend(wrap);
        Some(statements)
    }

    pub(in crate::lowering) fn call_result_keeps_layout(
        &self,
        plan: &CallPlan<'_>,
        ty: &Type,
    ) -> bool {
        self.go_result_layout_change(&plan.resolved.abi, ty)
            .is_none_or(|change| change.is_identity())
    }

    pub(in crate::lowering) fn kept_layout_tuple_arity(
        &self,
        plan: &CallPlan<'_>,
        ty: &Type,
    ) -> Option<usize> {
        let CallableReturnAbi::Lowered(LoweredReturnAbi::Tuple { arity }) =
            plan.resolved.abi.result
        else {
            return None;
        };
        self.call_result_keeps_layout(plan, ty).then_some(arity)
    }

    pub(in crate::lowering) fn try_plan_go_call_discarded(
        &mut self,
        setup: &mut Vec<Statement>,
        call_expression: &Expression,
    ) -> Option<GoExpression> {
        let plan = self.plan_call(call_expression)?;
        if !plan.resolved.abi.result.is_lowered()
            && (!matches!(plan.resolved.origin, CallableOrigin::GoInterop)
                || self.call_result_keeps_layout(&plan, &call_expression.get_type()))
        {
            return None;
        }

        let (call_setup, call) = self
            .plan_call_expression(call_expression, None, ExpressionContext::value())
            .into_parts();
        setup.extend(call_setup);

        Some(call)
    }

    pub(in crate::lowering) fn create_temp_vars(
        &mut self,
        hint: &'static str,
        count: usize,
    ) -> Vec<GoIdentifier> {
        (0..count).map(|_| self.fresh_declared(hint)).collect()
    }

    pub(in crate::lowering) fn plan_tuple_from_vars(
        &mut self,
        statements: &mut Vec<Statement>,
        values: Vec<GoExpression>,
    ) -> GoExpression {
        let constructor = build_tuple_literal(values);
        GoExpression::identifier(self.hoist_tmp_value_statement(statements, "tup", constructor))
    }

    pub(in crate::lowering) fn lowered_call<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<LoweredCall<'a>> {
        let (call, wraps) = self.peel_wrap_err(subject);
        let plan = self.plan_call(call)?;
        self.lowered_call_of(call, wraps, &plan)
    }

    pub(in crate::lowering) fn lowered_call_of<'a>(
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
        let nil_guard =
            self.lowered_payload_nil_guard(&shape, plan.resolved.origin.producer(), &ty);

        let change = self.go_result_layout_change(&plan.resolved.abi, &ty);
        Some(LoweredCall {
            call,
            wraps,
            shape,
            from_go: matches!(plan.resolved.origin, CallableOrigin::GoInterop),
            ok_ty,
            nil_guard,
            change,
        })
    }
}

pub(in crate::lowering) fn build_tuple_literal(values: Vec<GoExpression>) -> GoExpression {
    GoExpression::pure_call(
        GoExpression::generated(
            GeneratedPackage::Prelude,
            go_name::make_tuple_constructor(values.len()),
        ),
        values,
    )
}

#[derive(Debug, Clone)]

pub(in crate::lowering) struct LoweredCall<'a> {
    pub(in crate::lowering) call: &'a Expression,
    pub(in crate::lowering) wraps: Vec<&'a Expression>,
    pub(in crate::lowering) shape: LoweredReturnAbi,
    pub(in crate::lowering) from_go: bool,
    pub(in crate::lowering) ok_ty: Type,
    pub(in crate::lowering) nil_guard: Option<NilGuard>,
    pub(in crate::lowering) change: Option<GoResultLayoutChange>,
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct RawPair {
    pub(in crate::lowering) has_value_slot: bool,
    pub(in crate::lowering) nil_guard: Option<NilGuard>,
    pub(in crate::lowering) payload_layout_change: Option<LayoutChange>,
}

impl LoweredCall<'_> {
    pub(in crate::lowering) fn raw_pair(&self) -> Option<RawPair> {
        if self.shape.has_flattened_payload() {
            return None;
        }
        let has_value_slot = !matches!(self.shape, LoweredReturnAbi::BareError);
        if has_value_slot && self.ok_ty.is_unit() {
            return None;
        }
        let payload_layout_change = match &self.change {
            Some(GoResultLayoutChange::Payload { value, error }) if error.is_identity() => {
                (!value.is_identity()).then(|| value.clone())
            }
            Some(GoResultLayoutChange::Payload { .. }) => return None,
            _ => None,
        };
        Some(RawPair {
            has_value_slot,
            nil_guard: self.nil_guard,
            payload_layout_change,
        })
    }

    pub(in crate::lowering) fn is_result(&self) -> bool {
        matches!(
            self.shape,
            LoweredReturnAbi::Result { .. } | LoweredReturnAbi::BareError
        )
    }

    pub(in crate::lowering) fn changes_layout(&self) -> bool {
        self.change
            .as_ref()
            .is_some_and(|change| !change.is_identity())
    }

    pub(in crate::lowering) fn has_tuple_payload(&self, planner: &Planner<'_>) -> bool {
        planner.facts.has_tuple_payload(&self.call.get_type())
    }
}
