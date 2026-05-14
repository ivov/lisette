use syntax::ast::Expression;

use crate::utils::observable_after_mutation;

/// Whether an inline-valued emission needs to be captured to a temp when a
/// later sibling produces setup statements. Setup-bearing emissions are
/// already pinned and use `Never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapturePolicy {
    Never,
    IfLaterSetup,
}

/// Result of emitting a sub-expression to a separate buffer.
pub(crate) struct EmittedExpression {
    pub setup: String,
    pub value: String,
    pub capture: CapturePolicy,
}

impl EmittedExpression {
    /// Construct from raw setup + value, deriving the capture policy from
    /// the source expression (inline values that are observable after
    /// mutation require capture before any later sibling's setup runs).
    pub(crate) fn new(setup: String, value: String, expression: &Expression) -> Self {
        let capture = if setup.is_empty() && observable_after_mutation(expression) {
            CapturePolicy::IfLaterSetup
        } else {
            CapturePolicy::Never
        };
        Self {
            setup,
            value,
            capture,
        }
    }

    /// Inline value with no setup. Caller has verified the value does not
    /// need capture (e.g. literal, identifier, or already-temp).
    #[allow(dead_code)]
    pub(crate) fn inline(value: String) -> Self {
        Self {
            setup: String::new(),
            value,
            capture: CapturePolicy::Never,
        }
    }

    /// Value already pinned by `setup` (typically a temp binding). Capture
    /// is never needed because setup is what isolates the value.
    #[allow(dead_code)]
    pub(crate) fn with_setup(setup: String, value: String) -> Self {
        Self {
            setup,
            value,
            capture: CapturePolicy::Never,
        }
    }

    /// Append `setup` to `output` and return the value. Convenience for
    /// single-expression sites that do not need the sibling-aware
    /// `sequence` machinery.
    #[allow(dead_code)]
    pub(crate) fn write_setup_into(self, output: &mut String) -> String {
        output.push_str(&self.setup);
        self.value
    }
}
