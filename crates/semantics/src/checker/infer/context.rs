use std::ops::{Deref, DerefMut};

use crate::checker::TaskState;
use crate::store::Store;

pub struct InferCtx<'a> {
    state: &'a mut TaskState,
    pub(crate) store: &'a Store,
}

impl<'a> InferCtx<'a> {
    pub fn new(state: &'a mut TaskState, store: &'a Store) -> Self {
        Self { state, store }
    }

    pub(crate) fn with_scope<T>(
        &mut self,
        f: impl for<'scope> FnOnce(&mut InferCtx<'scope>) -> T,
    ) -> T {
        let store = self.store;
        self.state.with_scope(|state| {
            let mut context = InferCtx::new(state, store);
            f(&mut context)
        })
    }

    pub(crate) fn without_enclosing_loop<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let loops = self.scopes.take_loops();
        let result = f(self);
        self.scopes.restore_loops(loops);
        result
    }

    pub(crate) fn in_defer_block<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.increment_defer_block_depth();
        let result = self.without_enclosing_loop(f);
        self.scopes.decrement_defer_block_depth();
        result
    }

    pub(crate) fn in_negation<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.increment_negation_depth();
        let result = f(self);
        self.scopes.decrement_negation_depth();
        result
    }

    pub(crate) fn with_temporary_bindings<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let checkpoint = self.facts.binding_checkpoint();
        let result = f(self);
        self.facts.remove_bindings_from(checkpoint);
        result
    }
}

impl Deref for InferCtx<'_> {
    type Target = TaskState;

    fn deref(&self) -> &Self::Target {
        self.state
    }
}

impl DerefMut for InferCtx<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
    }
}
