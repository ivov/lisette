use syntax::ast::Expression;
use syntax::types::Type;

use crate::EmitEffects;
use crate::Planner;
use crate::calls::go_interop::WrapperTarget;
use crate::definitions::interface_adapter::AdapterPlan;
use crate::plan::bodies::LoweredStatement;
use crate::types::shape::NullableCollectionShape;

pub(crate) struct Coercion {
    kind: CoercionKind,
}

pub(crate) enum CoercionKind {
    Identity,
    WrapAsInterface(AdapterPlan),
    WrapNewtype {
        ty: Type,
    },
    UnwrapNullableOption {
        ty: Type,
    },
    UnwrapPointerOption {
        ty: Type,
    },
    UnwrapNullableCollection {
        ty: Type,
        shape: NullableCollectionShape,
    },
    UnwrapOptionToAny,
    WrapNullableOption {
        ty: Type,
    },
    WrapPointerOption {
        ty: Type,
    },
    WrapNullableCollection {
        ty: Type,
        shape: NullableCollectionShape,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum CoercionDirection {
    Internal,
    ToGoBoundary,
    FromGoBoundary,
}

impl Coercion {
    pub(crate) fn resolve(
        planner: &Planner,
        from: &Type,
        to: &Type,
        direction: CoercionDirection,
    ) -> Self {
        let kind = match direction {
            CoercionDirection::Internal => resolve_internal(planner, from, to),
            CoercionDirection::ToGoBoundary => resolve_to_go(planner, from, to),
            CoercionDirection::FromGoBoundary => resolve_from_go(planner, from),
        };
        Self { kind }
    }

    pub(crate) fn apply(
        self,
        planner: &mut Planner,
        output: &mut String,
        value: String,
        fx: &mut EmitEffects,
    ) -> String {
        match self.kind {
            CoercionKind::Identity => value,
            CoercionKind::WrapAsInterface(plan) => {
                let adapter_name = planner.ensure_adapter_type(plan, fx);
                format!("{}{{inner: {}}}", adapter_name, value)
            }
            CoercionKind::WrapNewtype { ty } => {
                let type_name = planner.go_type_string(&ty, fx);
                format!("{}({})", type_name, value)
            }
            CoercionKind::UnwrapNullableOption { ty } => {
                let inner = planner.go_type_string(&ty.ok_type(), fx);
                planner.emit_option_projection(output, &value, "unwrap", &inner, false, fx)
            }
            CoercionKind::UnwrapPointerOption { ty } => {
                let ptr = format!("*{}", planner.go_type_string(&ty.ok_type(), fx));
                planner.emit_option_projection(output, &value, "ptr", &ptr, true, fx)
            }
            CoercionKind::UnwrapNullableCollection { ty, shape } => {
                planner.emit_collection_nullable_unwrap(output, &value, &ty, &shape, fx)
            }
            CoercionKind::UnwrapOptionToAny => {
                planner.emit_option_projection(output, &value, "unwrap", "any", false, fx)
            }
            CoercionKind::WrapNullableOption { ty } => planner
                .emit_nil_check_option_wrap(output, &value, &ty, WrapperTarget::FreshSlot, fx)
                .expect("wrapper produced no slot"),
            CoercionKind::WrapPointerOption { ty } => {
                planner.emit_pointer_to_option_wrap(output, &value, &ty, fx)
            }
            CoercionKind::WrapNullableCollection { ty, shape } => {
                planner.emit_collection_nullable_wrap(output, &value, &ty, &shape, fx)
            }
        }
    }

    /// Typed counterpart of `apply`.
    pub(crate) fn lower(
        self,
        planner: &mut Planner,
        value: String,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let mut statements = Vec::new();
        let value = match self.kind {
            CoercionKind::Identity => value,
            CoercionKind::WrapAsInterface(plan) => {
                let adapter_name = planner.ensure_adapter_type(plan, fx);
                format!("{}{{inner: {}}}", adapter_name, value)
            }
            CoercionKind::WrapNewtype { ty } => {
                let type_name = planner.go_type_string(&ty, fx);
                format!("{}({})", type_name, value)
            }
            CoercionKind::UnwrapNullableOption { ty } => {
                let inner = planner.go_type_string(&ty.ok_type(), fx);
                planner.plan_option_projection(&mut statements, &value, "unwrap", &inner, false, fx)
            }
            CoercionKind::UnwrapPointerOption { ty } => {
                let ptr = format!("*{}", planner.go_type_string(&ty.ok_type(), fx));
                planner.plan_option_projection(&mut statements, &value, "ptr", &ptr, true, fx)
            }
            CoercionKind::UnwrapOptionToAny => {
                planner.plan_option_projection(&mut statements, &value, "unwrap", "any", false, fx)
            }
            CoercionKind::UnwrapNullableCollection { ty, shape } => {
                planner.plan_collection_nullable_unwrap(&mut statements, &value, &ty, &shape, fx)
            }
            CoercionKind::WrapNullableOption { ty } => {
                planner.plan_nil_check_option_wrap(&mut statements, &value, &ty, fx)
            }
            CoercionKind::WrapPointerOption { ty } => {
                planner.plan_pointer_to_option_wrap(&mut statements, &value, &ty, fx)
            }
            CoercionKind::WrapNullableCollection { ty, shape } => {
                planner.plan_collection_nullable_wrap(&mut statements, &value, &ty, &shape, fx)
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
        fx: &mut EmitEffects,
    ) -> String {
        let Some(target) = target_ty else {
            return emitted;
        };
        let coercion = Coercion::resolve(
            self,
            &expression.get_type(),
            target,
            CoercionDirection::Internal,
        );
        coercion.apply(self, output, emitted, fx)
    }
}

fn resolve_internal(planner: &Planner, from: &Type, to: &Type) -> CoercionKind {
    if let Some(plan) = planner.needs_adapter(from, to) {
        CoercionKind::WrapAsInterface(plan)
    } else if needs_newtype_wrap(planner, from, to) {
        CoercionKind::WrapNewtype { ty: to.clone() }
    } else {
        CoercionKind::Identity
    }
}

/// Option-related shape of a type at a Go boundary. Adding a new variant is
/// a compile-time call to revisit every `match` against it.
pub(crate) enum OptionShape {
    Plain,
    /// Lisette `Option<T>` where `T` is Go-nilable (interface, slice, pointer);
    /// the Go side uses nil itself as the absence marker.
    Nullable,
    /// Lisette `Option<T>` where `T` is a Go non-nilable scalar; the Go side
    /// uses `*T` and bridges nil ↔ `None`.
    PointerBridged,
    NullableCollection {
        shape: NullableCollectionShape,
    },
}

pub(crate) fn classify_option_shape(planner: &Planner, ty: &Type) -> OptionShape {
    if planner.facts.is_nullable_option(ty) {
        OptionShape::Nullable
    } else if planner.is_non_nilable_option(ty) {
        OptionShape::PointerBridged
    } else if let Some(shape) = planner.nullable_collection_shape(ty) {
        OptionShape::NullableCollection { shape }
    } else {
        OptionShape::Plain
    }
}

fn resolve_to_go(planner: &Planner, from: &Type, to: &Type) -> CoercionKind {
    use OptionShape::*;
    if to.resolves_to_unknown() && from.is_option() {
        return CoercionKind::UnwrapOptionToAny;
    }
    match classify_option_shape(planner, from) {
        Plain => CoercionKind::Identity,
        Nullable => CoercionKind::UnwrapNullableOption { ty: from.clone() },
        // Only unwrap to `*T` when the Go side also expects `*T`. A
        // pointer-bridged source against any other target stays tagged.
        PointerBridged if matches!(classify_option_shape(planner, to), PointerBridged) => {
            CoercionKind::UnwrapPointerOption { ty: from.clone() }
        }
        PointerBridged => CoercionKind::Identity,
        NullableCollection { shape } => CoercionKind::UnwrapNullableCollection {
            ty: from.clone(),
            shape,
        },
    }
}

fn resolve_from_go(planner: &Planner, from: &Type) -> CoercionKind {
    use OptionShape::*;
    match classify_option_shape(planner, from) {
        Plain => CoercionKind::Identity,
        Nullable => CoercionKind::WrapNullableOption { ty: from.clone() },
        PointerBridged => CoercionKind::WrapPointerOption { ty: from.clone() },
        NullableCollection { shape } => CoercionKind::WrapNullableCollection {
            ty: from.clone(),
            shape,
        },
    }
}

fn needs_newtype_wrap(planner: &Planner, from: &Type, to: &Type) -> bool {
    if from == to {
        return false;
    }
    let Some(underlying) = planner.get_newtype_underlying(to) else {
        return false;
    };
    underlying == *from
}
