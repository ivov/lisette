//! Union-find-style binding table for `Type::Var` handles.
//!
//! `Type::Var(TypeVarId)` is a handle; the binding (Unbound vs Bound-to-a-Type)
//! lives here in `entries`, indexed by id. Cloning a `Type` clones just the
//! handle, so `Type` is a pure value (Clone / Eq / Hash / Serialize friendly)
//! with no shared mutable state.
//!
//! Speculative unification works through the `undo_log`: a fresh log is
//! pushed when entering speculation, bindings are recorded as they happen,
//! and on Err the originals are restored in reverse order. On Ok the log is
//! either discarded (no enclosing speculation) or appended to the parent log
//! (nested speculation — the bindings are committed to the parent, but still
//! reversible if the parent fails).

use ecow::EcoString;
use syntax::types::{Bound, Type, TypeVarId};

#[derive(Debug, Clone)]
pub enum VarState {
    Unbound { hint: Option<EcoString> },
    Bound(Type),
}

pub struct TypeEnv {
    entries: Vec<VarState>,
    /// When Some, bindings performed during the current speculative region
    /// are logged here as `(id, prior_state)` so they can be reverted.
    undo_log: Option<Vec<(TypeVarId, VarState)>>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            undo_log: None,
        }
    }

    /// Allocate a fresh unbound variable and return its handle.
    pub fn fresh(&mut self, hint: Option<EcoString>) -> TypeVarId {
        let id = TypeVarId(self.entries.len() as u32);
        self.entries.push(VarState::Unbound { hint });
        id
    }

    fn slot(id: TypeVarId) -> usize {
        debug_assert!(
            !id.is_reserved(),
            "TypeEnv should not be queried for reserved ids"
        );
        id.0 as usize
    }

    pub fn state(&self, id: TypeVarId) -> &VarState {
        &self.entries[Self::slot(id)]
    }

    pub fn is_unbound(&self, id: TypeVarId) -> bool {
        if id.is_reserved() {
            return true;
        }
        matches!(self.entries[Self::slot(id)], VarState::Unbound { .. })
    }

    /// Bind `id` to `ty`. Reserved sentinel ids (ignored/uninferred) are
    /// silently accepted: they unify with anything without storing anything.
    pub fn bind(&mut self, id: TypeVarId, ty: Type) {
        if id.is_reserved() {
            return;
        }
        let slot = Self::slot(id);
        let old = std::mem::replace(&mut self.entries[slot], VarState::Bound(ty));
        if let Some(log) = &mut self.undo_log {
            log.push((id, old));
        }
    }

    /// Follow a `Type::Var` chain one step at a time until we reach either
    /// an unbound variable or a non-Var type.
    pub fn shallow_resolve(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        loop {
            match &current {
                Type::Var { id, .. } if !id.is_reserved() => match &self.entries[Self::slot(*id)] {
                    VarState::Unbound { .. } => return current,
                    VarState::Bound(bound) => current = bound.clone(),
                },
                _ => return current,
            }
        }
    }

    /// Deep resolve: chase `Type::Var` chains, substitute every bound var with
    /// its chased value, and recurse into composites. Unbound vars (including
    /// reserved sentinel ids like `IGNORED`/`UNINFERRED`) are preserved as-is.
    /// Used both during inference and as the post-inference freeze pass.
    pub fn resolve(&self, ty: &Type) -> Type {
        self.resolve_changed(ty).unwrap_or_else(|| ty.clone())
    }

    /// Resolve `ty` in place; skips the clone-and-rebuild when nothing is bound.
    /// Returns whether anything changed.
    pub fn resolve_in_place(&self, ty: &mut Type) -> bool {
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
            Type::Var { id, .. } if !id.is_reserved() => match &self.entries[Self::slot(*id)] {
                VarState::Unbound { .. } => None,
                VarState::Bound(bound) => Some(self.resolve(bound)),
            },
            Type::Nominal {
                id,
                params,
                underlying_ty,
            } => {
                let new_params = self.resolve_slice(params);
                let new_underlying = underlying_ty
                    .as_ref()
                    .and_then(|u| self.resolve_changed(u).map(Box::new));
                if new_params.is_none() && new_underlying.is_none() {
                    return None;
                }
                Some(Type::Nominal {
                    id: id.clone(),
                    params: new_params.unwrap_or_else(|| params.clone()),
                    underlying_ty: new_underlying
                        .map(Some)
                        .unwrap_or_else(|| underlying_ty.clone()),
                })
            }
            Type::Compound { kind, args } => self
                .resolve_slice(args)
                .map(|args| Type::Compound { kind: *kind, args }),
            Type::Function(f) => {
                let new_params = self.resolve_slice(&f.params);
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
    pub fn occurs(&self, id: TypeVarId, ty: &Type) -> bool {
        match ty {
            Type::Var { id: other, .. } => {
                if *other == id {
                    return true;
                }
                if other.is_reserved() {
                    return false;
                }
                match &self.entries[Self::slot(*other)] {
                    VarState::Unbound { .. } => false,
                    VarState::Bound(bound) => self.occurs(id, bound),
                }
            }
            Type::Nominal { params, .. } => params.iter().any(|p| self.occurs(id, p)),
            Type::Compound { args, .. } => args.iter().any(|a| self.occurs(id, a)),
            Type::Function(f) => {
                f.params.iter().any(|p| self.occurs(id, p)) || self.occurs(id, &f.return_type)
            }
            Type::Forall { body, .. } => self.occurs(id, body),
            Type::Tuple(elements) => elements.iter().any(|e| self.occurs(id, e)),
            _ => false,
        }
    }

    /// Begin a speculative region. Caller holds the returned handle and
    /// passes it back to `end_speculation` with the region's outcome.
    pub fn begin_speculation(&mut self) -> Speculation {
        let prev = self.undo_log.take();
        self.undo_log = Some(Vec::new());
        Speculation { prev }
    }

    /// End a speculative region. If `is_err`, revert all bindings made
    /// during the region. Otherwise, either commit them (no enclosing
    /// region) or append them to the enclosing region's log (so it can
    /// still revert them if it fails).
    pub fn end_speculation(&mut self, spec: Speculation, is_err: bool) {
        let log = self.undo_log.take().expect("speculation log must exist");
        self.undo_log = spec.prev;
        if is_err {
            for (id, original) in log.into_iter().rev() {
                self.entries[Self::slot(id)] = original;
            }
        } else if let Some(parent_log) = &mut self.undo_log {
            parent_log.extend(log);
        }
    }
}

/// Handle returned by `begin_speculation`, consumed by `end_speculation`.
/// Not clonable — ensures each region is ended exactly once.
#[must_use]
pub struct Speculation {
    prev: Option<Vec<(TypeVarId, VarState)>>,
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
