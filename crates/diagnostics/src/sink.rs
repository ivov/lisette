use std::cell::RefCell;
use std::collections::HashSet;

use syntax::ParseError;

use crate::LisetteDiagnostic;

#[derive(Debug, Default)]
pub struct LocalSink {
    diagnostics: RefCell<Vec<LisetteDiagnostic>>,
}

#[derive(Debug)]
pub struct DiagnosticCheckpoint(usize);

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

    pub fn any(&self, predicate: impl FnMut(&LisetteDiagnostic) -> bool) -> bool {
        self.diagnostics.borrow().iter().any(predicate)
    }

    pub fn error_label_points(&self) -> Vec<(u32, usize)> {
        self.diagnostics
            .borrow()
            .iter()
            .filter(|d| d.is_error())
            .flat_map(LisetteDiagnostic::label_points)
            .collect()
    }

    pub fn checkpoint(&self) -> DiagnosticCheckpoint {
        DiagnosticCheckpoint(self.diagnostics.borrow().len())
    }

    pub fn into_diagnostics(self) -> Vec<LisetteDiagnostic> {
        without_repeats(self.diagnostics.into_inner())
    }

    pub fn into_diagnostics_since(
        self,
        checkpoint: DiagnosticCheckpoint,
    ) -> (Vec<LisetteDiagnostic>, Vec<LisetteDiagnostic>) {
        let mut before = self.diagnostics.into_inner();
        let since = before.split_off(checkpoint.0);
        (without_repeats(before), without_repeats(since))
    }

    pub fn rollback(&self, checkpoint: DiagnosticCheckpoint) {
        self.diagnostics.borrow_mut().truncate(checkpoint.0);
    }

    pub fn drop_reports_repeated_since(&self, checkpoint: DiagnosticCheckpoint) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        let since = diagnostics.split_off(checkpoint.0);
        for diagnostic in since {
            if !diagnostics
                .iter()
                .any(|earlier| earlier.reports_same_as(&diagnostic))
            {
                diagnostics.push(diagnostic);
            }
        }
    }

    pub fn has_code_since(&self, checkpoint: DiagnosticCheckpoint, code: &str) -> bool {
        self.diagnostics.borrow()[checkpoint.0..]
            .iter()
            .any(|diagnostic| diagnostic.code_str() == Some(code))
    }

    pub fn has_changed_since(&self, checkpoint: DiagnosticCheckpoint) -> bool {
        self.diagnostics.borrow().len() != checkpoint.0
    }

    pub fn extend(&self, diagnostics: impl IntoIterator<Item = LisetteDiagnostic>) {
        self.diagnostics.borrow_mut().extend(diagnostics);
    }

    pub fn extend_parse_errors(&self, errors: Vec<ParseError>) {
        let diagnostics = errors.into_iter().map(LisetteDiagnostic::from);
        self.diagnostics.borrow_mut().extend(diagnostics);
    }

    pub fn merge(sinks: Vec<LocalSink>) -> Vec<LisetteDiagnostic> {
        let mut all: Vec<LisetteDiagnostic> = sinks
            .into_iter()
            .flat_map(LocalSink::into_diagnostics)
            .collect();
        all.sort_by(LisetteDiagnostic::sort_key);
        without_repeats(all)
    }
}

fn without_repeats(diagnostics: Vec<LisetteDiagnostic>) -> Vec<LisetteDiagnostic> {
    let is_first: Vec<bool> = {
        let mut seen = HashSet::with_capacity(diagnostics.len());
        diagnostics
            .iter()
            .map(|diagnostic| seen.insert(diagnostic))
            .collect()
    };
    diagnostics
        .into_iter()
        .zip(is_first)
        .filter_map(|(diagnostic, is_first)| is_first.then_some(diagnostic))
        .collect()
}
