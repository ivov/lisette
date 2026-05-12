use syntax::ast::Expression;
use syntax::types::Type;

use crate::Emitter;
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
                emitter.emit_option_unwrap_to_nullable(output, &value, &ty)
            }
            CoercionKind::UnwrapPointerOption { ty } => {
                emitter.emit_option_unwrap_to_go_pointer(output, &value, &ty)
            }
            CoercionKind::UnwrapNullableCollection { ty, elem_option_ty } => {
                emitter.emit_collection_nullable_unwrap(output, &value, &ty, &elem_option_ty)
            }
            CoercionKind::WrapNullableOption { ty } => {
                emitter.emit_nil_check_option_wrap(output, &value, &ty)
            }
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

fn resolve_to_go(emitter: &Emitter, value_ty: &Type, target_ty: &Type) -> CoercionKind {
    if emitter.is_non_nilable_option(value_ty) && emitter.is_non_nilable_option(target_ty) {
        return CoercionKind::UnwrapPointerOption {
            ty: value_ty.clone(),
        };
    }
    if emitter.is_nullable_option(value_ty) {
        CoercionKind::UnwrapNullableOption {
            ty: value_ty.clone(),
        }
    } else if let Some(elem_option_ty) = emitter.nullable_collection_element_ty(value_ty) {
        CoercionKind::UnwrapNullableCollection {
            ty: value_ty.clone(),
            elem_option_ty,
        }
    } else {
        CoercionKind::Identity
    }
}

fn resolve_from_go(emitter: &Emitter, value_ty: &Type) -> CoercionKind {
    if emitter.is_nullable_option(value_ty) {
        CoercionKind::WrapNullableOption {
            ty: value_ty.clone(),
        }
    } else if emitter.is_non_nilable_option(value_ty) {
        CoercionKind::WrapPointerOption {
            ty: value_ty.clone(),
        }
    } else if let Some(elem_option_ty) = emitter.nullable_collection_element_ty(value_ty) {
        CoercionKind::WrapNullableCollection {
            ty: value_ty.clone(),
            elem_option_ty,
        }
    } else {
        CoercionKind::Identity
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
