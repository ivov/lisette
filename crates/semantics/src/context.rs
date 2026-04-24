//! Shared immutable view of the program after registration finishes.
//! Inference reads through this handle; it cannot mutate.
//!
//! Invariant: only `&` references. No `RefCell`, `Cell`, `Rc`, or owned
//! mutable state. The `analysis_context_is_send_sync` test below
//! enforces this at compile time.

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

#[cfg(test)]
mod tests {
    use super::*;
    use diagnostics::LocalSink;

    #[test]
    fn analysis_context_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AnalysisContext<'_>>();
        assert_send_sync::<&Store>();
    }

    #[test]
    fn local_sink_is_send_not_sync() {
        fn assert_send<T: Send>() {}
        assert_send::<LocalSink>();
    }
}
