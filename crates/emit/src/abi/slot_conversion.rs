use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Expression, Span};
use syntax::program::{Adaptation, CheckedValueUse, File, SlotOrigin, ValueUseKey};
use syntax::types::Type;

use crate::Planner;
use crate::analyze::facts::EmitFacts;
use crate::ir::bodies::Statement;
use crate::ir::values::{GoExpression, ValuePlan};

use super::coercion::{self, CoercionPlan};
use super::layout::ValueLayout;

impl Planner<'_> {
    fn record_applied_conversion(&self, key: ValueUseKey) {
        self.package.record_applied_conversion(key);
    }

    pub(crate) fn cast_operand_coercion(
        &self,
        cast: &Expression,
        source_ty: &Type,
        target_ty: &Type,
    ) -> Option<CoercionPlan> {
        self.record_applied_conversion(ValueUseKey::cast_operand(cast.id()));
        let adaptation = &self.facts.checked(cast).cast_operand().adaptation;
        matches!(adaptation, Adaptation::Interface(_))
            .then(|| CoercionPlan::adapted(self, adaptation, source_ty, target_ty, None))
    }

    pub(crate) fn propagated_error_coercion(
        &self,
        operand: &Expression,
        from: &Type,
        to: &Type,
    ) -> CoercionPlan {
        let Some(recorded) = self.facts.checked(operand).propagated_error() else {
            return CoercionPlan::Identity;
        };
        self.record_applied_conversion(ValueUseKey::propagated_error(operand));
        if !recorded.adaptation.converts() {
            return CoercionPlan::Identity;
        }
        CoercionPlan::adapted(self, &recorded.adaptation, from, to, None)
    }

    pub(crate) fn applied_element_adaptations(&self, value: &Expression) -> Vec<Adaptation> {
        let Adaptation::Tuple(adaptations) = &self.facts.checked(value).value_use().adaptation
        else {
            return Vec::new();
        };
        self.record_applied_conversion(ValueUseKey::value(value));
        adaptations.clone()
    }

    pub(crate) fn convert_to_recorded_slot(
        &mut self,
        value: &Expression,
        plan: ValuePlan,
    ) -> ValuePlan {
        let Some(slot) = self.facts.value_use_slot(value) else {
            return plan;
        };
        let coercion = self.value_slot_coercion(value, slot);
        if coercion.is_identity() {
            return plan;
        }
        plan.map_expression(|setup, emitted| {
            let (coercion_setup, coerced) = coercion.lower(self, emitted);
            setup.extend(coercion_setup);
            coerced
        })
    }

    pub(crate) fn record_bridged_conversion(&self, value: &Expression) {
        self.record_applied_conversion(ValueUseKey::value(value));
    }

    pub(crate) fn record_dropped_code(&self, span: Span) {
        self.package.record_dropped_code(span);
    }

    pub(crate) fn record_discarded_value(&self, value: &Expression) {
        self.record_applied_conversion(ValueUseKey::value(value));
    }

    pub(crate) fn value_slot_coercion(&self, value: &Expression, layout: &Type) -> CoercionPlan {
        self.slot_coercion(value, layout, None)
    }

    pub(crate) fn slot_coercion(
        &self,
        value: &Expression,
        layout: &Type,
        go_layout: Option<&ValueLayout>,
    ) -> CoercionPlan {
        let value_use = self.facts.checked(value).value_use();
        self.adapted_slot_coercion(
            value,
            &value_use.adaptation,
            &value_use.slot,
            layout,
            go_layout,
        )
    }

    pub(crate) fn adapted_slot_coercion(
        &self,
        value: &Expression,
        adaptation: &Adaptation,
        slot: &Type,
        layout: &Type,
        go_layout: Option<&ValueLayout>,
    ) -> CoercionPlan {
        self.record_applied_conversion(ValueUseKey::value(value));
        self.adapted_slot_plan(value, adaptation, slot, layout, go_layout)
    }

    fn adapted_slot_plan(
        &self,
        value: &Expression,
        adaptation: &Adaptation,
        slot: &Type,
        layout: &Type,
        go_layout: Option<&ValueLayout>,
    ) -> CoercionPlan {
        let delegated = matches!(adaptation, Adaptation::Delegated { .. });
        let source_ty = if delegated {
            slot.clone()
        } else {
            value.get_type()
        };
        if go_layout.is_some() {
            if delegated {
                return CoercionPlan::Identity;
            }
            return CoercionPlan::adapted(self, adaptation, &source_ty, slot, go_layout);
        }
        let bridge = if delegated {
            self.function_type_slot_bridge(&source_ty, layout)
        } else {
            self.function_slot_bridge(value, layout)
        };
        if bridge.is_identity() {
            let internal = if delegated {
                CoercionPlan::Identity
            } else {
                CoercionPlan::adapted(self, adaptation, &source_ty, slot, None)
            };
            if !internal.is_identity() {
                return internal;
            }
            let target = self.value_layout(layout, SlotOrigin::Lisette);
            if matches!(
                target.underlying(),
                ValueLayout::TaggedOption { .. }
                    | ValueLayout::Fallible { .. }
                    | ValueLayout::Tuple { .. }
                    | ValueLayout::Reference { .. }
            ) {
                let source = self.value_layout(&source_ty, SlotOrigin::Lisette);
                let bridge = CoercionPlan::bridge(self, &source, &target);
                let reference = matches!(target.underlying(), ValueLayout::Reference { .. });
                if !bridge.is_identity()
                    && (!reference
                        || matches!(
                            bridge,
                            CoercionPlan::Layout(coercion::LayoutBridge::Conversion { .. })
                        ))
                {
                    return bridge;
                }
            }
            internal
        } else {
            bridge
        }
    }

    pub(crate) fn pair_slot_coercions(
        &self,
        pair: &Expression,
        elements: [&Expression; 2],
        layouts: [&Type; 2],
    ) -> [CoercionPlan; 2] {
        let value_use = self.facts.checked(pair).value_use();
        let Adaptation::Tuple(adaptations) = &value_use.adaptation else {
            return [0, 1].map(|index| self.value_slot_coercion(elements[index], layouts[index]));
        };
        self.record_applied_conversion(ValueUseKey::value(pair));
        [0, 1].map(|index| {
            let adaptation = adaptations.get(index).unwrap_or(&Adaptation::Identity);
            self.adapted_slot_coercion(
                elements[index],
                adaptation,
                layouts[index],
                layouts[index],
                None,
            )
        })
    }

    pub(crate) fn coerce_into_slot(
        &mut self,
        statements: &mut Vec<Statement>,
        (value_expression, value): (&Expression, GoExpression),
        slot_ty: &Type,
        go_slot: Option<(&ValueLayout, &ValueLayout)>,
    ) -> GoExpression {
        let layout_bridge = go_slot.map_or(CoercionPlan::Identity, |(source, target)| {
            CoercionPlan::bridge(self, source, target)
        });
        let (bridge_setup, value) = layout_bridge.lower(self, value);
        statements.extend(bridge_setup);
        let coercion =
            self.slot_coercion(value_expression, slot_ty, go_slot.map(|(_, target)| target));
        let (coercion_setup, coerced) = coercion.lower(self, value);
        statements.extend(coercion_setup);
        coerced
    }

    pub(crate) fn apply_type_coercion(
        &mut self,
        statements: &mut Vec<Statement>,
        target_ty: Option<&Type>,
        expression: &Expression,
        emitted: GoExpression,
    ) -> GoExpression {
        let Some(target) = target_ty else {
            return emitted;
        };
        let coercion = self.value_slot_coercion(expression, target);
        let (setup, value) = coercion.lower(self, emitted);
        statements.extend(setup);
        value
    }
}

pub(crate) fn check_applied_conversions(
    facts: &EmitFacts<'_>,
    files: &[&File],
    applied: &HashSet<ValueUseKey>,
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
                .any(|dropped| contains(dropped, &value_use.span))
            || open_bug_unit_into_unknown(facts, value_use)
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
                "{}:{line}: `{written}` of type `{}` is not converted to `{}` ({})",
                file.display_path,
                value_use.value_type,
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
        Adaptation::Receiver(_) => "receiver",
    }
}

fn open_bug_unit_into_unknown(facts: &EmitFacts<'_>, value_use: &CheckedValueUse) -> bool {
    matches!(value_use.adaptation, Adaptation::Interface(_))
        && facts.has_no_go_value(&value_use.value_type)
}

fn needs_application(adaptation: &Adaptation) -> bool {
    adaptation.converts()
        && !matches!(
            adaptation,
            Adaptation::Delegated { .. } | Adaptation::Receiver(_)
        )
}

fn contains(outer: &Span, inner: &Span) -> bool {
    outer.file_id == inner.file_id
        && outer.byte_offset <= inner.byte_offset
        && inner.byte_offset + inner.byte_length <= outer.byte_offset + outer.byte_length
}
