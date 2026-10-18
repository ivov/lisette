//! Union-find-style binding table for `Type::Var` handles.
//!
//! `Type::Var(TypeVarId)` is a handle; the binding (Unbound vs Bound-to-a-Type)
//! lives here in `entries`, indexed by id. Cloning a `Type` clones just the
//! handle, so `Type` is a pure value (Clone / Eq / Hash / Serialize friendly)
//! with no shared mutable state.
//!
//! Speculative unification uses checkpoints into one undo log. Nested commits
//! leave their writes in the parent region; rollback restores slots before
//! truncating the bound-type pool.

use std::num::NonZeroU32;
use syntax::types::FunctionParameter;
use syntax::types::{Bound, Type, TypeVarId};

/// Zero means unbound; nonzero indices address the append-only bound-type pool.
#[derive(Debug, Clone, Copy)]
struct BindingSlot(NonZeroU32);

type VarState = Option<BindingSlot>;

const _: () = assert!(size_of::<VarState>() == 4);

pub struct TypeEnv {
    entries: Vec<VarState>,
    bound: Vec<Type>,
    undo: Vec<(TypeVarId, VarState)>,
    speculation_depth: usize,
}

#[must_use]
pub(super) struct Speculation {
    depth: usize,
    undo_start: usize,
    bound_start: usize,
}

pub(super) enum SpeculationOutcome {
    Commit,
    Rollback,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            bound: Vec::new(),
            undo: Vec::new(),
            speculation_depth: 0,
        }
    }

    /// Allocate a fresh unbound variable and return its handle.
    pub(crate) fn fresh(&mut self) -> TypeVarId {
        let id = TypeVarId::new(self.entries.len() as u32);
        self.entries.push(None);
        id
    }

    fn slot(id: TypeVarId) -> usize {
        id.index() as usize
    }

    pub(crate) fn bind(&mut self, id: TypeVarId, ty: Type) {
        let slot = Self::slot(id);
        let index = u32::try_from(self.bound.len()).expect("too many type bindings");
        let binding = BindingSlot(
            NonZeroU32::new(index.checked_add(1).expect("too many type bindings"))
                .expect("binding indices start at one"),
        );
        self.bound.push(ty);
        let old = self.entries[slot].replace(binding);
        if self.speculation_depth != 0 {
            self.undo.push((id, old));
        }
    }

    fn binding(&self, id: TypeVarId) -> Option<&Type> {
        self.entries[Self::slot(id)].map(|slot| &self.bound[(slot.0.get() - 1) as usize])
    }

    /// Follow a `Type::Var` chain one step at a time until we reach either
    /// an unbound variable or a non-Var type.
    pub(crate) fn shallow_resolve(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        loop {
            match &current {
                Type::Var { id, .. } => match self.binding(*id) {
                    None => return current,
                    Some(bound) => current = bound.clone(),
                },
                _ => return current,
            }
        }
    }

    /// Deep resolve: chase `Type::Var` chains, substitute every bound var with
    /// its chased value, and recurse into composites. Unbound vars are preserved as-is.
    /// Used both during inference and as the post-inference freeze pass.
    pub(crate) fn resolve(&self, ty: &Type) -> Type {
        self.resolve_changed(ty).unwrap_or_else(|| ty.clone())
    }

    /// Resolve `ty` in place; skips the clone-and-rebuild when nothing is bound.
    /// Returns whether anything changed.
    pub(crate) fn resolve_in_place(&self, ty: &mut Type) -> bool {
        if let Some(resolved) = self.resolve_changed(ty) {
            *ty = resolved;
            true
        } else {
            false
        }
    }

    /// Returns `Some` only when resolving `ty` would change it (some bound var
    /// is reachable), allocating just the changed spine. `None` means unchanged.
    fn resolve_changed(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Var { id, .. } => match self.binding(*id) {
                None => None,
                Some(first) => {
                    let mut cursor = first;
                    loop {
                        match cursor {
                            Type::Var { id, .. } => match self.binding(*id) {
                                None => return Some(cursor.clone()),
                                Some(next) => cursor = next,
                            },
                            _ => return Some(self.resolve(cursor)),
                        }
                    }
                }
            },
            Type::Nominal {
                id,
                params,
                writable,
            } => self.resolve_slice(params).map(|params| Type::Nominal {
                id: id.clone(),
                params,
                writable: *writable,
            }),
            Type::Compound {
                kind,
                args,
                writable,
            } => self
                .resolve_slice(args)
                .map(|args| Type::qualified_compound(*kind, args, *writable)),
            Type::Function(f) => {
                let new_params = self.resolve_function_params(&f.params);
                let new_return = self.resolve_changed(&f.return_type).map(Box::new);
                let new_bounds = self.resolve_bounds(&f.bounds);
                if new_params.is_none() && new_return.is_none() && new_bounds.is_none() {
                    return None;
                }
                Some(f.rebuild(
                    new_params.unwrap_or_else(|| f.params.clone()),
                    new_bounds.unwrap_or_else(|| f.bounds.clone()),
                    new_return.unwrap_or_else(|| f.return_type.clone()),
                ))
            }
            Type::Forall { vars, body } => self.resolve_changed(body).map(|body| Type::Forall {
                vars: vars.clone(),
                body: Box::new(body),
            }),
            Type::Tuple(elements) => self.resolve_slice(elements).map(Type::Tuple),
            Type::Array { length, element } => {
                self.resolve_changed(element).map(|element| Type::Array {
                    length: *length,
                    element: Box::new(element),
                })
            }
            _ => None,
        }
    }

    /// [`resolve_changed`] over a slice; `Some` only if an element changed.
    fn resolve_slice(&self, items: &[Type]) -> Option<Vec<Type>> {
        let mut out: Option<Vec<Type>> = None;
        for (i, item) in items.iter().enumerate() {
            match self.resolve_changed(item) {
                Some(resolved) => {
                    out.get_or_insert_with(|| items[..i].to_vec())
                        .push(resolved);
                }
                None => {
                    if let Some(v) = out.as_mut() {
                        v.push(item.clone());
                    }
                }
            }
        }
        out
    }

    fn resolve_function_params(
        &self,
        params: &[FunctionParameter],
    ) -> Option<Vec<FunctionParameter>> {
        let mut out: Option<Vec<FunctionParameter>> = None;
        for (index, param) in params.iter().enumerate() {
            match self.resolve_changed(&param.ty) {
                Some(resolved) => {
                    out.get_or_insert_with(|| params[..index].to_vec())
                        .push(param.with_type(resolved));
                }
                None => {
                    if let Some(resolved) = out.as_mut() {
                        resolved.push(param.clone());
                    }
                }
            }
        }
        out
    }

    /// Bound-list variant of [`resolve_slice`].
    fn resolve_bounds(&self, bounds: &[Bound]) -> Option<Vec<Bound>> {
        let mut out: Option<Vec<Bound>> = None;
        for (i, b) in bounds.iter().enumerate() {
            let new_generic = self.resolve_changed(&b.generic);
            let new_ty = self.resolve_changed(&b.ty);
            if new_generic.is_none() && new_ty.is_none() {
                if let Some(v) = out.as_mut() {
                    v.push(b.clone());
                }
                continue;
            }
            out.get_or_insert_with(|| bounds[..i].to_vec()).push(Bound {
                param_name: b.param_name.clone(),
                generic: new_generic.unwrap_or_else(|| b.generic.clone()),
                ty: new_ty.unwrap_or_else(|| b.ty.clone()),
            });
        }
        out
    }

    /// Occurs check: does `id` appear anywhere inside `ty` (following Var
    /// chains but stopping at unbound Vars)?
    pub(crate) fn occurs(&self, id: TypeVarId, ty: &Type) -> bool {
        match ty {
            Type::Var { id: other, .. } => {
                if *other == id {
                    return true;
                }
                match self.binding(*other) {
                    None => false,
                    Some(bound) => self.occurs(id, bound),
                }
            }
            _ => ty
                .children()
                .into_iter()
                .any(|child| self.occurs(id, child)),
        }
    }

    pub(super) fn begin_speculation(&mut self) -> Speculation {
        self.speculation_depth += 1;
        Speculation {
            depth: self.speculation_depth,
            undo_start: self.undo.len(),
            bound_start: self.bound.len(),
        }
    }

    pub(super) fn end_speculation(
        &mut self,
        speculation: Speculation,
        outcome: SpeculationOutcome,
    ) {
        assert_eq!(
            speculation.depth, self.speculation_depth,
            "speculations must finish in nesting order"
        );
        self.speculation_depth -= 1;
        match outcome {
            SpeculationOutcome::Rollback => {
                for (id, original) in self.undo.drain(speculation.undo_start..).rev() {
                    self.entries[Self::slot(id)] = original;
                }
                self.bound.truncate(speculation.bound_start);
            }
            SpeculationOutcome::Commit => {
                if self.speculation_depth == 0 {
                    self.undo.clear();
                }
            }
        }
    }
}

/// Extension trait for `Type` giving env-aware resolve convenience methods.
/// Call-site sugar for `env.resolve(&ty)` written as `ty.resolve_in(&env)`.
pub trait EnvResolve {
    fn resolve_in(&self, env: &TypeEnv) -> Type;
    fn shallow_resolve_in(&self, env: &TypeEnv) -> Type;
}

impl EnvResolve for Type {
    fn resolve_in(&self, env: &TypeEnv) -> Type {
        env.resolve(self)
    }
    fn shallow_resolve_in(&self, env: &TypeEnv) -> Type {
        env.shallow_resolve(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::types::SimpleKind;

    fn var(id: TypeVarId) -> Type {
        Type::Var { id, hint: None }
    }

    #[test]
    fn resolve_follows_deep_var_chain_without_overflow() {
        let mut env = TypeEnv::new();
        const DEPTH: usize = 100_000;

        let ids: Vec<TypeVarId> = (0..DEPTH).map(|_| env.fresh()).collect();
        for pair in ids.windows(2) {
            env.bind(pair[0], var(pair[1]));
        }
        env.bind(ids[DEPTH - 1], Type::Simple(SimpleKind::Int));

        assert_eq!(env.resolve(&var(ids[0])), Type::Simple(SimpleKind::Int));
    }

    #[test]
    fn outer_rollback_includes_committed_nested_bindings() {
        let mut env = TypeEnv::new();
        let outer = env.fresh();
        let inner = env.fresh();

        let outer_speculation = env.begin_speculation();
        env.bind(outer, Type::Simple(SimpleKind::Int));
        let inner_speculation = env.begin_speculation();
        env.bind(inner, Type::Simple(SimpleKind::String));
        env.end_speculation(inner_speculation, SpeculationOutcome::Commit);
        env.end_speculation(outer_speculation, SpeculationOutcome::Rollback);

        assert_eq!(env.shallow_resolve(&var(outer)), var(outer));
        assert_eq!(env.shallow_resolve(&var(inner)), var(inner));
    }

    #[test]
    fn rollback_restores_rebindings_and_keeps_new_variables_usable() {
        let mut env = TypeEnv::new();
        let existing = env.fresh();
        env.bind(existing, Type::int());

        let outer = env.begin_speculation();
        env.bind(existing, Type::bool());
        let created = env.fresh();
        env.bind(created, Type::string());
        let inner = env.begin_speculation();
        env.bind(existing, Type::string());
        env.end_speculation(inner, SpeculationOutcome::Commit);
        env.bind(existing, Type::Never);
        env.end_speculation(outer, SpeculationOutcome::Rollback);

        assert_eq!(env.resolve(&var(existing)), Type::int());
        assert_eq!(env.resolve(&var(created)), var(created));
        env.bind(created, Type::bool());
        assert_eq!(env.resolve(&var(created)), Type::bool());
        assert_eq!(env.resolve(&var(existing)), Type::int());
    }

    #[test]
    fn inner_rollback_preserves_outer_writes_and_later_commit() {
        let mut env = TypeEnv::new();
        let id = env.fresh();
        env.bind(id, Type::int());
        let outer = env.begin_speculation();
        env.bind(id, Type::string());
        let inner = env.begin_speculation();
        env.bind(id, Type::bool());

        env.end_speculation(inner, SpeculationOutcome::Rollback);
        assert_eq!(env.resolve(&var(id)), Type::string());
        env.end_speculation(outer, SpeculationOutcome::Commit);

        let later = env.begin_speculation();
        env.bind(id, Type::Never);
        env.end_speculation(later, SpeculationOutcome::Rollback);
        assert_eq!(env.resolve(&var(id)), Type::string());
    }
}
