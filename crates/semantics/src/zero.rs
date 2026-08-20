//! Whether a type has a Lisette-side zero value, used by inference (struct
//! literal spreads) and by the `replaceable_with_autofill` lint.

use ecow::EcoString;
use syntax::ast::StructFieldDefinition;
use syntax::ast::StructFields;
use syntax::program::AliasKind;
use syntax::program::Definition;
use syntax::program::DefinitionBody;
use syntax::types;
use syntax::types::{
    CompoundKind, SimpleKind, SubstitutionMap, Symbol, Type, build_named_substitution_map,
    substitute,
};

use crate::store::Store;

/// Chain of field accesses leading to a non-zero-constructible field.
/// Used to render diagnostics like "outer.inner.b is private to package other".
#[derive(Debug, Clone)]
pub struct NoZero {
    pub(crate) chain: Vec<EcoString>,
    pub(crate) reason: NoZeroReason,
    pub leaf_ty: Box<Type>,
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
    /// calling package.
    PrivateField {
        struct_name: EcoString,
        field: EcoString,
        owning_package: EcoString,
    },
    /// A Go type curated as broken at its zero value, named for the diagnostic.
    HiddenGoState { go_type: EcoString },
    /// The leaf type is a `Map`, whose Go zero is nil.
    NilMap,
}

/// Who supplies the zero value of a `Map` reached while walking a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapZero {
    /// The compiler builds an empty map.
    Built,
    /// Go leaves it nil, and a nil map panics on write.
    Nil,
}

/// Predicate: does `ty` have a Lisette-side zero, constructible from `from_package`?
/// Returns `Err(NoZero)` with a chain of field accesses to the offending leaf when
/// no zero is available; `Ok(())` otherwise.
pub fn has_zero(
    store: &Store,
    ty: &Type,
    from_package: &str,
    map_zero: MapZero,
) -> Result<(), NoZero> {
    ZeroWalk {
        store,
        from_package,
        map_zero,
        visited: Vec::new(),
    }
    .walk(ty)
}

struct ZeroWalk<'a> {
    store: &'a Store,
    from_package: &'a str,
    map_zero: MapZero,
    visited: Vec<Type>,
}

impl ZeroWalk<'_> {
    const MAX_ZERO_DEPTH: usize = 256;

    fn walk(&mut self, ty: &Type) -> Result<(), NoZero> {
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
                CompoundKind::Map => match self.map_zero {
                    MapZero::Built => Ok(()),
                    MapZero::Nil => Err(NoZero {
                        chain: vec![],
                        reason: NoZeroReason::NilMap,
                        leaf_ty: Box::new(ty.clone()),
                    }),
                },
                // A nil slice reads, lengths, and appends like an empty one.
                CompoundKind::Slice | CompoundKind::EnumeratedSlice => Ok(()),
                // Ref<T>, Channel<T>, Sender<T>, Receiver<T>, VarArgs<T> have no zero.
                CompoundKind::Ref
                | CompoundKind::Channel
                | CompoundKind::Sender
                | CompoundKind::Receiver
                | CompoundKind::VarArgs => Err(NoZero {
                    chain: vec![],
                    reason: NoZeroReason::NoZeroForType,
                    leaf_ty: Box::new(ty.clone()),
                }),
            },
            Type::Tuple(elements) => {
                for (i, e) in elements.iter().enumerate() {
                    if let Err(mut nz) = self.walk(e) {
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
                    self.walk(element)
                }
            }
            Type::Function(_) => Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: Box::new(ty.clone()),
            }),
            Type::Nominal { id, params, .. } => {
                if id.as_str() == "prelude.Option" {
                    // Option<T>'s zero is None regardless of T. Stop recursion.
                    return Ok(());
                }
                self.walk_nominal(id, params, ty)
            }
            Type::Forall { body, .. } => self.walk(body),
            Type::Var { .. }
            | Type::Uninferred
            | Type::Ignored
            | Type::Parameter(_)
            | Type::ReceiverPlaceholder => {
                // Conservative: unresolved/abstract types have no known zero.
                Err(NoZero {
                    chain: vec![],
                    reason: NoZeroReason::NoZeroForType,
                    leaf_ty: Box::new(ty.clone()),
                })
            }
            Type::Never | Type::Error | Type::ImportNamespace(_) => Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: Box::new(ty.clone()),
            }),
        }
    }

    fn walk_nominal(
        &mut self,
        id: &Symbol,
        params: &[Type],
        original_ty: &Type,
    ) -> Result<(), NoZero> {
        let size = type_node_count(original_ty);
        let is_recursion = self.visited.iter().any(|ancestor| match ancestor {
            Type::Nominal {
                id: ancestor_id, ..
            } => ancestor_id.as_str() == id.as_str() && type_node_count(ancestor) <= size,
            _ => false,
        });
        if is_recursion {
            return Ok(());
        }
        if self.visited.len() >= Self::MAX_ZERO_DEPTH {
            return Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: Box::new(original_ty.clone()),
            });
        }
        self.visited.push(original_ty.clone());
        let result = self.walk_nominal_fields(id, params, original_ty);
        self.visited.pop();
        result
    }

    fn walk_nominal_fields(
        &mut self,
        id: &Symbol,
        params: &[Type],
        original_ty: &Type,
    ) -> Result<(), NoZero> {
        let Some(def) = self.store.get_definition(id.as_str()) else {
            // Unknown nominal, conservatively reject.
            return Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: Box::new(original_ty.clone()),
            });
        };

        match &def.body {
            DefinitionBody::Struct { fields, .. } => {
                let is_go_struct = id.as_str().starts_with("go:");
                if is_go_struct && go_struct_denies_zero(def, fields) {
                    return Err(NoZero {
                        chain: vec![],
                        reason: NoZeroReason::HiddenGoState {
                            go_type: go_display_name(id),
                        },
                        leaf_ty: Box::new(original_ty.clone()),
                    });
                }
                let def_ty = &def.ty;
                let map = build_substitution(def_ty, params);
                let struct_package = self
                    .store
                    .package_for_qualified_name(id.as_str())
                    .unwrap_or(self.from_package);
                let struct_is_foreign = struct_package != self.from_package;

                let struct_name: EcoString = id.last_segment().into();
                for f in fields {
                    if is_go_struct && hidden_embed_field(f) {
                        continue;
                    }
                    if struct_is_foreign && !f.visibility.is_public() && !is_go_struct {
                        return Err(NoZero {
                            chain: vec![f.name.clone()],
                            reason: NoZeroReason::PrivateField {
                                struct_name: struct_name.clone(),
                                field: f.name.clone(),
                                owning_package: EcoString::from(struct_package),
                            },
                            leaf_ty: Box::new(f.ty.clone()),
                        });
                    }
                    let resolved = if map.is_empty() {
                        f.ty.clone()
                    } else {
                        substitute(&f.ty, &map)
                    };
                    if let Err(mut nz) = self.walk(&resolved) {
                        let mut chain = vec![f.name.clone()];
                        chain.append(&mut nz.chain);
                        nz.chain = chain;
                        return Err(nz);
                    }
                }
                Ok(())
            }
            DefinitionBody::TypeAlias { alias, .. } => {
                if matches!(alias, AliasKind::Opaque(_)) {
                    if def.is_zero_safe() {
                        return Ok(());
                    }
                    return Err(NoZero {
                        chain: vec![],
                        reason: NoZeroReason::NoZeroForType,
                        leaf_ty: Box::new(original_ty.clone()),
                    });
                }
                let resolved = def
                    .instantiate_alias_target(params, false)
                    .expect("transparent alias has a target");
                self.walk(&resolved)
            }
            // Enums and other definitions have no zero unless we add a designated
            // default-variant mechanism later.
            _ => Err(NoZero {
                chain: vec![],
                reason: NoZeroReason::NoZeroForType,
                leaf_ty: Box::new(original_ty.clone()),
            }),
        }
    }
}

fn type_node_count(ty: &Type) -> usize {
    1 + ty
        .children()
        .iter()
        .map(|c| type_node_count(c))
        .sum::<usize>()
}

/// `go:archive/zip.File` as `archive/zip.File`, so diagnostics name the package.
fn go_display_name(id: &Symbol) -> EcoString {
    id.as_str()
        .strip_prefix(types::GO_IMPORT_PREFIX)
        .unwrap_or(id.as_str())
        .into()
}

/// Zero-construction verdict for a Go-imported struct. A struct with visible
/// fields is designed for literal construction, so its hidden state is
/// presumed zero-safe (matching Go's own struct literals) unless curated
/// `zero_unsafe`. A struct whose only content is hidden state is opaque to
/// callers, so it stays refused unless curated `zero_safe`.
pub fn go_struct_denies_zero(def: &Definition, fields: &StructFields) -> bool {
    if def.is_zero_unsafe() {
        return true;
    }
    if def.is_zero_safe() {
        return false;
    }
    let has_hidden_state = def.has_hidden_fields() || fields.iter().any(hidden_embed_field);
    has_hidden_state && !fields.iter().any(|f| f.visibility.is_public())
}

/// An unexported embed is hidden Go state like any dropped private field, so
/// the struct-level verdict covers it and per-field zero checks skip it.
pub fn hidden_embed_field(field: &StructFieldDefinition) -> bool {
    field.is_embedded() && !field.visibility.is_public()
}

fn build_substitution(def_ty: &Type, params: &[Type]) -> SubstitutionMap {
    if let Type::Forall { vars, .. } = def_ty
        && vars.len() == params.len()
    {
        return build_named_substitution_map(vars, params);
    }
    SubstitutionMap::default()
}
