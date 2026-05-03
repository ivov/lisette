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
        match ty {
            Type::Var { id, .. } if !id.is_reserved() => match &self.entries[Self::slot(*id)] {
                VarState::Unbound { .. } => ty.clone(),
                VarState::Bound(bound) => self.resolve(bound),
            },
            Type::Nominal {
                id,
                params,
                underlying_ty,
            } => Type::Nominal {
                id: id.clone(),
                params: params.iter().map(|p| self.resolve(p)).collect(),
                underlying_ty: underlying_ty.as_ref().map(|u| Box::new(self.resolve(u))),
            },
            Type::Compound { kind, args } => Type::Compound {
                kind: *kind,
                args: args.iter().map(|a| self.resolve(a)).collect(),
            },
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| self.resolve(p)).collect(),
                param_mutability: param_mutability.clone(),
                bounds: bounds
                    .iter()
                    .map(|b| Bound {
                        param_name: b.param_name.clone(),
                        generic: self.resolve(&b.generic),
                        ty: self.resolve(&b.ty),
                    })
                    .collect(),
                return_type: Box::new(self.resolve(return_type)),
            },
            Type::Forall { vars, body } => Type::Forall {
                vars: vars.clone(),
                body: Box::new(self.resolve(body)),
            },
            Type::Tuple(elements) => {
                Type::Tuple(elements.iter().map(|e| self.resolve(e)).collect())
            }
            _ => ty.clone(),
        }
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
            Type::Function {
                params,
                return_type,
                ..
            } => params.iter().any(|p| self.occurs(id, p)) || self.occurs(id, return_type),
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
