use syntax::ast::Expression;
use syntax::types::Type;

use crate::Planner;
use crate::Renderer;
use crate::definitions::interface_adapter::AdapterPlan;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;

use super::callable::AbiTransition;
use super::layout::{FunctionLayout, ValueLayout};

pub(crate) struct CoercionPlan {
    kind: CoercionKind,
}

enum CoercionKind {
    Identity,
    WrapAsInterface(AdapterPlan),
    WrapNewtype { ty: Type },
    Layout(LayoutBridge),
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
        option_type: Type,
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    UnwrapPointerOption {
        option_type: Type,
        target_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    WrapNullableOption {
        option_type: Type,
        source_payload: Box<ValueLayout>,
        payload: Box<LayoutBridge>,
    },
    WrapPointerOption {
        option_type: Type,
        source_payload: Box<ValueLayout>,
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
        let kind = if let Some(plan) = planner.needs_adapter(from, to) {
            CoercionKind::WrapAsInterface(plan)
        } else if needs_newtype_wrap(planner, from, to) {
            CoercionKind::WrapNewtype { ty: to.clone() }
        } else {
            CoercionKind::Identity
        };
        Self { kind }
    }

    pub(crate) fn bridge(
        planner: &Planner<'_>,
        source: &ValueLayout,
        target: &ValueLayout,
    ) -> Self {
        let bridge = resolve_layout_bridge(planner, source, target);
        let kind = if bridge.is_identity() {
            CoercionKind::Identity
        } else {
            CoercionKind::Layout(bridge)
        };
        Self { kind }
    }

    pub(crate) fn is_identity(&self) -> bool {
        matches!(self.kind, CoercionKind::Identity)
    }

    pub(crate) fn lower(
        self,
        planner: &mut Planner<'_>,
        value: String,
    ) -> (Vec<LoweredStatement>, String) {
        let mut statements = Vec::new();
        let value = match self.kind {
            CoercionKind::Identity => value,
            CoercionKind::WrapAsInterface(plan) => {
                let adapter_name = planner.ensure_adapter_type(plan);
                format!("{}{{inner: {}}}", adapter_name, value)
            }
            CoercionKind::WrapNewtype { ty } => {
                let type_name = planner.go_type_string(&ty);
                format!("{}({})", type_name, value)
            }
            CoercionKind::Layout(bridge) => {
                planner.plan_layout_bridge(&mut statements, &value, &bridge)
            }
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
                option_type,
                payload: source_payload,
            },
            NullableOption {
                payload: target_payload,
                ..
            },
        ) => LayoutBridge::UnwrapNullableOption {
            option_type: option_type.clone(),
            target_payload: target_payload.clone(),
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
        },
        (
            TaggedOption {
                option_type,
                payload: source_payload,
            },
            PointerOption {
                payload: target_payload,
                ..
            },
        ) => LayoutBridge::UnwrapPointerOption {
            option_type: option_type.clone(),
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
            source_payload: source_payload.clone(),
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
            source_payload: source_payload.clone(),
            payload: Box::new(resolve_layout_bridge(
                planner,
                source_payload,
                target_payload,
            )),
        },
        (
            TaggedOption {
                option_type,
                payload: source_payload,
            },
            target,
        ) if is_go_interface_slot(planner, target.logical_type()) => {
            LayoutBridge::UnwrapNullableOption {
                option_type: option_type.clone(),
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

fn needs_newtype_wrap(planner: &Planner<'_>, from: &Type, to: &Type) -> bool {
    if from == to {
        return false;
    }
    let Some(underlying) = planner.get_newtype_underlying(to) else {
        return false;
    };
    underlying == *from
}
