use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;

use crate::store::Store;
use syntax::ast::{EnumVariant, Generic, StructFieldDefinition};
use syntax::program::Definition;
use syntax::types::Type;
use syntax::types::{SubstitutionMap, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InhabitanceState {
    Visiting,
    Inhabited,
    Uninhabited,
}

#[derive(Default)]
pub struct InhabitanceCache {
    cache: RefCell<HashMap<String, InhabitanceState>>,
}

impl InhabitanceCache {
    pub fn new() -> Self {
        Self::default()
    }
}

fn type_key(ty: &Type) -> String {
    match ty {
        Type::Never => "Never".to_string(),
        Type::Nominal { id, params, .. } => {
            if params.is_empty() {
                id.to_string()
            } else {
                let param_keys: Vec<String> = params.iter().map(type_key).collect();
                format!("{}<{}>", id, param_keys.join(", "))
            }
        }
        Type::Tuple(elements) => {
            let elem_keys: Vec<String> = elements.iter().map(type_key).collect();
            format!("({})", elem_keys.join(", "))
        }
        Type::Function { .. } => "fn".to_string(),
        Type::Var { .. } | Type::Parameter(_) | Type::Error => "param".to_string(),
        Type::Forall { body, .. } => type_key(body),
        Type::ImportNamespace(m) => format!("<import:{}>", m),
        Type::ReceiverPlaceholder => "<receiver>".to_string(),
        Type::Simple(kind) => kind.leaf_name().to_string(),
        Type::Compound { kind, args } => {
            if args.is_empty() {
                kind.leaf_name().to_string()
            } else {
                let arg_keys: Vec<String> = args.iter().map(type_key).collect();
                format!("{}<{}>", kind.leaf_name(), arg_keys.join(", "))
            }
        }
    }
}

pub fn is_inhabited(ty: &Type, store: &Store, cache: &InhabitanceCache) -> bool {
    match ty {
        Type::Never => return false,
        Type::Function { .. } => return true,
        Type::Var { .. } | Type::Parameter(_) => return true,
        _ => {}
    }

    if let Type::Tuple(elements) = ty {
        return elements.iter().all(|e| is_inhabited(e, store, cache));
    }

    let key = type_key(ty);

    {
        let cache_ref = cache.cache.borrow();
        if let Some(state) = cache_ref.get(&key) {
            return match state {
                InhabitanceState::Visiting => true,
                InhabitanceState::Inhabited => true,
                InhabitanceState::Uninhabited => false,
            };
        }
    }

    cache
        .cache
        .borrow_mut()
        .insert(key.clone(), InhabitanceState::Visiting);

    let result = match ty {
        Type::Nominal { id, params, .. } => check_constructor_inhabited(id, params, store, cache),
        Type::Forall { body, .. } => is_inhabited(body, store, cache),
        _ => true,
    };

    let final_state = if result {
        InhabitanceState::Inhabited
    } else {
        InhabitanceState::Uninhabited
    };
    cache.cache.borrow_mut().insert(key, final_state);

    result
}

fn check_constructor_inhabited(
    id: &str,
    params: &[Type],
    store: &Store,
    cache: &InhabitanceCache,
) -> bool {
    let Some(definition) = store.get_definition(id) else {
        return true;
    };

    match definition {
        Definition::Enum {
            generics, variants, ..
        } => {
            let map = build_substitution_map(generics, params);
            variants
                .iter()
                .any(|v| is_variant_inhabited_with_map(v, &map, store, cache))
        }

        Definition::Struct {
            generics, fields, ..
        } => {
            let map = build_substitution_map(generics, params);
            fields.iter().all(|f| {
                let field_ty = substitute(&f.ty, &map);
                is_inhabited(&field_ty, store, cache)
            })
        }

        Definition::TypeAlias { ty, generics, .. } => {
            let map = build_substitution_map(generics, params);
            let target_ty = substitute(ty, &map);

            if is_self_referential_alias(id, &target_ty) {
                return true;
            }

            is_inhabited(&target_ty, store, cache)
        }

        Definition::ValueEnum { .. } | Definition::Interface { .. } | Definition::Value { .. } => {
            true
        }
    }
}

fn is_self_referential_alias(alias_id: &str, target_ty: &Type) -> bool {
    match target_ty {
        Type::Nominal { id, .. } => id == alias_id,
        Type::Forall { body, .. } => is_self_referential_alias(alias_id, body),
        _ => false,
    }
}

pub fn is_variant_inhabited(
    variant: &EnumVariant,
    type_args: &[Type],
    generics: &[Generic],
    store: &Store,
    cache: &InhabitanceCache,
) -> bool {
    let map = build_substitution_map(generics, type_args);
    is_variant_inhabited_with_map(variant, &map, store, cache)
}

fn is_variant_inhabited_with_map(
    variant: &EnumVariant,
    map: &SubstitutionMap,
    store: &Store,
    cache: &InhabitanceCache,
) -> bool {
    variant.fields.iter().all(|field| {
        let field_ty = substitute(&field.ty, map);
        is_inhabited(&field_ty, store, cache)
    })
}

pub fn is_struct_inhabited(
    fields: &[StructFieldDefinition],
    type_args: &[Type],
    generics: &[Generic],
    store: &Store,
    cache: &InhabitanceCache,
) -> bool {
    let map = build_substitution_map(generics, type_args);
    fields.iter().all(|f| {
        let field_ty = substitute(&f.ty, &map);
        is_inhabited(&field_ty, store, cache)
    })
}

fn build_substitution_map(generics: &[Generic], type_args: &[Type]) -> SubstitutionMap {
    generics
        .iter()
        .map(|g| g.name.clone())
        .zip(type_args.iter().cloned())
        .collect()
}
