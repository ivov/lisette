use std::ops::{Deref, DerefMut};

use crate::checker::TaskState;
use crate::store::Store;

pub struct InferCtx<'a, 's> {
    state: &'a mut TaskState<'s>,
    pub(crate) store: &'a Store,
}

impl<'a, 's> InferCtx<'a, 's> {
    pub fn new(state: &'a mut TaskState<'s>, store: &'a Store) -> Self {
        Self { state, store }
    }
}

impl<'s> Deref for InferCtx<'_, 's> {
    type Target = TaskState<'s>;

    fn deref(&self) -> &Self::Target {
        self.state
    }
}

impl DerefMut for InferCtx<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
    }
}
