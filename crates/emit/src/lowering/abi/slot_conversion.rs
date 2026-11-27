use rustc_hash::FxHashSet;
use syntax::ast::{Expression, Span};
use syntax::program::{Adaptation, File, SlotOrigin, ValueUseKey};
use syntax::types::Type;

use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::ir::bodies::Statement;
use crate::target::ir::values::GoExpression;

use super::coercion_plan::{CoercionPlan, LayoutChange};
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lowering::calls::call_plan::FunctionLayoutChange;
use crate::lowering::expressions::tuple_literal::TupleLiteral;
use crate::lowering::value_plans::context::ExpressionContext;

#[derive(Clone, Copy)]
pub(in crate::lowering) enum SlotTarget<'t> {
    Recorded,
    Layout(&'t Type),
    Go {
        target: &'t ValueLayout,
    },
    GoAfter {
        change: &'t LayoutChange,
        target: &'t ValueLayout,
    },
    GoLayout {
        source: &'t ValueLayout,
        target: &'t ValueLayout,
    },
    TaggedCallback(&'t FunctionLayoutChange),
    Element {
        adaptation: &'t Adaptation,
        semantic: &'t Type,
        slot: &'t Type,
    },
    Unconverted,
}

impl<'t> SlotTarget<'t> {
    pub(in crate::lowering) fn go_or_layout(go: Option<&'t ValueLayout>, ty: &'t Type) -> Self {
        match go {
            Some(target) => Self::Go { target },
            None => Self::Layout(ty),
        }
    }
}

#[derive(Default)]
pub(in crate::lowering) struct TupleElementPlacement<'a> {
    adaptations: &'a [Adaptation],
    semantic: Vec<Type>,
}

impl TupleElementPlacement<'_> {
    pub(in crate::lowering) fn target<'t>(
        &'t self,
        index: usize,
        slot: Option<&'t Type>,
    ) -> SlotTarget<'t> {
        let Some(slot) = slot else {
            return SlotTarget::Recorded;
        };
        match self.adaptations.get(index).zip(self.semantic.get(index)) {
            Some((adaptation, semantic)) => SlotTarget::Element {
                adaptation,
                semantic,
                slot,
            },
            None => SlotTarget::Layout(slot),
        }
    }
}

#[must_use]
pub(in crate::lowering) struct SlotConversion {
    plan: CoercionPlan,
    applied: Option<ValueUseKey>,
}

impl SlotConversion {
    pub(in crate::lowering) fn identity() -> Self {
        Self {
            plan: CoercionPlan::Identity,
            applied: None,
        }
    }

    pub(in crate::lowering) fn is_identity(&self) -> bool {
        self.plan.is_identity()
    }

    pub(in crate::lowering) fn apply_identity(self, planner: &Planner<'_>) {
        debug_assert!(self.is_identity());
        if let Some(key) = self.applied {
            planner.conversions.record_applied_conversion(key);
        }
    }

    pub(in crate::lowering) fn apply(
        self,
        planner: &mut Planner<'_>,
        setup: &mut Vec<Statement>,
        value: GoExpression,
    ) -> GoExpression {
        if let Some(key) = self.applied {
            planner.conversions.record_applied_conversion(key);
        }
        let (conversion_setup, converted) = self.plan.lower(planner, value);
        setup.extend(conversion_setup);
        converted
    }
}

impl<'a> Planner<'a> {
    pub(in crate::lowering) fn slot_conversion(
        &self,
        expression: &Expression,
        context: ExpressionContext<'_>,
        target: SlotTarget<'_>,
    ) -> SlotConversion {
        let (plan, applies) = match target {
            SlotTarget::Unconverted => (CoercionPlan::Identity, false),
            SlotTarget::Recorded => match self.facts.value_use_slot(expression) {
                Some(slot) if self.fills_own_type(expression, slot) => {
                    (CoercionPlan::Identity, true)
                }
                Some(slot) => (self.value_slot_coercion(expression, slot), true),
                None => (CoercionPlan::Identity, false),
            },
            SlotTarget::Layout(layout) => (self.value_slot_coercion(expression, layout), true),
            SlotTarget::Go { target } => {
                let source = self.queries().emitted_value_layout(expression, context);
                (self.go_slot_coercion(expression, &source, target), true)
            }
            SlotTarget::GoLayout { source, target } => {
                let change = self.value_layout_change(expression, source, target);
                let applies = !change.is_identity();
                (change, applies)
            }
            SlotTarget::TaggedCallback(change) => {
                let change = CoercionPlan::Layout(LayoutChange::Function {
                    source: Box::new(change.source.clone()),
                    target: Box::new(change.target.clone()),
                });
                (change, true)
            }
            SlotTarget::GoAfter { change, target } => {
                let change = CoercionPlan::layout(change.clone());
                (
                    change.then(self.go_slot_adaptation(expression, target)),
                    true,
                )
            }
            SlotTarget::Element {
                adaptation,
                semantic,
                slot,
            } => (
                self.adapted_slot_coercion(expression, adaptation, semantic, slot),
                true,
            ),
        };
        SlotConversion {
            plan,
            applied: applies.then(|| ValueUseKey::value(expression)),
        }
    }

    fn go_slot_coercion(
        &self,
        expression: &Expression,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> CoercionPlan {
        let adaptation_reaches_go_layout = matches!(
            self.facts.checked(expression).value_use().adaptation,
            Adaptation::DivergingFunction
        );
        let layout_change = if adaptation_reaches_go_layout {
            CoercionPlan::Identity
        } else {
            self.value_layout_change(expression, source, target)
        };
        layout_change.then(self.go_slot_adaptation(expression, target))
    }

    pub(in crate::lowering) fn cast_operand_coercion(
        &self,
        cast: &Expression,
        source_ty: &Type,
        target_ty: &Type,
    ) -> Option<CoercionPlan> {
        self.conversions
            .record_applied_conversion(ValueUseKey::cast_operand(cast.id()));
        let adaptation = &self.facts.checked(cast).cast_operand().adaptation;
        matches!(adaptation, Adaptation::Interface(_))
            .then(|| CoercionPlan::adapted(&self.queries(), adaptation, source_ty, target_ty, None))
    }

    pub(in crate::lowering) fn propagated_error_coercion(
        &self,
        operand: &Expression,
        from: &Type,
        to: &Type,
    ) -> CoercionPlan {
        let Some(recorded) = self.facts.checked(operand).propagated_error() else {
            return CoercionPlan::Identity;
        };
        self.conversions
            .record_applied_conversion(ValueUseKey::propagated_error(operand));
        if !recorded.adaptation.converts() {
            return CoercionPlan::Identity;
        }
        CoercionPlan::adapted(&self.queries(), &recorded.adaptation, from, to, None)
    }

    pub(in crate::lowering) fn applied_element_adaptations(
        &self,
        value: &Expression,
    ) -> &'a [Adaptation] {
        let Adaptation::Tuple(adaptations) = &self.facts.checked(value).value_use().adaptation
        else {
            return &[];
        };
        self.record_converted_value(value);
        adaptations
    }

    pub(in crate::lowering) fn tuple_element_placement(
        &self,
        literal: TupleLiteral<'_>,
        slot_types: &[Type],
    ) -> TupleElementPlacement<'a> {
        let TupleLiteral {
            expression: tuple,
            ty,
            ..
        } = literal;
        if self
            .facts
            .same_types(&self.facts.tuple_or_type_arguments(ty), slot_types)
        {
            return TupleElementPlacement::default();
        }
        let adaptations = self.applied_element_adaptations(tuple);
        if adaptations.is_empty() {
            return TupleElementPlacement::default();
        }
        let slot = &self.facts.checked(tuple).value_use().slot;
        TupleElementPlacement {
            adaptations,
            semantic: self.facts.tuple_or_type_arguments(slot),
        }
    }

    pub(in crate::lowering) fn apply_conversions(
        &mut self,
        setup: &mut Vec<Statement>,
        values: Vec<GoExpression>,
        conversions: Vec<SlotConversion>,
    ) -> Vec<GoExpression> {
        values
            .into_iter()
            .zip(conversions)
            .map(|(value, conversion)| conversion.apply(self, setup, value))
            .collect()
    }

    pub(in crate::lowering) fn record_converted_value(&self, value: &Expression) {
        self.conversions
            .record_applied_conversion(ValueUseKey::value(value));
    }

    fn fills_own_type(&self, value: &Expression, slot: &Type) -> bool {
        matches!(
            self.facts.checked(value).value_use().adaptation,
            Adaptation::Identity
        ) && value.type_ref() == Some(slot)
            && self.facts.resolve_to_function_type(slot).is_none()
    }

    fn value_slot_coercion(&self, value: &Expression, layout: &Type) -> CoercionPlan {
        let value_use = self.facts.checked(value).value_use();
        self.adapted_slot_coercion(value, &value_use.adaptation, &value_use.slot, layout)
    }

    fn go_slot_adaptation(&self, value: &Expression, go_layout: &ValueLayout) -> CoercionPlan {
        let value_use = self.facts.checked(value).value_use();
        match &value_use.adaptation {
            Adaptation::Delegated { .. } => CoercionPlan::Identity,
            adaptation @ Adaptation::Identity
            | adaptation @ Adaptation::Interface(_)
            | adaptation @ Adaptation::ImplicitSome { .. }
            | adaptation @ Adaptation::Newtype
            | adaptation @ Adaptation::Tuple(_)
            | adaptation @ Adaptation::Array(_)
            | adaptation @ Adaptation::DivergingFunction
            | adaptation @ Adaptation::Receiver(_) => CoercionPlan::adapted(
                &self.queries(),
                adaptation,
                &value.get_type(),
                &value_use.slot,
                Some(go_layout),
            ),
        }
    }

    fn adapted_slot_coercion(
        &self,
        value: &Expression,
        adaptation: &Adaptation,
        slot: &Type,
        layout: &Type,
    ) -> CoercionPlan {
        let delegated = matches!(adaptation, Adaptation::Delegated { .. });
        let source_ty = if delegated {
            slot.clone()
        } else {
            value.get_type()
        };
        let change = if delegated {
            self.queries()
                .function_type_slot_layout_change(&source_ty, layout)
        } else {
            self.queries().function_slot_layout_change(value, layout)
        };
        if change.changes_layout() {
            return change;
        }
        if !delegated {
            let internal =
                CoercionPlan::adapted(&self.queries(), adaptation, &source_ty, slot, None);
            if internal.changes_layout() {
                return internal;
            }
        }
        let target = self.facts.value_layout(layout, SlotOrigin::Lisette);
        if matches!(
            target.underlying(),
            ValueLayout::TaggedOption { .. }
                | ValueLayout::Fallible { .. }
                | ValueLayout::Tuple { .. }
                | ValueLayout::Reference { .. }
        ) {
            let source = self.facts.value_layout(&source_ty, SlotOrigin::Lisette);
            let change = CoercionPlan::change(&self.queries(), &source, &target);
            let reference = matches!(target.underlying(), ValueLayout::Reference { .. });
            if change.changes_layout()
                && (!reference
                    || matches!(
                        change,
                        CoercionPlan::Layout(LayoutChange::Conversion { .. })
                    ))
            {
                return change;
            }
        }
        CoercionPlan::Identity
    }

    fn value_layout_change(
        &self,
        value: &Expression,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> CoercionPlan {
        match self.facts.checked(value).value_use().adaptation {
            Adaptation::ImplicitSome { .. } => CoercionPlan::Identity,
            Adaptation::Identity
            | Adaptation::Delegated { .. }
            | Adaptation::Interface(_)
            | Adaptation::Newtype
            | Adaptation::Tuple(_)
            | Adaptation::Array(_)
            | Adaptation::DivergingFunction
            | Adaptation::Receiver(_) => CoercionPlan::change(&self.queries(), source, target),
        }
    }
}

pub(in crate::lowering) fn check_applied_conversions(
    facts: &EmitFacts<'_>,
    files: &[&File],
    applied: &FxHashSet<ValueUseKey>,
    dropped_code: &[Span],
) {
    if !cfg!(debug_assertions) {
        return;
    }
    let mut failures: Vec<(u32, u32, String)> = Vec::new();
    for (key, value_use) in facts.value_uses() {
        let Some(file) = files.iter().find(|file| file.id == value_use.span.file_id) else {
            continue;
        };
        if !needs_application(&value_use.adaptation)
            || applied.contains(key)
            || dropped_code
                .iter()
                .any(|dropped| dropped.contains(&value_use.span))
        {
            continue;
        }
        let start = value_use.span.byte_offset as usize;
        let end = start + value_use.span.byte_length as usize;
        let line = file.source[..start.min(file.source.len())]
            .matches('\n')
            .count()
            + 1;
        let written = file.source.get(start..end).unwrap_or_default();
        failures.push((
            value_use.span.file_id,
            value_use.span.byte_offset,
            format!(
                "{}:{line}: `{written}` is not converted to `{}` ({})",
                file.display_path,
                value_use.slot,
                adaptation_kind(&value_use.adaptation)
            ),
        ));
    }
    if failures.is_empty() {
        return;
    }
    failures.sort();
    let report: Vec<String> = failures.into_iter().map(|(_, _, line)| line).collect();
    panic!(
        "internal error: emit did not apply a conversion that the checker recorded\n{}",
        report.join("\n")
    );
}

fn adaptation_kind(adaptation: &Adaptation) -> &'static str {
    match adaptation {
        Adaptation::Identity => "identity",
        Adaptation::Delegated { .. } => "delegated",
        Adaptation::Interface(_) => "interface",
        Adaptation::ImplicitSome { .. } => "implicit Some",
        Adaptation::Newtype => "newtype",
        Adaptation::Tuple(_) => "tuple",
        Adaptation::Array(_) => "array",
        Adaptation::DivergingFunction => "diverging function",
        Adaptation::Receiver(_) => "receiver",
    }
}

fn needs_application(adaptation: &Adaptation) -> bool {
    adaptation.converts()
        && !matches!(
            adaptation,
            Adaptation::Delegated { .. } | Adaptation::Receiver(_)
        )
}
