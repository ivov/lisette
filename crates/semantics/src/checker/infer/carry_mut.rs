use rustc_hash::FxHashSet;

use crate::checker::EnvResolve;
use crate::checker::type_env::TypeEnv;
use crate::store::Store;
use syntax::ast::VariantFields;
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, Symbol, Type, build_substitution_map, substitute};

/// Whether a value of `ty` can carry mutation across a function call boundary.
///
/// True for `Slice<T>`, `Map<K,V>`, `EnumeratedSlice<T>`, and any struct,
/// tuple, or enum that recursively contains one. `Ref<T>`, `Channel<T>`,
/// `Sender<T>`, and `Receiver<T>` are excluded by design.
pub(super) fn can_carry_mutation_across_fn_boundary(
    ty: &Type,
    env: &TypeEnv,
    store: &Store,
) -> bool {
    let mut visited: FxHashSet<Symbol> = FxHashSet::default();
    can_carry_mutation(ty, env, store, &mut visited)
}

/// Whether `.clone()` on `ty` fully severs. Only built-in container clones
/// qualify, and they leave collections inside a nominal element shared, so a
/// container of such elements does not sever.
pub(super) fn clone_severs_alias(ty: &Type, env: &TypeEnv, store: &Store) -> bool {
    let resolved = store.peel_alias(&ty.resolve_in(env));
    match &resolved {
        Type::Compound {
            kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice,
            args,
        } => args
            .first()
            .is_some_and(|elem| element_severed(elem, env, store)),
        Type::Compound {
            kind: CompoundKind::Map,
            args,
        } => args
            .get(1)
            .is_some_and(|value| element_severed(value, env, store)),
        _ => false,
    }
}

/// Whether the built-in clone fully copies this element: scalars and nested
/// containers/tuples do, a carry-mut nominal does not (it copies shallowly).
fn element_severed(ty: &Type, env: &TypeEnv, store: &Store) -> bool {
    let resolved = store.peel_alias(&ty.resolve_in(env));
    if !can_carry_mutation_across_fn_boundary(&resolved, env, store) {
        return true;
    }
    match &resolved {
        Type::Compound {
            kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice,
            args,
        } => args
            .first()
            .is_some_and(|elem| element_severed(elem, env, store)),
        Type::Compound {
            kind: CompoundKind::Map,
            args,
        } => args
            .get(1)
            .is_some_and(|value| element_severed(value, env, store)),
        Type::Tuple(elems) => elems.iter().all(|e| element_severed(e, env, store)),
        Type::Forall { body, .. } => element_severed(body, env, store),
        _ => false,
    }
}

/// Whether `.clone()` on `ty` is a built-in container clone (depth the
/// compiler controls). A nominal `.clone()` is user-written, so it is treated
/// as an ordinary call producing a fresh value.
pub(super) fn clone_recipe_is_known(ty: &Type, env: &TypeEnv, store: &Store) -> bool {
    let resolved = store.peel_alias(&ty.resolve_in(env));
    matches!(
        resolved,
        Type::Compound {
            kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
            ..
        }
    )
}

fn can_carry_mutation(
    ty: &Type,
    env: &TypeEnv,
    store: &Store,
    visited: &mut FxHashSet<Symbol>,
) -> bool {
    let resolved = ty.resolve_in(env);
    match &resolved {
        Type::Compound { kind, args } => match kind {
            CompoundKind::Slice | CompoundKind::Map | CompoundKind::EnumeratedSlice => true,
            CompoundKind::Ref
            | CompoundKind::Channel
            | CompoundKind::Sender
            | CompoundKind::Receiver => false,
            CompoundKind::VarArgs => args
                .first()
                .is_some_and(|inner| can_carry_mutation(inner, env, store, visited)),
        },
        Type::Tuple(elems) => elems
            .iter()
            .any(|e| can_carry_mutation(e, env, store, visited)),
        Type::Nominal { id, params, .. } => {
            let peeled = store.peel_alias(&resolved);
            if !matches!(&peeled, Type::Nominal { id: pid, .. } if pid == id) {
                return can_carry_mutation(&peeled, env, store, visited);
            }
            if !visited.insert(id.clone()) {
                return false;
            }
            let result = nominal_can_carry_mutation(id, params, env, store, visited);
            visited.remove(id);
            result
        }
        Type::Forall { body, .. } => can_carry_mutation(body, env, store, visited),
        Type::Function(_)
        | Type::Var { .. }
        | Type::Parameter(_)
        | Type::Simple(_)
        | Type::Never
        | Type::ImportNamespace(_)
        | Type::ReceiverPlaceholder
        | Type::Error => false,
    }
}

fn nominal_can_carry_mutation(
    id: &Symbol,
    params: &[Type],
    env: &TypeEnv,
    store: &Store,
    visited: &mut FxHashSet<Symbol>,
) -> bool {
    let Some(def) = store.get_definition(id.as_str()) else {
        return false;
    };
    match &def.body {
        DefinitionBody::Struct {
            generics, fields, ..
        } => {
            let map = build_substitution_map(generics, params);
            fields.iter().any(|f| {
                let substituted = substitute(&f.ty, &map);
                can_carry_mutation(&substituted, env, store, visited)
            })
        }
        DefinitionBody::Enum {
            generics, variants, ..
        } => {
            let map = build_substitution_map(generics, params);
            variants.iter().any(|v| match &v.fields {
                VariantFields::Unit => false,
                VariantFields::Tuple(fields) | VariantFields::Struct(fields) => {
                    fields.iter().any(|f| {
                        let substituted = substitute(&f.ty, &map);
                        can_carry_mutation(&substituted, env, store, visited)
                    })
                }
            })
        }
        DefinitionBody::TypeAlias { .. }
        | DefinitionBody::Interface { .. }
        | DefinitionBody::Value { .. } => false,
    }
}
