use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

use crate::Planner;
use crate::Renderer;
use crate::definitions::interface_adapter::AdapterPlan;
use crate::names::go_name;
use crate::plan::bodies::{LoopKind, LoopPlan, LoweredBlock, LoweredStatement};

use super::callable::AbiTransition;
use super::layout::{FunctionLayout, ValueLayout};

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
    UnwrapNullableOption {
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    UnwrapPointerOption {
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    WrapNullableOption {
        option_type: Type,
        payload: Box<LayoutBridge>,
    },
    WrapPointerOption {
        option_type: Type,
        payload: Box<LayoutBridge>,
    },
    Reference {
        pointee: Box<LayoutBridge>,
    },
    Function {
        source: Box<FunctionLayout>,
        target: Box<FunctionLayout>,
        direction: BridgeDirection,
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
            Self::Identity => None,
            Self::UnwrapNullableOption { .. } | Self::UnwrapPointerOption { .. } => {
                Some(BridgeDirection::ToGo)
            }
            Self::WrapNullableOption { .. } | Self::WrapPointerOption { .. } => {
                Some(BridgeDirection::FromGo)
            }
            Self::Reference { pointee } => pointee.direction(),
            Self::Function { direction, .. } => Some(*direction),
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
        value: String,
    ) -> (Vec<LoweredStatement>, String) {
        let mut statements = Vec::new();
        let value = match self {
            Self::Identity => value,
            Self::WrapAsInterface(plan) => {
                let adapter_name = planner.ensure_adapter_type(plan);
                format!("{}{{inner: {}}}", adapter_name, value)
            }
            Self::WrapNewtype { ty } => {
                let type_name = planner.go_type_string(&ty);
                format!("{}({})", type_name, value)
            }
            Self::Layout(bridge) => planner.plan_layout_bridge(&mut statements, &value, &bridge),
            Self::RebuildArray {
                array_type,
                element,
            } => planner.plan_array_rebuild(&mut statements, &value, &array_type, *element),
            Self::RebuildTuple {
                slot_types,
                elements,
            } => planner.plan_tuple_rebuild(&mut statements, &value, &slot_types, elements),
        };
        (statements, value)
    }
}

impl Planner<'_> {
    pub(crate) fn apply_type_coercion(
        &mut self,
        output: &mut String,
        target_ty: Option<&Type>,
        expression: &Expression,
        emitted: String,
    ) -> String {
        let Some(target) = target_ty else {
            return emitted;
        };
        let coercion = CoercionPlan::internal(self, &expression.get_type(), target);
        let (setup, value) = coercion.lower(self, emitted);
        output.push_str(&Renderer.render_setup(&setup));
        value
    }

    /// Bind `value` to a name that can be read more than once.
    fn stable_source(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        hint: &str,
        value: &str,
    ) -> String {
        if go_name::is_plain_identifier(value) {
            return value.to_string();
        }
        self.hoist_tmp_value_statement(statements, hint, value)
    }

    fn plan_array_rebuild(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: &str,
        array_type: &Type,
        element: CoercionPlan,
    ) -> String {
        let source = self.stable_source(statements, "src", value);
        let go_type = self.go_type_string(array_type);
        let output = self.fresh_var(Some("boxed"));
        self.declare(&output);
        statements.push(LoweredStatement::VarDecl {
            name: output.clone(),
            go_type,
            value: None,
        });

        let index = self.fresh_var(Some("i"));
        self.declare(&index);
        let (mut body, coerced) = element.lower(self, format!("{source}[{index}]"));
        body.push(LoweredStatement::RawGo(format!(
            "{output}[{index}] = {coerced}\n"
        )));
        statements.push(LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            kind: LoopKind::Generated { label: None },
            header: format!("for {index} := range {source} {{\n"),
            body: LoweredBlock { statements: body },
        }));
        output
    }

    fn plan_tuple_rebuild(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: &str,
        slot_types: &[Type],
        elements: Vec<CoercionPlan>,
    ) -> String {
        let source = self.stable_source(statements, "tup", value);

        let mut arguments = Vec::with_capacity(elements.len());
        for (index, element) in elements.into_iter().enumerate() {
            let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
            let (element_setup, coerced) = element.lower(self, format!("{source}.{field}"));
            statements.extend(element_setup);
            arguments.push(coerced);
        }
        format!(
            "{}({})",
            self.make_tuple_callee(slot_types, slot_types.len()),
            arguments.join(", ")
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
        Array, Function, Map, Named, NullableOption, PointerOption, Reference, Slice, TaggedOption,
    };

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
                option_type,
                payload: target_payload,
            },
        ) => LayoutBridge::WrapNullableOption {
            option_type: option_type.clone(),
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
                option_type,
                payload: target_payload,
            },
        ) => LayoutBridge::WrapPointerOption {
            option_type: option_type.clone(),
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
            target,
        ) if is_go_interface_slot(planner, target.logical_type()) => {
            LayoutBridge::UnwrapNullableOption {
                target_payload: Box::new(target.clone()),
                payload: Box::new(resolve_layout_bridge(planner, source_payload, target)),
            }
        }
        (Function { layout: source, .. }, Function { layout: target, .. })
            if source.return_abi == target.return_abi =>
        {
            LayoutBridge::Function {
                direction: function_bridge_direction(planner, source, target),
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

fn function_bridge_direction(
    planner: &Planner<'_>,
    source: &FunctionLayout,
    target: &FunctionLayout,
) -> BridgeDirection {
    let result = resolve_layout_bridge(planner, &source.result, &target.result).direction();
    let payload = source
        .payload
        .as_deref()
        .zip(target.payload.as_deref())
        .and_then(|(source, target)| resolve_layout_bridge(planner, source, target).direction());
    let parameter = target
        .parameters
        .iter()
        .zip(&source.parameters)
        .find_map(|(target, source)| resolve_layout_bridge(planner, target, source).direction())
        .map(invert_direction);
    result
        .or(payload)
        .or(parameter)
        .or_else(
            || match source.return_abi.transition_to(&target.return_abi) {
                AbiTransition::LowerFromTagged => Some(BridgeDirection::ToGo),
                AbiTransition::WrapToTagged => Some(BridgeDirection::FromGo),
                AbiTransition::Identity | AbiTransition::Reencode | AbiTransition::Incompatible => {
                    None
                }
            },
        )
        .unwrap_or(BridgeDirection::ToGo)
}

fn invert_direction(direction: BridgeDirection) -> BridgeDirection {
    match direction {
        BridgeDirection::ToGo => BridgeDirection::FromGo,
        BridgeDirection::FromGo => BridgeDirection::ToGo,
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

fn is_go_interface_slot(planner: &Planner<'_>, ty: &Type) -> bool {
    planner
        .facts
        .as_interface(ty)
        .is_some_and(|id| go_name::is_go_import(&id))
}

fn aggregate_rebuild(planner: &Planner<'_>, from: &Type, to: &Type) -> Option<CoercionPlan> {
    match (from, to) {
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
    let plan = CoercionPlan::internal(planner, from, to);
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
