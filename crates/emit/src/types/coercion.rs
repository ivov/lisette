use syntax::ast::{Expression, UnaryOperator};
use syntax::types::{Type, peel_to_range_type};

use crate::Emitter;
use crate::definitions::interface_adapter::AdapterPlan;

pub(crate) struct Coercion {
    #[allow(dead_code)]
    from: Type,
    #[allow(dead_code)]
    to: Type,
    kind: CoercionKind,
}

pub(crate) enum CoercionKind {
    Identity,
    WrapAsInterface(AdapterPlan),
    UnwrapNullableOption {
        ty: Type,
    },
    UnwrapPointerOption {
        ty: Type,
    },
    UnwrapNullableCollection {
        ty: Type,
        elem_option_ty: Type,
    },
    WrapNullableOption {
        ty: Type,
    },
    WrapPointerOption {
        ty: Type,
    },
    WrapNullableCollection {
        ty: Type,
        elem_option_ty: Type,
    },
    /// Severs the backing-array alias on `let mut x = arr[range]` so writes
    /// through `x` do not mutate `arr`.
    CloneSubslice,
}

impl Coercion {
    pub(crate) fn resolve(emitter: &Emitter, from: &Type, to: &Type) -> Self {
        let kind = if let Some(plan) = emitter.needs_adapter(from, to) {
            CoercionKind::WrapAsInterface(plan)
        } else {
            CoercionKind::Identity
        };
        Self {
            from: from.clone(),
            to: to.clone(),
            kind,
        }
    }

    pub(crate) fn resolve_unwrap_go_nullable(
        emitter: &Emitter,
        value_ty: &Type,
        target_ty: Option<&Type>,
    ) -> Self {
        if let Some(target_ty) = target_ty
            && emitter.is_non_nilable_option(value_ty)
            && emitter.is_non_nilable_option(target_ty)
        {
            return Self {
                from: value_ty.clone(),
                to: target_ty.clone(),
                kind: CoercionKind::UnwrapPointerOption {
                    ty: value_ty.clone(),
                },
            };
        }
        let kind = if emitter.is_nullable_option(value_ty) {
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
        };
        Self {
            from: value_ty.clone(),
            to: value_ty.clone(),
            kind,
        }
    }

    pub(crate) fn resolve_subslice_clone(value: &Expression, mutable: bool) -> Self {
        let kind = if is_mutable_subslice(value, mutable) {
            CoercionKind::CloneSubslice
        } else {
            CoercionKind::Identity
        };
        let ty = value.get_type();
        Self {
            from: ty.clone(),
            to: ty,
            kind,
        }
    }

    pub(crate) fn resolve_wrap_go_nullable(emitter: &Emitter, value_ty: &Type) -> Self {
        let kind = if emitter.is_nullable_option(value_ty) {
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
        };
        Self {
            from: value_ty.clone(),
            to: value_ty.clone(),
            kind,
        }
    }

    pub(crate) fn apply(self, emitter: &mut Emitter, output: &mut String, value: String) -> String {
        match self.kind {
            CoercionKind::Identity => value,
            CoercionKind::WrapAsInterface(plan) => {
                let adapter_name = emitter.ensure_adapter_type(plan);
                format!("{}{{inner: {}}}", adapter_name, value)
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
            CoercionKind::CloneSubslice => {
                emitter.flags.needs_slices = true;
                format!("slices.Clone({})", value)
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_identity(&self) -> bool {
        matches!(self.kind, CoercionKind::Identity)
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
        let coercion = Coercion::resolve(self, &expression.get_type(), target);
        coercion.apply(self, output, emitted)
    }
}

fn is_mutable_subslice(value: &Expression, mutable: bool) -> bool {
    if !mutable {
        return false;
    }
    let value = value.unwrap_parens();
    let Expression::IndexedAccess {
        expression, index, ..
    } = value
    else {
        return false;
    };

    let is_range_index = matches!(**index, Expression::Range { .. })
        || peel_to_range_type(&index.get_type()).is_some();

    if !is_range_index {
        return false;
    }

    let collection_ty = match expression.as_ref() {
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => {
            let inner_ty = inner.get_type();
            inner_ty.inner().unwrap_or(inner_ty)
        }
        other => other.get_type(),
    };
    collection_ty.has_name("Slice")
}
