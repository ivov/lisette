use syntax::ParseError;
use syntax::program::EmitInput;

use crate::LisetteDiagnostic;

pub struct SemanticResult {
    pub emit_input: EmitInput,
    diagnostics: Vec<LisetteDiagnostic>,
}

impl SemanticResult {
    pub fn new(emit_input: EmitInput, diagnostics: Vec<LisetteDiagnostic>) -> Self {
        let (mut errors, lints): (Vec<_>, Vec<_>) = diagnostics
            .into_iter()
            .partition(LisetteDiagnostic::is_error);
        errors.extend(lints);
        Self {
            emit_input,
            diagnostics: errors,
        }
    }

    pub fn with_parse_errors(errors: Vec<ParseError>, entry_module_id: &str) -> Self {
        Self::new(
            EmitInput {
                entry_module_id: entry_module_id.to_string(),
                ..EmitInput::default()
            },
            errors.into_iter().map(Into::into).collect(),
        )
    }

    pub fn diagnostics(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics
    }

    pub fn errors(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics[..self.error_count()]
    }

    pub fn lints(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics[self.error_count()..]
    }

    pub fn prepend_errors(&mut self, errors: impl IntoIterator<Item = LisetteDiagnostic>) {
        let errors: Vec<_> = errors.into_iter().collect();
        assert!(errors.iter().all(LisetteDiagnostic::is_error));
        self.diagnostics.splice(0..0, errors);
    }

    pub fn push_error(&mut self, error: LisetteDiagnostic) {
        assert!(error.is_error());
        let index = self.error_count();
        self.diagnostics.insert(index, error);
    }

    fn error_count(&self) -> usize {
        self.diagnostics
            .partition_point(LisetteDiagnostic::is_error)
    }

    pub fn failed(&self) -> bool {
        !self.errors().is_empty()
    }

    pub fn into_emit_input(self) -> EmitInput {
        self.emit_input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_classifies_diagnostics_by_severity() {
        let result = SemanticResult::new(
            EmitInput::default(),
            vec![
                LisetteDiagnostic::warn("warning"),
                LisetteDiagnostic::error("error"),
                LisetteDiagnostic::info("info"),
            ],
        );

        let messages = (
            result
                .errors()
                .iter()
                .map(LisetteDiagnostic::plain_message)
                .collect::<Vec<_>>(),
            result
                .lints()
                .iter()
                .map(LisetteDiagnostic::plain_message)
                .collect::<Vec<_>>(),
        );
        assert_eq!(messages, (vec!["error"], vec!["warning", "info"]));
    }

    #[test]
    fn injected_errors_preserve_the_single_severity_boundary() {
        let mut result = SemanticResult::new(
            EmitInput::default(),
            vec![
                LisetteDiagnostic::warn("warning"),
                LisetteDiagnostic::error("semantic"),
            ],
        );

        result.prepend_errors([LisetteDiagnostic::error("parse")]);
        result.push_error(LisetteDiagnostic::error("manifest"));

        let messages = result
            .diagnostics()
            .iter()
            .map(LisetteDiagnostic::plain_message)
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["parse", "semantic", "manifest", "warning"]);
    }
}
