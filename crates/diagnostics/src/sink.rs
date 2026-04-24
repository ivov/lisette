use std::sync::Mutex;

use syntax::ParseError;

use crate::LisetteDiagnostic;

#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diagnostics: Mutex<Vec<LisetteDiagnostic>>,
}

impl DiagnosticSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, diagnostic: LisetteDiagnostic) {
        self.diagnostics.lock().unwrap().push(diagnostic);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .lock()
            .unwrap()
            .iter()
            .any(|d| d.is_error())
    }

    pub fn len(&self) -> usize {
        self.diagnostics.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.lock().unwrap().is_empty()
    }

    pub fn to_vec(&self) -> Vec<LisetteDiagnostic> {
        self.diagnostics.lock().unwrap().clone()
    }

    pub fn take(&self) -> Vec<LisetteDiagnostic> {
        std::mem::take(&mut *self.diagnostics.lock().unwrap())
    }

    pub fn truncate(&self, len: usize) {
        self.diagnostics.lock().unwrap().truncate(len);
    }

    pub fn extend(&self, diagnostics: impl IntoIterator<Item = LisetteDiagnostic>) {
        self.diagnostics.lock().unwrap().extend(diagnostics);
    }

    pub fn extend_parse_errors(&self, errors: Vec<ParseError>) {
        let diagnostics = errors.into_iter().map(LisetteDiagnostic::from);
        self.diagnostics.lock().unwrap().extend(diagnostics);
    }
}
