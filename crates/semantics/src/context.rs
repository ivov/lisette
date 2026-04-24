//! Analysis context — the shared immutable view of the program after
//! registration has finished.
//!
//! Invariant: every field is a shared reference. No interior mutability.
//! This is what will eventually become `Send + Sync` to permit parallel
//! inference workers to read through a single context. Do not add
//! `RefCell`, `Cell`, or owned mutable state here.

use rustc_hash::FxHashSet as HashSet;

use crate::store::Store;

pub struct AnalysisContext<'r> {
    pub store: &'r Store,
    pub ufcs_methods: &'r HashSet<(String, String)>,
}

impl<'r> AnalysisContext<'r> {
    pub fn new(store: &'r Store, ufcs_methods: &'r HashSet<(String, String)>) -> Self {
        Self {
            store,
            ufcs_methods,
        }
    }
}
