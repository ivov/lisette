use std::cell::RefCell;

use syntax::ParseError;

use crate::LisetteDiagnostic;

#[derive(Debug, Default)]
pub struct LocalSink {
    diagnostics: RefCell<Vec<LisetteDiagnostic>>,
}

impl LocalSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, diagnostic: LisetteDiagnostic) {
        self.diagnostics.borrow_mut().push(diagnostic);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.borrow().iter().any(|d| d.is_error())
    }

    pub fn len(&self) -> usize {
        self.diagnostics.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.borrow().is_empty()
    }

    pub fn to_vec(&self) -> Vec<LisetteDiagnostic> {
        self.diagnostics.borrow().clone()
    }

    pub fn take(&self) -> Vec<LisetteDiagnostic> {
        self.diagnostics.take()
    }

    pub fn truncate(&self, len: usize) {
        self.diagnostics.borrow_mut().truncate(len);
    }

    pub fn extend(&self, diagnostics: impl IntoIterator<Item = LisetteDiagnostic>) {
        self.diagnostics.borrow_mut().extend(diagnostics);
    }

    pub fn extend_parse_errors(&self, errors: Vec<ParseError>) {
        let diagnostics = errors.into_iter().map(LisetteDiagnostic::from);
        self.diagnostics.borrow_mut().extend(diagnostics);
    }

    pub fn merge(sinks: Vec<LocalSink>) -> Vec<LisetteDiagnostic> {
        let mut all: Vec<LisetteDiagnostic> = sinks.into_iter().flat_map(|s| s.take()).collect();
        all.sort_by_key(|d| d.primary_offset());
        all
    }
}
