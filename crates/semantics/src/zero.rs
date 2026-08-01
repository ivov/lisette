//! Whether a type has a Lisette-side zero value, used by inference (struct
//! literal spreads) and by the `replaceable_with_autofill` lint.

use ecow::EcoString;
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, SimpleKind, SubstitutionMap, Symbol, Type, substitute};

use crate::store::Store;

/// Chain of field accesses leading to a non-zero-constructible field.
/// Used to render diagnostics like "outer.inner.b is private to module other".
#[derive(Debug, Clone)]
pub struct NoZero {
    pub(crate) chain: Vec<EcoString>,
    pub(crate) reason: NoZeroReason,
    pub leaf_ty: Type,
}

impl NoZero {
    pub fn hidden_go_state(&self) -> Option<&str> {
        match &self.reason {
            NoZeroReason::HiddenGoState { go_type } => Some(go_type),
            _ => None,
        }
    }
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
    /// A Go type with state Lisette cannot see, named for the diagnostic.
    HiddenGoState { go_type: EcoString },
}

/// Predicate: does `ty` have a Lisette-side zero, constructible from `from_module`?
/// Returns `Err(NoZero)` with a chain of field accesses to the offending leaf when
/// no zero is available; `Ok(())` otherwise.
pub fn has_zero(store: &Store, ty: &Type, from_module: &str) -> Result<(), NoZero> {
    has_zero_seen(store, ty, from_module, &mut Vec::new())
}

fn has_zero_seen(
    store: &Store,
    ty: &Type,
    from_module: &str,
    visited: &mut Vec<Type>,
) -> Result<(), NoZero> {
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
                if let Err(mut nz) = has_zero_seen(store, e, from_module, visited) {
                    let mut chain = vec![EcoString::from(i.to_string())];
                    chain.append(&mut nz.chain);
                    nz.chain = chain;
                    return Err(nz);
                }
            }
            Ok(())
        }
        Type::Array { length, element } => {
            if *length == 0 {
                Ok(())
            } else {
                has_zero_seen(store, element, from_module, visited)
            }
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
            has_zero_nominal(store, id, params, from_module, ty, visited)
        }
        Type::Forall { body, .. } => has_zero_seen(store, body, from_module, visited),
        Type::Var { .. }
        | Type::Uninferred
        | Type::Ignored
        | Type::Parameter(_)
        | Type::ReceiverPlaceholder => {
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

const MAX_ZERO_DEPTH: usize = 256;

fn has_zero_nominal(
    store: &Store,
    id: &Symbol,
    params: &[Type],
    from_module: &str,
    original_ty: &Type,
    visited: &mut Vec<Type>,
) -> Result<(), NoZero> {
    let size = type_node_count(original_ty);
    let is_recursion = visited.iter().any(|ancestor| match ancestor {
        Type::Nominal {
            id: ancestor_id, ..
        } => ancestor_id.as_str() == id.as_str() && type_node_count(ancestor) <= size,
        _ => false,
    });
    if is_recursion {
        return Ok(());
    }
    if visited.len() >= MAX_ZERO_DEPTH {
        return Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: original_ty.clone(),
        });
    }
    visited.push(original_ty.clone());
    let result = has_zero_nominal_fields(store, id, params, from_module, original_ty, visited);
    visited.pop();
    result
}

fn type_node_count(ty: &Type) -> usize {
    1 + ty
        .children()
        .iter()
        .map(|c| type_node_count(c))
        .sum::<usize>()
}

fn has_zero_nominal_fields(
    store: &Store,
    id: &Symbol,
    params: &[Type],
    from_module: &str,
    original_ty: &Type,
    visited: &mut Vec<Type>,
) -> Result<(), NoZero> {
    let Some(def) = store.get_definition(id.as_str()) else {
        // Unknown nominal, conservatively reject.
        return Err(NoZero {
            chain: vec![],
            reason: NoZeroReason::NoZeroForType,
            leaf_ty: original_ty.clone(),
        });
    };

    match &def.body {
        DefinitionBody::Struct { fields, .. } => {
            if def.has_hidden_fields() && !def.is_zero_safe() {
                return Err(NoZero {
                    chain: vec![],
                    reason: NoZeroReason::HiddenGoState {
                        go_type: go_display_name(id),
                    },
                    leaf_ty: original_ty.clone(),
                });
            }
            let def_ty = &def.ty;
            let map = build_substitution(def_ty, params);
            let struct_module = store
                .module_for_qualified_name(id.as_str())
                .unwrap_or(from_module);
            let struct_is_foreign = struct_module != from_module;

            let is_go_struct = id.as_str().starts_with("go:");
            let struct_name: EcoString = id.last_segment().into();
            for f in fields {
                if is_go_struct && curation_covers_embed(def, f) {
                    continue;
                }
                if struct_is_foreign && !f.visibility.is_public() && !is_go_struct {
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
                if let Err(mut nz) = has_zero_seen(store, &resolved, from_module, visited) {
                    let mut chain = vec![f.name.clone()];
                    chain.append(&mut nz.chain);
                    nz.chain = chain;
                    return Err(nz);
                }
            }
            Ok(())
        }
        DefinitionBody::TypeAlias { alias, .. } => {
            if matches!(alias, syntax::program::AliasKind::Opaque(_)) {
                if def.is_zero_safe() {
                    return Ok(());
                }
                return Err(NoZero {
                    chain: vec![],
                    reason: NoZeroReason::NoZeroForType,
                    leaf_ty: original_ty.clone(),
                });
            }
            let resolved = def
                .instantiate_alias_target(params)
                .expect("transparent alias has a target");
            has_zero_seen(store, &resolved, from_module, visited)
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

/// `go:archive/zip.File` as `archive/zip.File`, so diagnostics name the package.
fn go_display_name(id: &Symbol) -> EcoString {
    id.as_str()
        .strip_prefix(syntax::types::GO_IMPORT_PREFIX)
        .unwrap_or(id.as_str())
        .into()
}

/// An unexported embed is state no caller can reach, so the opt-in covers it.
pub fn curation_covers_embed(
    def: &syntax::program::Definition,
    field: &syntax::ast::StructFieldDefinition,
) -> bool {
    def.is_zero_safe() && field.is_embedded() && !field.visibility.is_public()
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
