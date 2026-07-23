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
