use crate::names::go_name;
use syntax::program::{Adaptation, SlotOrigin};
use syntax::types::Type;

use crate::Planner;
use crate::control_flow::fallible::{option_variant, prelude_call};
use crate::definitions::interface_adapter::AdapterPlan;
use crate::ir::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, Statement, assign, declare,
};
use crate::ir::go_expression::CompositeLayout;
use crate::ir::values::GoExpression;
use crate::types::go_type::GoType;

use super::layout::{FunctionLayout, ValueLayout};

#[derive(Debug, Clone)]
pub(crate) enum CoercionPlan {
    Identity,
    WrapAsInterface(AdapterPlan),
    WrapNewtype {
        ty: Type,
    },
    WrapSome {
        payload_type: Type,
        payload: Box<CoercionPlan>,
    },
    Layout(LayoutBridge),
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
pub(crate) enum BridgeDirection {
    ToGo,
    FromGo,
}

#[derive(Debug, Clone)]
pub(crate) enum LayoutBridge {
    Identity,
    Conversion {
        target: Box<ValueLayout>,
    },
    UnwrapNullableOption {
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    UnwrapPointerOption {
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    WrapNullableOption {
        target: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    WrapPointerOption {
        target: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    Reference {
        pointee: Box<LayoutBridge>,
    },
    Function {
        source: Box<FunctionLayout>,
        target: Box<FunctionLayout>,
    },
    NilablePayload {
        target: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    Immutable {
        target: Box<ValueLayout>,
        elements: Vec<LayoutBridge>,
    },
    Aggregate {
        source: Box<ValueLayout>,
        target: Box<ValueLayout>,
        key: Option<Box<LayoutBridge>>,
        element: Box<LayoutBridge>,
    },
}

impl LayoutBridge {
    pub(crate) fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }

    pub(crate) fn direction(&self) -> Option<BridgeDirection> {
        match self {
            Self::Identity | Self::Conversion { .. } => None,
            Self::UnwrapNullableOption { .. } | Self::UnwrapPointerOption { .. } => {
                Some(BridgeDirection::ToGo)
            }
            Self::WrapNullableOption { .. } | Self::WrapPointerOption { .. } => {
                Some(BridgeDirection::FromGo)
            }
            Self::Reference { pointee } => pointee.direction(),
            Self::NilablePayload { payload, .. } => payload.direction(),
            Self::Function { .. } => None,
            Self::Immutable { elements, .. } => elements.iter().find_map(LayoutBridge::direction),
            Self::Aggregate { key, element, .. } => key
                .as_deref()
                .and_then(LayoutBridge::direction)
                .or_else(|| element.direction()),
        }
    }
}

impl CoercionPlan {
    pub(crate) fn adapted(
        planner: &Planner<'_>,
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
                    .unwrap_or_else(|| planner.value_layout(to, SlotOrigin::Lisette));
                if matches!(layout.underlying(), ValueLayout::TaggedOption { .. }) {
                    Self::WrapSome {
                        payload_type,
                        payload: Box::new(payload),
                    }
                } else {
                    payload
                }
            }
            Adaptation::Tuple(elements) => {
                match (facts.tuple_elements(from), facts.tuple_elements(to)) {
                    (Some(sources), Some(slots)) => Self::RebuildTuple {
                        elements: elements
                            .iter()
                            .zip(sources.iter().zip(&slots))
                            .map(|(element, (from, to))| {
                                Self::adapted_element(planner, element, from, to)
                            })
                            .collect(),
                        slot_types: slots,
                    },
                    _ => Self::internal(planner, from, to),
                }
            }
            Adaptation::Array(element) => match (facts.array_parts(from), facts.array_parts(to)) {
                (Some((_, source)), Some((_, slot))) => Self::RebuildArray {
                    array_type: to.clone(),
                    element: Box::new(Self::adapted_element(planner, element, &source, &slot)),
                },
                _ => Self::internal(planner, from, to),
            },
            Adaptation::DivergingFunction => {
                let source = planner.value_layout(from, SlotOrigin::Lisette);
                let target = slot
                    .cloned()
                    .unwrap_or_else(|| planner.value_layout(to, SlotOrigin::Lisette));
                match (source.underlying(), target.underlying()) {
                    (
                        ValueLayout::Function { layout: source, .. },
                        ValueLayout::Function { layout: target, .. },
                    ) => Self::Layout(LayoutBridge::Function {
                        source: Box::new(source.clone()),
                        target: Box::new(target.clone()),
                    }),
                    _ => Self::internal(planner, from, to),
                }
            }
            _ => Self::internal(planner, from, to),
        }
    }

    fn adapted_element(
        planner: &Planner<'_>,
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

    pub(crate) fn internal(planner: &Planner<'_>, from: &Type, to: &Type) -> Self {
        if from == to {
            Self::Identity
        } else if planner.facts.is_interface_or_unknown(to)
            && let Some((stored, lisette)) = planner.any_slot_function_layouts(from)
        {
            Self::bridge(planner, &lisette, &stored)
        } else if let Some(rebuild) = planner.aggregate_rebuild(from, to) {
            rebuild
        } else {
            Self::Identity
        }
    }

    pub(crate) fn bridge(
        planner: &Planner<'_>,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> Self {
        let bridge = planner.resolve_layout_bridge(source, target);
        if bridge.is_identity() {
            Self::Identity
        } else {
            Self::Layout(bridge)
        }
    }

    pub(crate) fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }

    pub(crate) fn lower(
        self,
        planner: &mut Planner<'_>,
        value: GoExpression,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let value = match self {
            Self::Identity => value,
            Self::WrapAsInterface(plan) => {
                let adapter_name = planner.ensure_adapter_type(plan);
                GoExpression::struct_literal(
                    GoType::new(adapter_name),
                    vec![("inner".to_string(), value)],
                    CompositeLayout::Inline { padded: false },
                )
            }
            Self::WrapNewtype { ty } => {
                let type_name = planner.go_type(&ty);
                GoExpression::conversion(type_name, value)
            }
            Self::WrapSome {
                payload_type,
                payload,
            } => {
                let (payload_setup, payload) = payload.lower(planner, value);
                statements.extend(payload_setup);
                let type_arguments =
                    GoType::type_argument(&planner.component_go_type(&payload_type));
                prelude_call(
                    option_variant("Some").constructor,
                    type_arguments,
                    vec![payload],
                )
            }
            Self::Layout(bridge) => {
                planner.plan_layout_bridge(&mut statements, value, &bridge, true)
            }
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
        let go_type = self.go_type(array_type);
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
            self.make_tuple_callee(slot_types, slot_types.len()),
            arguments,
        )
    }

    pub(crate) fn resolve_layout_bridge(
        &self,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> LayoutBridge {
        if source.same_representation(target) {
            return LayoutBridge::Identity;
        }

        use ValueLayout::{
            Array, Fallible, Function, Map, Named, NullableOption, PointerOption, Reference, Slice,
            TaggedOption, Tuple,
        };

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
                return LayoutBridge::Identity;
            }
            return LayoutBridge::Conversion {
                target: Box::new(target.clone()),
            };
        }

        match (source, target) {
            (
                TaggedOption { .. } | NullableOption { .. } | PointerOption { .. },
                TaggedOption { .. } | NullableOption { .. } | PointerOption { .. },
            ) => self.option_bridge(source, target),
            (Fallible { .. }, Fallible { .. }) | (Tuple { .. }, Tuple { .. }) => {
                self.immutable_bridge(source, target)
            }
            (Function { layout: source, .. }, Function { layout: target, .. }) => {
                LayoutBridge::Function {
                    source: Box::new(source.clone()),
                    target: Box::new(target.clone()),
                }
            }
            (Reference { .. }, Reference { .. }) => self.reference_bridge(source, target),
            (Slice { .. }, Slice { .. })
            | (Array { .. }, Array { .. })
            | (Map { .. }, Map { .. }) => self.aggregate_bridge(source, target),
            (
                Named {
                    underlying: source_underlying,
                    ..
                },
                target,
            ) => self.resolve_layout_bridge(source_underlying, target),
            (
                source,
                Named {
                    underlying: target_underlying,
                    ..
                },
            ) => self.resolve_layout_bridge(source, target_underlying),
            _ => LayoutBridge::Identity,
        }
    }

    fn option_bridge(&self, source: &ValueLayout, target: &ValueLayout) -> LayoutBridge {
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
            ) => LayoutBridge::UnwrapNullableOption {
                target_payload: target_payload.clone(),
                payload: Box::new(self.resolve_layout_bridge(source_payload, target_payload)),
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
            ) => LayoutBridge::UnwrapPointerOption {
                target_payload: target_payload.clone(),
                payload: Box::new(self.resolve_layout_bridge(source_payload, target_payload)),
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
            ) => LayoutBridge::WrapNullableOption {
                target: Box::new(target.clone()),
                payload: Box::new(self.resolve_layout_bridge(source_payload, target_payload)),
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
            ) => LayoutBridge::WrapPointerOption {
                target: Box::new(target.clone()),
                payload: Box::new(self.resolve_layout_bridge(source_payload, target_payload)),
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
            ) => LayoutBridge::Immutable {
                target: Box::new(target.clone()),
                elements: vec![self.resolve_layout_bridge(source_payload, target_payload)],
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
            )
            | (
                PointerOption {
                    payload: source_payload,
                    ..
                },
                PointerOption {
                    payload: target_payload,
                    ..
                },
            ) => LayoutBridge::NilablePayload {
                target: Box::new(target.clone()),
                payload: Box::new(self.resolve_layout_bridge(source_payload, target_payload)),
            },
            _ => LayoutBridge::Identity,
        }
    }

    fn immutable_bridge(&self, source: &ValueLayout, target: &ValueLayout) -> LayoutBridge {
        use ValueLayout::{Fallible, Tuple};
        match (source, target) {
            (
                Fallible {
                    payload: source_payload,
                    error: source_error,
                    ..
                },
                Fallible {
                    payload: target_payload,
                    error: target_error,
                    ..
                },
            ) => LayoutBridge::Immutable {
                target: Box::new(target.clone()),
                elements: vec![
                    self.resolve_layout_bridge(source_payload, target_payload),
                    self.resolve_layout_bridge(source_error, target_error),
                ],
            },
            (
                Tuple {
                    elements: source_elements,
                    ..
                },
                Tuple {
                    elements: target_elements,
                    ..
                },
            ) => {
                assert_eq!(
                    source_elements.len(),
                    target_elements.len(),
                    "tuple layout bridge preserves arity"
                );
                LayoutBridge::Immutable {
                    target: Box::new(target.clone()),
                    elements: source_elements
                        .iter()
                        .zip(target_elements)
                        .map(|(source, target)| self.resolve_layout_bridge(source, target))
                        .collect(),
                }
            }
            _ => unreachable!("immutable_bridge takes a fallible or tuple pair"),
        }
    }

    fn reference_bridge(&self, source: &ValueLayout, target: &ValueLayout) -> LayoutBridge {
        use ValueLayout::{Function, Reference};
        let (
            Reference {
                pointee: source_pointee,
                ..
            },
            Reference {
                pointee: target_pointee,
                ..
            },
        ) = (source, target)
        else {
            unreachable!("reference_bridge takes a reference pair");
        };
        if let (
            Function { layout: source, .. },
            Function {
                layout: target_layout,
                ..
            },
        ) = (source_pointee.underlying(), target_pointee.underlying())
            && source.same_representation(target_layout)
        {
            return LayoutBridge::Conversion {
                target: Box::new(target.clone()),
            };
        }
        let pointee = self.resolve_layout_bridge(source_pointee, target_pointee);
        if pointee.is_identity() {
            LayoutBridge::Identity
        } else {
            LayoutBridge::Reference {
                pointee: Box::new(pointee),
            }
        }
    }

    fn aggregate_bridge(&self, source: &ValueLayout, target: &ValueLayout) -> LayoutBridge {
        use ValueLayout::{Array, Map, Slice};
        let (key_layouts, source_element, target_element) = match (source, target) {
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
            ) => (None, source_element, target_element),
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
            ) => (
                Some((source_key.as_ref(), target_key.as_ref())),
                source_value,
                target_value,
            ),
            _ => unreachable!("aggregate_bridge takes a slice, array or map pair"),
        };
        let key = key_layouts
            .map(|(source, target)| Box::new(self.resolve_layout_bridge(source, target)));
        let element = self.resolve_layout_bridge(source_element, target_element);
        if element.is_identity() && key.as_deref().is_none_or(LayoutBridge::is_identity) {
            LayoutBridge::Identity
        } else {
            LayoutBridge::Aggregate {
                source: Box::new(source.clone()),
                target: Box::new(target.clone()),
                key,
                element: Box::new(element),
            }
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
        let bridge = self.function_type_slot_bridge(from, to);
        let plan = if bridge.is_identity() {
            CoercionPlan::internal(self, from, to)
        } else {
            bridge
        };
        (from != to && !plan.is_identity()).then_some(plan)
    }
}
