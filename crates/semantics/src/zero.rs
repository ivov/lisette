//! Whether a type has a Lisette-side zero value, used by inference (struct
//! literal spreads) and by the `replaceable_with_zero_fill` lint.

use ecow::EcoString;
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, SimpleKind, SubstitutionMap, Symbol, Type, substitute};

use crate::store::Store;

/// Chain of field accesses leading to a non-zero-constructible field.
/// Used to render diagnostics like "outer.inner.b is private to module other".
#[derive(Debug, Clone)]
pub struct NoZero {
    pub chain: Vec<EcoString>,
    pub reason: NoZeroReason,
    pub leaf_ty: Type,
}

#[derive(Debug, Clone)]
pub enum NoZeroReason {
    /// The leaf type itself has no defined zero (e.g., bare `fn`, `Channel<T>`,
    /// `Ref<T>`, `Result<T, E>`, enum without default variant).
    NoZeroForType,
    /// A nested user-defined struct has a private field unreachable from the
    /// calling module.
    PrivateField {
        struct_name: EcoString,
        field: EcoString,
        owning_module: EcoString,
    },
}

/// Predicate: does `ty` have a Lisette-side zero, constructible from `from_module`?
/// Returns `Err(NoZero)` with a chain of field accesses to the offending leaf when
/// no zero is available; `Ok(())` otherwise.
#[allow(clippy::result_large_err)]
pub fn has_zero(store: &Store, ty: &Type, from_module: &str) -> Result<(), NoZero> {
    match ty {
        Type::Simple(kind) => match kind {
            SimpleKind::Bool
            | SimpleKind::String
            | SimpleKind::Int
            | SimpleKind::Int8
            | SimpleKind::Int16
            | SimpleKind::Int32
            | SimpleKind::Int64
            | SimpleKind::Uint
            | SimpleKind::Uint8
            | SimpleKind::Uint16
            | SimpleKind::Uint32
            | SimpleKind::Uint64
            | SimpleKind::Uintptr
            | SimpleKind::Byte
            | SimpleKind::Float32
            | SimpleKind::Float64
            | SimpleKind::Complex64
            | SimpleKind::Complex128
            | SimpleKind::Rune
            | SimpleKind::Unit => Ok(()),
        },
        Type::Compound { kind, .. } => match kind {
            // Slice<T>, Map<K,V> always have a zero (empty, non-nil).
            CompoundKind::Slice | CompoundKind::Map | CompoundKind::EnumeratedSlice => Ok(()),
            // Ref<T>, Channel<T>, Sender<T>, Receiver<T>, VarArgs<T> have no zero.
            CompoundKind::Ref
            | CompoundKind::Channel
            | CompoundKind::Sender
            | CompoundKind::Receiver
            | CompoundKind::VarArgs => Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: ty.clone(),
            }),
        },
        Type::Tuple(elements) => {
            for (i, e) in elements.iter().enumerate() {
                if let Err(mut nz) = has_zero(store, e, from_module) {
                    let mut chain = vec![EcoString::from(i.to_string())];
                    chain.append(&mut nz.chain);
                    nz.chain = chain;
                    return Err(nz);
                }
            }
            Ok(())
        }
        Type::Function(_) => Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: ty.clone(),
        }),
        Type::Nominal { id, params, .. } => {
            if id.as_str() == "prelude.Option" {
                // Option<T>'s zero is None regardless of T. Stop recursion.
                return Ok(());
            }
            has_zero_nominal(store, id, params, from_module, ty)
        }
        Type::Forall { body, .. } => has_zero(store, body, from_module),
        Type::Var { .. } | Type::Parameter(_) | Type::ReceiverPlaceholder => {
            // Conservative: unresolved/abstract types have no known zero.
            Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: ty.clone(),
            })
        }
        Type::Never | Type::Error | Type::ImportNamespace(_) => Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: ty.clone(),
        }),
    }
}

#[allow(clippy::result_large_err)]
fn has_zero_nominal(
    store: &Store,
    id: &Symbol,
    params: &[Type],
    from_module: &str,
    original_ty: &Type,
) -> Result<(), NoZero> {
    // Go-imported nominal: every Go field has a Go zero by language definition.
    // Accept the whole nominal without recursing into its fields (Go's own
    // `T{}` zeroing is what the emit will use).
    if id.as_str().starts_with("go:") {
        return Ok(());
    }

    let Some(def) = store.get_definition(id.as_str()) else {
        // Unknown nominal — conservatively reject.
        return Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: original_ty.clone(),
        });
    };

    match &def.body {
        DefinitionBody::Struct { fields, .. } => {
            let def_ty = &def.ty;
            let map = build_substitution(def_ty, params);
            let struct_module = store
                .module_for_qualified_name(id.as_str())
                .unwrap_or(from_module);
            let struct_is_foreign = struct_module != from_module;
            let struct_name: EcoString = id.last_segment().into();
            for f in fields {
                if struct_is_foreign && !f.visibility.is_public() {
                    return Err(NoZero {
                        chain: vec![f.name.clone()],
                        reason: NoZeroReason::PrivateField {
                            struct_name: struct_name.clone(),
                            field: f.name.clone(),
                            owning_module: EcoString::from(struct_module),
                        },
                        leaf_ty: f.ty.clone(),
                    });
                }
                let resolved = if map.is_empty() {
                    f.ty.clone()
                } else {
                    substitute(&f.ty, &map)
                };
                if let Err(mut nz) = has_zero(store, &resolved, from_module) {
                    let mut chain = vec![f.name.clone()];
                    chain.append(&mut nz.chain);
                    nz.chain = chain;
                    return Err(nz);
                }
            }
            Ok(())
        }
        DefinitionBody::TypeAlias { annotation, .. } => {
            let alias_ty = &def.ty;
            if annotation.is_opaque() {
                return Err(NoZero {
                    chain: vec![],
                    reason: NoZeroReason::NoZeroForType,
                    leaf_ty: original_ty.clone(),
                });
            }
            let underlying = match alias_ty {
                Type::Forall { body, .. } => body.as_ref().clone(),
                other => other.clone(),
            };
            let underlying = store.peel_alias(&underlying);
            let map = build_substitution(alias_ty, params);
            let resolved = if map.is_empty() {
                underlying
            } else {
                substitute(&underlying, &map)
            };
            has_zero(store, &resolved, from_module)
        }
        // Enums and other definitions have no zero unless we add a designated
        // default-variant mechanism later.
        _ => Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: original_ty.clone(),
        }),
    }
}

fn build_substitution(def_ty: &Type, params: &[Type]) -> SubstitutionMap {
    let mut map = SubstitutionMap::default();
    if let Type::Forall { vars, .. } = def_ty
        && vars.len() == params.len()
    {
        for (var, param) in vars.iter().zip(params.iter()) {
            map.insert(var.clone(), param.clone());
        }
    }
    map
}
