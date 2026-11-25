use crate::lowering::planner::queries::PlannerQueries;
use crate::target::names::go_name;
use syntax::program::{Adaptation, SlotOrigin};
use syntax::types::Type;

use crate::Planner;
use crate::lowering::abi::layout_change::OptionSlot;
use crate::lowering::definitions::interface_adapter::AdapterPlan;
use crate::target::ir::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, Statement, assign, declare,
};
use crate::target::ir::go_expression::CompositeLayout;
use crate::target::ir::prelude::{option_variant, prelude_call};
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;

use crate::lookup::facts::value_layout::{FunctionLayout, ValueLayout};

#[derive(Debug, Clone)]
pub(in crate::lowering) enum CoercionPlan {
    Identity,
    WrapAsInterface(AdapterPlan),
    WrapNewtype {
        ty: Type,
    },
    WrapSome {
        payload_type: Type,
        payload: Box<CoercionPlan>,
    },
    Layout(LayoutChange),
    RebuildArray {
        array_type: Type,
        element: Box<CoercionPlan>,
    },
    RebuildTuple {
        slot_types: Vec<Type>,
        elements: Vec<CoercionPlan>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum LayoutChangeDirection {
    ToGo,
    FromGo,
}

#[derive(Debug, Clone)]
pub(in crate::lowering) enum LayoutChange {
    Identity,
    Conversion {
        target: Box<ValueLayout>,
    },
    UnwrapNullableOption {
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutChange>,
    },
    UnwrapPointerOption {
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutChange>,
    },
    WrapNullableOption {
        target: Box<ValueLayout>,
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutChange>,
    },
    WrapPointerOption {
        target: Box<ValueLayout>,
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutChange>,
    },
    Reference {
        pointee: Box<LayoutChange>,
    },
    Function {
        source: Box<FunctionLayout>,
        target: Box<FunctionLayout>,
    },
    NilablePayload {
        target: Box<ValueLayout>,
        slot: OptionSlot,
        payload: Box<LayoutChange>,
    },
    Immutable {
        target: Box<ValueLayout>,
        container: ImmutableContainer,
        elements: Vec<LayoutChange>,
    },
    Aggregate(AggregateLayoutChange),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum ImmutableContainer {
    Tuple,
    Option,
    Result,
    Partial,
}

impl ImmutableContainer {
    fn of_fallible(fallible_type: &Type) -> Self {
        if fallible_type.is_partial() {
            Self::Partial
        } else {
            Self::Result
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct AggregateLayoutChange {
    pub(in crate::lowering) source: Box<ValueLayout>,
    pub(in crate::lowering) target: Box<ValueLayout>,
    pub(in crate::lowering) key: Option<Box<LayoutChange>>,
    pub(in crate::lowering) element: Box<LayoutChange>,
}

impl LayoutChange {
    pub(in crate::lowering) fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }

    pub(in crate::lowering) fn changes_layout(&self) -> bool {
        !self.is_identity()
    }

    pub(in crate::lowering) fn direction(&self) -> Option<LayoutChangeDirection> {
        match self {
            Self::UnwrapNullableOption { .. } | Self::UnwrapPointerOption { .. } => {
                Some(LayoutChangeDirection::ToGo)
            }
            Self::WrapNullableOption { .. } | Self::WrapPointerOption { .. } => {
                Some(LayoutChangeDirection::FromGo)
            }
            Self::Reference { pointee } => pointee.direction(),
            Self::NilablePayload { payload, .. } => payload.direction(),
            Self::Identity | Self::Conversion { .. } | Self::Function { .. } => None,
            Self::Immutable { elements, .. } => elements.iter().find_map(LayoutChange::direction),
            Self::Aggregate(AggregateLayoutChange { key, element, .. }) => key
                .as_deref()
                .and_then(LayoutChange::direction)
                .or_else(|| element.direction()),
        }
    }
}

impl CoercionPlan {
    pub(in crate::lowering) fn adapted(
        planner: &PlannerQueries<'_, '_>,
        adaptation: &Adaptation,
        from: &Type,
        to: &Type,
        slot: Option<&ValueLayout>,
    ) -> Self {
        let facts = &planner.facts;
        match adaptation {
            Adaptation::Interface(conversion) => planner
                .interface_adapter(conversion)
                .map_or_else(|| Self::internal(planner, from, to), Self::WrapAsInterface),
            Adaptation::Newtype => Self::WrapNewtype { ty: to.clone() },
            Adaptation::ImplicitSome { payload } => {
                let payload_type = facts.ok_payload(to);
                let payload = Self::adapted(planner, payload, from, &payload_type, None);
                let layout = slot
                    .cloned()
                    .unwrap_or_else(|| planner.facts.value_layout(to, SlotOrigin::Lisette));
                if matches!(layout.underlying(), ValueLayout::TaggedOption { .. }) {
                    Self::WrapSome {
                        payload_type,
                        payload: Box::new(payload),
                    }
                } else {
                    payload
                }
            }
            Adaptation::Tuple(elements) => Self::adapted_tuple(planner, elements, from, to),
            Adaptation::Array(element) => match (facts.array_parts(from), facts.array_parts(to)) {
                (Some((_, source)), Some((_, slot))) => Self::RebuildArray {
                    array_type: to.clone(),
                    element: Box::new(Self::adapted_element(planner, element, &source, &slot)),
                },
                _ => panic!("internal error: an array adaptation does not fit `{from}` to `{to}`"),
            },
            Adaptation::DivergingFunction => {
                Self::adapted_diverging_function(planner, from, to, slot)
            }
            _ => Self::internal(planner, from, to),
        }
    }

    fn adapted_tuple(
        planner: &PlannerQueries<'_, '_>,
        elements: &[Adaptation],
        from: &Type,
        to: &Type,
    ) -> Self {
        let facts = &planner.facts;
        match (facts.tuple_elements(from), facts.tuple_elements(to)) {
            (Some(sources), Some(slots)) => Self::RebuildTuple {
                elements: elements
                    .iter()
                    .zip(sources.iter().zip(&slots))
                    .map(|(element, (from, to))| Self::adapted_element(planner, element, from, to))
                    .collect(),
                slot_types: slots,
            },
            _ => panic!("internal error: a tuple adaptation does not fit `{from}` to `{to}`"),
        }
    }

    fn adapted_diverging_function(
        planner: &PlannerQueries<'_, '_>,
        from: &Type,
        to: &Type,
        slot: Option<&ValueLayout>,
    ) -> Self {
        let source = planner.facts.value_layout(from, SlotOrigin::Lisette);
        let target = slot
            .cloned()
            .unwrap_or_else(|| planner.facts.value_layout(to, SlotOrigin::Lisette));
        match (source.underlying(), target.underlying()) {
            (
                ValueLayout::Function { layout: source, .. },
                ValueLayout::Function { layout: target, .. },
            ) => Self::Layout(LayoutChange::Function {
                source: Box::new(source.clone()),
                target: Box::new(target.clone()),
            }),
            _ => panic!(
                "internal error: a diverging function adaptation does not fit `{from}` to `{to}`"
            ),
        }
    }

    fn adapted_element(
        planner: &PlannerQueries<'_, '_>,
        adaptation: &Adaptation,
        from: &Type,
        to: &Type,
    ) -> Self {
        match adaptation {
            Adaptation::Identity => planner
                .widening_element_plan(from, to)
                .unwrap_or(Self::Identity),
            _ => Self::adapted(planner, adaptation, from, to, None),
        }
    }

    pub(in crate::lowering) fn internal(
        planner: &PlannerQueries<'_, '_>,
        from: &Type,
        to: &Type,
    ) -> Self {
        if from == to {
            Self::Identity
        } else if planner.facts.is_interface_or_unknown(to)
            && let Some((stored, lisette)) = planner.any_slot_function_layouts(from)
        {
            Self::change(planner, &lisette, &stored)
        } else if let Some(rebuild) = planner.aggregate_rebuild(from, to) {
            rebuild
        } else {
            Self::Identity
        }
    }

    pub(in crate::lowering) fn change(
        planner: &PlannerQueries<'_, '_>,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> Self {
        let change = planner.resolve_layout_change(source, target);
        if change.is_identity() {
            Self::Identity
        } else {
            Self::Layout(change)
        }
    }

    pub(in crate::lowering) fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }

    pub(in crate::lowering) fn changes_layout(&self) -> bool {
        !self.is_identity()
    }

    pub(in crate::lowering) fn lower(
        self,
        planner: &mut Planner<'_>,
        value: GoExpression,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let value = match self {
            Self::Identity => value,
            Self::WrapAsInterface(plan) => {
                let adapter_type = planner.ensure_adapter_type(plan);
                GoExpression::struct_literal(
                    adapter_type,
                    vec![("inner".to_string(), value)],
                    CompositeLayout::Inline { padded: false },
                )
            }
            Self::WrapNewtype { ty } => {
                let type_name = planner.queries().go_type(&ty);
                GoExpression::conversion(type_name, value)
            }
            Self::WrapSome {
                payload_type,
                payload,
            } => {
                let (payload_setup, payload) = payload.lower(planner, value);
                statements.extend(payload_setup);
                let type_arguments =
                    GoType::type_argument(&planner.queries().component_go_type(&payload_type));
                prelude_call(
                    option_variant("Some").constructor,
                    type_arguments,
                    vec![payload],
                )
            }
            Self::Layout(change) => planner.plan_layout_change(&mut statements, value, &change),
            Self::RebuildArray {
                array_type,
                element,
            } => planner.plan_array_rebuild(&mut statements, value, &array_type, *element),
            Self::RebuildTuple {
                slot_types,
                elements,
            } => planner.plan_tuple_rebuild(&mut statements, value, &slot_types, elements),
        };

        (statements, value)
    }
}

impl Planner<'_> {
    fn plan_array_rebuild(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        array_type: &Type,
        element: CoercionPlan,
    ) -> GoExpression {
        let source = self.stable_source(statements, "src", value);
        let go_type = self.queries().go_type(array_type);
        let output = self.fresh_declared("boxed");
        statements.push(declare(output.clone(), go_type));

        let index = self.fresh_declared("i");
        let element_read =
            GoExpression::index(source.clone(), GoExpression::identifier(index.clone()));
        let (mut body, coerced) = element.lower(self, element_read);
        body.push(assign(
            GoExpression::index(
                GoExpression::identifier(output.clone()),
                GoExpression::identifier(index.clone()),
            ),
            coerced,
        ));
        statements.push(
            LoweredStatement::Loop(LoopPlan {
                kind: LoopKind::Generated { label: None },
                header: LoopHeader::Range {
                    key: Some(index),
                    value: None,
                    iterable: source,
                },
                body: LoweredBlock { statements: body },
            })
            .into(),
        );
        GoExpression::identifier(output)
    }

    fn plan_tuple_rebuild(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        slot_types: &[Type],
        elements: Vec<CoercionPlan>,
    ) -> GoExpression {
        let source = self.stable_source(statements, "tup", value);

        let mut arguments = Vec::with_capacity(elements.len());
        for (index, element) in elements.into_iter().enumerate() {
            let slot_read =
                GoExpression::selector(source.clone(), go_name::tuple_field(index).to_string());
            let (element_setup, coerced) = element.lower(self, slot_read);
            statements.extend(element_setup);
            arguments.push(coerced);
        }
        GoExpression::pure_call(
            self.queries()
                .make_tuple_callee(slot_types, slot_types.len()),
            arguments,
        )
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn resolve_layout_change(
        &self,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> LayoutChange {
        if source.same_representation(target) {
            return LayoutChange::Identity;
        }

        use ValueLayout::{Function, Named, NullableOption, PointerOption, TaggedOption};

        if let (
            Function {
                layout: source_layout,
                ..
            },
            Function {
                layout: target_layout,
                ..
            },
        ) = (source.underlying(), target.underlying())
            && source_layout.same_representation(target_layout)
        {
            if source.function_name().is_none() || target.function_name().is_none() {
                return LayoutChange::Identity;
            }
            return LayoutChange::Conversion {
                target: Box::new(target.clone()),
            };
        }

        match (source, target) {
            (
                TaggedOption { .. } | NullableOption { .. } | PointerOption { .. },
                TaggedOption { .. } | NullableOption { .. } | PointerOption { .. },
            ) => self.option_layout_change(source, target),
            (
                Named {
                    underlying: source_underlying,
                    ..
                },
                target,
            ) => self.resolve_layout_change(source_underlying, target),
            (
                source,
                Named {
                    underlying: target_underlying,
                    ..
                },
            ) => self.resolve_layout_change(source, target_underlying),
            _ => self.container_layout_change(source, target),
        }
    }

    fn container_layout_change(&self, source: &ValueLayout, target: &ValueLayout) -> LayoutChange {
        use ValueLayout::{Array, Fallible, Function, Map, Reference, Slice, Tuple};
        match (source, target) {
            (
                Fallible {
                    payload: source_payload,
                    error: source_error,
                    ..
                },
                Fallible {
                    fallible_type,
                    payload: target_payload,
                    error: target_error,
                },
            ) => self.immutable_layout_change(
                target,
                ImmutableContainer::of_fallible(fallible_type),
                [source_payload, source_error]
                    .into_iter()
                    .zip([target_payload, target_error])
                    .map(|(source, target)| (source.as_ref(), target.as_ref())),
            ),
            (
                Tuple {
                    elements: source_elements,
                    ..
                },
                Tuple {
                    elements: target_elements,
                    ..
                },
            ) => self.tuple_layout_change(target, source_elements, target_elements),
            (Function { layout: source, .. }, Function { layout: target, .. }) => {
                LayoutChange::Function {
                    source: Box::new(source.clone()),
                    target: Box::new(target.clone()),
                }
            }
            (
                Reference {
                    pointee: source_pointee,
                    ..
                },
                Reference {
                    pointee: target_pointee,
                    ..
                },
            ) => self.reference_layout_change(source_pointee, target_pointee, target),
            (
                Slice {
                    element: source_element,
                    ..
                },
                Slice {
                    element: target_element,
                    ..
                },
            )
            | (
                Array {
                    element: source_element,
                    ..
                },
                Array {
                    element: target_element,
                    ..
                },
            ) => self.aggregate_layout_change(
                source,
                target,
                None,
                (source_element.as_ref(), target_element.as_ref()),
            ),
            (
                Map {
                    key: source_key,
                    value: source_value,
                    ..
                },
                Map {
                    key: target_key,
                    value: target_value,
                    ..
                },
            ) => self.aggregate_layout_change(
                source,
                target,
                Some((source_key.as_ref(), target_key.as_ref())),
                (source_value.as_ref(), target_value.as_ref()),
            ),
            _ => LayoutChange::Identity,
        }
    }

    fn option_layout_change(&self, source: &ValueLayout, target: &ValueLayout) -> LayoutChange {
        use ValueLayout::{NullableOption, PointerOption, TaggedOption};
        match (source, target) {
            (
                TaggedOption {
                    payload: source_payload,
                    ..
                },
                NullableOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::UnwrapNullableOption {
                target_payload: target_payload.clone(),
                payload: Box::new(self.resolve_layout_change(source_payload, target_payload)),
            },
            (
                TaggedOption {
                    payload: source_payload,
                    ..
                },
                PointerOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::UnwrapPointerOption {
                target_payload: target_payload.clone(),
                payload: Box::new(self.resolve_layout_change(source_payload, target_payload)),
            },
            (
                NullableOption {
                    payload: source_payload,
                    ..
                },
                TaggedOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::WrapNullableOption {
                target: Box::new(target.clone()),
                target_payload: target_payload.clone(),
                payload: Box::new(self.resolve_layout_change(source_payload, target_payload)),
            },
            (
                PointerOption {
                    payload: source_payload,
                    ..
                },
                TaggedOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::WrapPointerOption {
                target: Box::new(target.clone()),
                target_payload: target_payload.clone(),
                payload: Box::new(self.resolve_layout_change(source_payload, target_payload)),
            },
            (
                TaggedOption {
                    payload: source_payload,
                    ..
                },
                TaggedOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::Immutable {
                target: Box::new(target.clone()),
                container: ImmutableContainer::Option,
                elements: vec![self.resolve_layout_change(source_payload, target_payload)],
            },
            (
                NullableOption {
                    payload: source_payload,
                    ..
                },
                NullableOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::NilablePayload {
                target: Box::new(target.clone()),
                slot: OptionSlot::Value,
                payload: Box::new(self.resolve_layout_change(source_payload, target_payload)),
            },
            (
                PointerOption {
                    payload: source_payload,
                    ..
                },
                PointerOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutChange::NilablePayload {
                target: Box::new(target.clone()),
                slot: OptionSlot::Pointer,
                payload: Box::new(self.resolve_layout_change(source_payload, target_payload)),
            },
            _ => LayoutChange::Identity,
        }
    }

    fn tuple_layout_change(
        &self,
        target: &ValueLayout,
        source_elements: &[ValueLayout],
        target_elements: &[ValueLayout],
    ) -> LayoutChange {
        assert_eq!(
            source_elements.len(),
            target_elements.len(),
            "tuple layout change preserves arity"
        );
        self.immutable_layout_change(
            target,
            ImmutableContainer::Tuple,
            source_elements.iter().zip(target_elements),
        )
    }

    fn immutable_layout_change<'l>(
        &self,
        target: &ValueLayout,
        container: ImmutableContainer,
        pairs: impl Iterator<Item = (&'l ValueLayout, &'l ValueLayout)>,
    ) -> LayoutChange {
        LayoutChange::Immutable {
            target: Box::new(target.clone()),
            container,
            elements: pairs
                .map(|(source, target)| self.resolve_layout_change(source, target))
                .collect(),
        }
    }

    fn reference_layout_change(
        &self,
        source_pointee: &ValueLayout,
        target_pointee: &ValueLayout,
        target: &ValueLayout,
    ) -> LayoutChange {
        use ValueLayout::Function;

        if let (
            Function { layout: source, .. },
            Function {
                layout: target_layout,
                ..
            },
        ) = (source_pointee.underlying(), target_pointee.underlying())
            && source.same_representation(target_layout)
        {
            return LayoutChange::Conversion {
                target: Box::new(target.clone()),
            };
        }
        let pointee = self.resolve_layout_change(source_pointee, target_pointee);
        if pointee.is_identity() {
            LayoutChange::Identity
        } else {
            LayoutChange::Reference {
                pointee: Box::new(pointee),
            }
        }
    }

    fn aggregate_layout_change(
        &self,
        source: &ValueLayout,
        target: &ValueLayout,
        key_layouts: Option<(&ValueLayout, &ValueLayout)>,
        element_layouts: (&ValueLayout, &ValueLayout),
    ) -> LayoutChange {
        let (source_element, target_element) = element_layouts;

        let key = key_layouts
            .map(|(source, target)| Box::new(self.resolve_layout_change(source, target)));
        let element = self.resolve_layout_change(source_element, target_element);
        if element.is_identity() && key.as_deref().is_none_or(LayoutChange::is_identity) {
            LayoutChange::Identity
        } else {
            LayoutChange::Aggregate(AggregateLayoutChange {
                source: Box::new(source.clone()),
                target: Box::new(target.clone()),
                key,
                element: Box::new(element),
            })
        }
    }

    fn aggregate_rebuild(&self, from: &Type, to: &Type) -> Option<CoercionPlan> {
        if let (Some((from_length, from_element)), Some((to_length, to_element))) =
            (self.facts.array_parts(from), self.facts.array_parts(to))
        {
            if from_length != to_length {
                return None;
            }
            let element = self.widening_element_plan(&from_element, &to_element)?;
            return Some(CoercionPlan::RebuildArray {
                array_type: to.clone(),
                element: Box::new(element),
            });
        }
        let (Some(from_elements), Some(to_elements)) = (
            self.facts.tuple_elements(from),
            self.facts.tuple_elements(to),
        ) else {
            return None;
        };

        if from_elements.len() != to_elements.len() {
            return None;
        }
        let elements: Vec<Option<CoercionPlan>> = from_elements
            .iter()
            .zip(&to_elements)
            .map(|(from, to)| self.widening_element_plan(from, to))
            .collect();
        if elements.iter().all(Option::is_none) {
            return None;
        }
        Some(CoercionPlan::RebuildTuple {
            elements: elements
                .into_iter()
                .map(|element| element.unwrap_or(CoercionPlan::Identity))
                .collect(),
            slot_types: to_elements,
        })
    }

    fn widening_element_plan(&self, from: &Type, to: &Type) -> Option<CoercionPlan> {
        let change = self.function_type_slot_layout_change(from, to);
        let plan = if change.is_identity() {
            CoercionPlan::internal(self, from, to)
        } else {
            change
        };
        (from != to && plan.changes_layout()).then_some(plan)
    }
}
