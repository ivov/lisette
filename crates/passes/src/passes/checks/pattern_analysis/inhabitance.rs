use semantics::store::Store;
use syntax::ast::{EnumVariant, Generic, StructFieldDefinition};
use syntax::program::AliasKind;
use syntax::program::DefinitionBody;
use syntax::types::Type;
use syntax::types::{SubstitutionMap, build_substitution_map, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InhabitanceState {
    Visiting,
    Inhabited,
    Uninhabited,
}

#[derive(Default)]
pub struct InhabitanceCache {
    states: Vec<(Type, InhabitanceState)>,
}

#[must_use]
struct PendingInhabitance(usize);

impl InhabitanceCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn lookup(&self, ty: &Type) -> Option<bool> {
        self.states
            .iter()
            .rev()
            .find(|(key, _)| key == ty)
            .map(|(_, state)| *state != InhabitanceState::Uninhabited)
    }

    fn begin(&mut self, ty: Type) -> PendingInhabitance {
        let pending = PendingInhabitance(self.states.len());
        self.states.push((ty, InhabitanceState::Visiting));
        pending
    }

    fn finish(&mut self, pending: PendingInhabitance, inhabited: bool) {
        self.states[pending.0].1 = if inhabited {
            InhabitanceState::Inhabited
        } else {
            InhabitanceState::Uninhabited
        };
    }
}

pub fn is_inhabited(ty: &Type, store: &Store, cache: &mut InhabitanceCache) -> bool {
    match ty {
        Type::Never => return false,
        Type::Function(_) => return true,
        Type::Var { .. } | Type::Uninferred | Type::Ignored | Type::Parameter(_) => return true,
        _ => {}
    }

    if let Type::Tuple(elements) = ty {
        return elements.iter().all(|e| is_inhabited(e, store, cache));
    }

    if let Some(inhabited) = cache.lookup(ty) {
        return inhabited;
    }
    let pending = cache.begin(ty.clone());

    let result = match ty {
        Type::Nominal { id, params, .. } => check_constructor_inhabited(id, params, store, cache),
        Type::Forall { body, .. } => is_inhabited(body, store, cache),
        Type::Array { length, element } => *length == 0 || is_inhabited(element, store, cache),
        _ => true,
    };

    cache.finish(pending, result);

    result
}

fn check_constructor_inhabited(
    id: &str,
    params: &[Type],
    store: &Store,
    cache: &mut InhabitanceCache,
) -> bool {
    let Some(definition) = store.get_definition(id) else {
        return true;
    };

    match &definition.body {
        DefinitionBody::Enum {
            generics, variants, ..
        } => {
            let map = build_substitution_map(generics, params);
            variants
                .iter()
                .any(|v| is_variant_inhabited_with_map(v, &map, store, cache))
        }

        DefinitionBody::Struct {
            generics, fields, ..
        } => {
            let map = build_substitution_map(generics, params);
            fields.iter().all(|f| {
                let field_ty = substitute(&f.ty, &map);
                is_inhabited(&field_ty, store, cache)
            })
        }

        DefinitionBody::TypeAlias {
            generics, alias, ..
        } => {
            let AliasKind::Transparent { target, .. } = alias else {
                return true;
            };
            let map = build_substitution_map(generics, params);
            let target_ty = substitute(target, &map);

            if is_self_referential_alias(id, &target_ty) {
                return true;
            }

            is_inhabited(&target_ty, store, cache)
        }

        DefinitionBody::Interface { .. } | DefinitionBody::Value { .. } => true,
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
    cache: &mut InhabitanceCache,
) -> bool {
    let map = build_substitution_map(generics, type_args);
    is_variant_inhabited_with_map(variant, &map, store, cache)
}

fn is_variant_inhabited_with_map(
    variant: &EnumVariant,
    map: &SubstitutionMap,
    store: &Store,
    cache: &mut InhabitanceCache,
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
    cache: &mut InhabitanceCache,
) -> bool {
    let map = build_substitution_map(generics, type_args);
    fields.iter().all(|f| {
        let field_ty = substitute(&f.ty, &map);
        is_inhabited(&field_ty, store, cache)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_visits_remain_provisional_until_their_own_result_is_finalized() {
        let mut cache = InhabitanceCache::new();
        assert_eq!(cache.lookup(&Type::int()), None);
        let outer = cache.begin(Type::int());
        let inner = cache.begin(Type::string());
        assert_eq!(cache.lookup(&Type::int()), Some(true));
        assert_eq!(cache.lookup(&Type::string()), Some(true));

        cache.finish(inner, false);
        assert_eq!(cache.lookup(&Type::int()), Some(true));
        assert_eq!(cache.lookup(&Type::string()), Some(false));
        cache.finish(outer, true);

        assert_eq!(cache.lookup(&Type::int()), Some(true));
        assert_eq!(cache.lookup(&Type::string()), Some(false));
    }
}
