use rustc_hash::FxHashSet;
use std::cell::RefCell;
use syntax::ast::Span;
use syntax::program::ValueUseKey;

#[derive(Clone, Default)]
pub(crate) struct ConversionRecord {
    applied_conversions: RefCell<FxHashSet<ValueUseKey>>,
    dropped_code: RefCell<Vec<Span>>,
}

impl ConversionRecord {
    pub(crate) fn record_applied_conversion(&self, key: ValueUseKey) {
        if cfg!(debug_assertions) {
            self.applied_conversions.borrow_mut().insert(key);
        }
    }

    pub(crate) fn record_dropped_code(&self, span: Span) {
        if cfg!(debug_assertions) {
            self.dropped_code.borrow_mut().push(span);
        }
    }

    pub(crate) fn recorded(&mut self) -> (&FxHashSet<ValueUseKey>, &[Span]) {
        (
            self.applied_conversions.get_mut(),
            self.dropped_code.get_mut(),
        )
    }
}
