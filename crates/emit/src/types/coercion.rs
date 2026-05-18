use syntax::ast::Expression;
use syntax::types::Type;

use crate::Emitter;
use crate::calls::go_interop::WrapperTarget;
use crate::definitions::interface_adapter::AdapterPlan;

pub(crate) struct Coercion {
    kind: CoercionKind,
}

pub(crate) enum CoercionKind {
    Identity,
    WrapAsInterface(AdapterPlan),
    WrapNewtype { ty: Type },
    UnwrapNullableOption { ty: Type },
    UnwrapPointerOption { ty: Type },
    UnwrapNullableCollection { ty: Type, elem_option_ty: Type },
    UnwrapOptionToAny,
    WrapNullableOption { ty: Type },
    WrapPointerOption { ty: Type },
    WrapNullableCollection { ty: Type, elem_option_ty: Type },
}

/// Outbound and inbound Option-bridge cases look identical by type alone,
/// so direction is picked at the call site rather than inferred.
#[derive(Clone, Copy)]
pub(crate) enum CoercionDirection {
    Internal,
    ToGoBoundary,
    FromGoBoundary,
}

impl Coercion {
    pub(crate) fn resolve(
        emitter: &Emitter,
        from: &Type,
        to: &Type,
        direction: CoercionDirection,
    ) -> Self {
        let kind = match direction {
            CoercionDirection::Internal => resolve_internal(emitter, from, to),
            CoercionDirection::ToGoBoundary => resolve_to_go(emitter, from, to),
            CoercionDirection::FromGoBoundary => resolve_from_go(emitter, from),
        };
        Self { kind }
    }

    pub(crate) fn apply(self, emitter: &mut Emitter, output: &mut String, value: String) -> String {
        match self.kind {
            CoercionKind::Identity => value,
            CoercionKind::WrapAsInterface(plan) => {
                let adapter_name = emitter.ensure_adapter_type(plan);
                format!("{}{{inner: {}}}", adapter_name, value)
            }
            CoercionKind::WrapNewtype { ty } => {
                let type_name = emitter.go_type_as_string(&ty);
                format!("{}({})", type_name, value)
            }
            CoercionKind::UnwrapNullableOption { ty } => {
                let inner = emitter.go_type_as_string(&ty.ok_type());
                emitter.emit_option_projection(output, &value, "unwrap", &inner, false)
            }
            CoercionKind::UnwrapPointerOption { ty } => {
                let ptr = format!("*{}", emitter.go_type_as_string(&ty.ok_type()));
                emitter.emit_option_projection(output, &value, "ptr", &ptr, true)
            }
            CoercionKind::UnwrapNullableCollection { ty, elem_option_ty } => {
                emitter.emit_collection_nullable_unwrap(output, &value, &ty, &elem_option_ty)
            }
            CoercionKind::UnwrapOptionToAny => {
                emitter.emit_option_projection(output, &value, "unwrap", "any", false)
            }
            CoercionKind::WrapNullableOption { ty } => emitter
                .emit_nil_check_option_wrap(output, &value, &ty, WrapperTarget::FreshSlot)
                .expect("wrapper produced no slot"),
            CoercionKind::WrapPointerOption { ty } => {
                emitter.emit_pointer_to_option_wrap(output, &value, &ty)
            }
            CoercionKind::WrapNullableCollection { ty, elem_option_ty } => {
                emitter.emit_collection_nullable_wrap(output, &value, &ty, &elem_option_ty)
            }
        }
    }
}

impl Emitter<'_> {
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
        let coercion = Coercion::resolve(
            self,
            &expression.get_type(),
            target,
            CoercionDirection::Internal,
        );
        coercion.apply(self, output, emitted)
    }
}

fn resolve_internal(emitter: &Emitter, from: &Type, to: &Type) -> CoercionKind {
    if let Some(plan) = emitter.needs_adapter(from, to) {
        CoercionKind::WrapAsInterface(plan)
    } else if needs_newtype_wrap(emitter, from, to) {
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
        elem_option_ty: Type,
    },
}

pub(crate) fn classify_option_shape(emitter: &Emitter, ty: &Type) -> OptionShape {
    if emitter.facts.is_nullable_option(ty) {
        OptionShape::Nullable
    } else if emitter.is_non_nilable_option(ty) {
        OptionShape::PointerBridged
    } else if let Some(shape) = emitter.nullable_collection_shape(ty) {
        OptionShape::NullableCollection {
            elem_option_ty: shape.elem_option_ty,
        }
    } else {
        OptionShape::Plain
    }
}

fn resolve_to_go(emitter: &Emitter, from: &Type, to: &Type) -> CoercionKind {
    use OptionShape::*;
    if to.resolves_to_unknown() && from.is_option() {
        return CoercionKind::UnwrapOptionToAny;
    }
    match classify_option_shape(emitter, from) {
        Plain => CoercionKind::Identity,
        Nullable => CoercionKind::UnwrapNullableOption { ty: from.clone() },
        // Only unwrap to `*T` when the Go side also expects `*T`. A
        // pointer-bridged source against any other target stays tagged.
        PointerBridged if matches!(classify_option_shape(emitter, to), PointerBridged) => {
            CoercionKind::UnwrapPointerOption { ty: from.clone() }
        }
        PointerBridged => CoercionKind::Identity,
        NullableCollection { elem_option_ty } => CoercionKind::UnwrapNullableCollection {
            ty: from.clone(),
            elem_option_ty,
        },
    }
}

fn resolve_from_go(emitter: &Emitter, from: &Type) -> CoercionKind {
    use OptionShape::*;
    match classify_option_shape(emitter, from) {
        Plain => CoercionKind::Identity,
        Nullable => CoercionKind::WrapNullableOption { ty: from.clone() },
        PointerBridged => CoercionKind::WrapPointerOption { ty: from.clone() },
        NullableCollection { elem_option_ty } => CoercionKind::WrapNullableCollection {
            ty: from.clone(),
            elem_option_ty,
        },
    }
}

fn needs_newtype_wrap(emitter: &Emitter, from: &Type, to: &Type) -> bool {
    if from == to {
        return false;
    }
    let Some(underlying) = emitter.get_newtype_underlying(to) else {
        return false;
    };
    underlying == *from
}
