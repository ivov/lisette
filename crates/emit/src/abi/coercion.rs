use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

use crate::Planner;
use crate::definitions::interface_adapter::AdapterPlan;
use crate::plan::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, Statement, assign,
};
use crate::plan::go_expression::CompositeLayout;
use crate::plan::values::GoExpression;

use super::layout::{FunctionLayout, ValueLayout};

#[derive(Debug, Clone)]
pub(crate) enum CoercionPlan {
    Identity,
    WrapAsInterface(AdapterPlan),
    WrapNewtype {
        ty: Type,
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
    pub(crate) fn internal(planner: &Planner<'_>, from: &Type, to: &Type) -> Self {
        if from == to {
            Self::Identity
        } else if let Some(plan) = planner.needs_adapter(from, to) {
            Self::WrapAsInterface(plan)
        } else if planner.facts.is_interface_or_unknown(to)
            && let Some((stored, lisette)) = planner.any_slot_function_layouts(from)
        {
            Self::bridge(planner, &lisette, &stored)
        } else if needs_newtype_wrap(planner, from, to) {
            Self::WrapNewtype { ty: to.clone() }
        } else if let Some(rebuild) = aggregate_rebuild(planner, from, to) {
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
        let bridge = resolve_layout_bridge(planner, source, target);
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
                    adapter_name,
                    vec![("inner".to_string(), value)],
                    CompositeLayout::Inline { padded: false },
                )
            }
            Self::WrapNewtype { ty } => {
                let type_name = planner.use_go_type(&ty);
                GoExpression::conversion(type_name, value)
            }
            Self::Layout(bridge) => planner.plan_layout_bridge(&mut statements, value, &bridge),
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

    fn plan_array_rebuild(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        array_type: &Type,
        element: CoercionPlan,
    ) -> GoExpression {
        let source = self.stable_source(statements, "src", value);
        let go_type = self.use_go_type(array_type);
        let output = self.fresh_local(Some("boxed"));
        self.declare(&output);
        statements.push(
            LoweredStatement::VarDecl {
                name: output.clone(),
                go_type,
                value: None,
            }
            .into(),
        );

        let index = self.fresh_local(Some("i"));
        self.declare(&index);
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
            let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
            let slot_read = GoExpression::selector(source.clone(), field.to_string());
            let (element_setup, coerced) = element.lower(self, slot_read);
            statements.extend(element_setup);
            arguments.push(coerced);
        }
        GoExpression::pure_call(
            self.make_tuple_callee(slot_types, slot_types.len()),
            arguments,
        )
    }
}

pub(crate) fn resolve_layout_bridge(
    planner: &Planner<'_>,
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
        // Go assigns an unnamed function to the same underlying signature.
        // Two defined function types require an explicit conversion.
        if source.function_name().is_none() || target.function_name().is_none() {
            return LayoutBridge::Identity;
        }
        return LayoutBridge::Conversion {
            target: Box::new(target.clone()),
        };
    }

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
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
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
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
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
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
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
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
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
            elements: vec![resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )],
        },
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
                resolve_layout_bridge(planner, source_payload, target_payload),
                resolve_layout_bridge(planner, source_error, target_error),
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
                    .map(|(source, target)| resolve_layout_bridge(planner, source, target))
                    .collect(),
            }
        }
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
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
        },
        (Function { layout: source, .. }, Function { layout: target, .. }) => {
            LayoutBridge::Function {
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
        ) => {
            if let (
                Function { layout: source, .. },
                Function {
                    layout: target_layout,
                    ..
                },
            ) = (source_pointee.underlying(), target_pointee.underlying())
                && source.same_representation(target_layout)
            {
                // A pointer conversion changes only the named function type,
                // keeping both references attached to the same mutable slot.
                return LayoutBridge::Conversion {
                    target: Box::new(target.clone()),
                };
            }
            let pointee = resolve_layout_bridge(planner, source_pointee, target_pointee);
            if pointee.is_identity() {
                LayoutBridge::Identity
            } else {
                LayoutBridge::Reference {
                    pointee: Box::new(pointee),
                }
            }
        }
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
        ) => aggregate_bridge(
            planner,
            source,
            target,
            None,
            source_element,
            target_element,
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
        ) => aggregate_bridge(
            planner,
            source,
            target,
            Some((source_key, target_key)),
            source_value,
            target_value,
        ),
        (
            Named {
                underlying: source_underlying,
                ..
            },
            target,
        ) => resolve_layout_bridge(planner, source_underlying, target),
        (
            source,
            Named {
                underlying: target_underlying,
                ..
            },
        ) => resolve_layout_bridge(planner, source, target_underlying),
        _ => LayoutBridge::Identity,
    }
}

fn aggregate_bridge(
    planner: &Planner<'_>,
    source: &ValueLayout,
    target: &ValueLayout,
    key_layouts: Option<(&ValueLayout, &ValueLayout)>,
    source_element: &ValueLayout,
    target_element: &ValueLayout,
) -> LayoutBridge {
    let key = key_layouts
        .map(|(source, target)| Box::new(resolve_layout_bridge(planner, source, target)));
    let element = resolve_layout_bridge(planner, source_element, target_element);
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

fn aggregate_rebuild(planner: &Planner<'_>, from: &Type, to: &Type) -> Option<CoercionPlan> {
    match (
        &planner.facts.peel_alias(from),
        &planner.facts.peel_alias(to),
    ) {
        (
            Type::Array {
                length: from_length,
                element: from_element,
            },
            Type::Array {
                length: to_length,
                element: to_element,
            },
        ) if from_length == to_length => {
            let element = widening_element_plan(planner, from_element, to_element)?;
            Some(CoercionPlan::RebuildArray {
                array_type: to.clone(),
                element: Box::new(element),
            })
        }
        (Type::Tuple(from_elements), Type::Tuple(to_elements))
            if from_elements.len() == to_elements.len() =>
        {
            let elements: Vec<Option<CoercionPlan>> = from_elements
                .iter()
                .zip(to_elements)
                .map(|(from, to)| widening_element_plan(planner, from, to))
                .collect();
            if elements.iter().all(Option::is_none) {
                return None;
            }
            Some(CoercionPlan::RebuildTuple {
                slot_types: to_elements.clone(),
                elements: elements
                    .into_iter()
                    .map(|element| element.unwrap_or(CoercionPlan::Identity))
                    .collect(),
            })
        }
        _ => None,
    }
}

fn widening_element_plan(planner: &Planner<'_>, from: &Type, to: &Type) -> Option<CoercionPlan> {
    let bridge = planner.function_type_slot_bridge(from, to);
    let plan = if bridge.is_identity() {
        CoercionPlan::internal(planner, from, to)
    } else {
        bridge
    };
    (from != to && (planner.facts.is_interface_or_unknown(to) || !plan.is_identity()))
        .then_some(plan)
}

fn needs_newtype_wrap(planner: &Planner<'_>, from: &Type, to: &Type) -> bool {
    if from == to {
        return false;
    }
    let Some(underlying) = planner.get_newtype_underlying(to) else {
        return false;
    };
    underlying == *from
}
