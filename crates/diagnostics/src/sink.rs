use std::cell::RefCell;

use syntax::ParseError;

use crate::LisetteDiagnostic;

#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diagnostics: RefCell<Vec<LisetteDiagnostic>>,
}

impl DiagnosticSink {
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

    pub fn extend(&self, diagnostics: impl IntoIterator<Item = LisetteDiagnostic>) {
        self.diagnostics.borrow_mut().extend(diagnostics);
    }

    pub fn extend_parse_errors(&self, errors: Vec<ParseError>) {
        let diagnostics = errors.into_iter().map(LisetteDiagnostic::from);
        self.diagnostics.borrow_mut().extend(diagnostics);
    }
}
