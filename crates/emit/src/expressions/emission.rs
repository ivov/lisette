use syntax::ast::Expression;

use crate::plan::bodies::LoweredStatement;
use crate::plan::values::ValuePlan;
use crate::utils::observable_after_mutation;

/// Whether an inline-valued emission needs to be captured to a temp when a
/// later sibling produces setup statements. Setup-bearing emissions are
/// already pinned and use `Never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapturePolicy {
    Never,
    IfLaterSetup,
}

/// Result of staging a sub-expression to a separate buffer.
pub(crate) struct StagedExpression {
    pub setup: Vec<LoweredStatement>,
    pub value: String,
    pub capture: CapturePolicy,
    pub non_literal: bool,
}

impl StagedExpression {
    /// Derive the capture policy and observability flag from `expression`,
    /// given already-structured setup and value text. The single place those
    /// two fields are computed.
    fn build(setup: Vec<LoweredStatement>, value: String, expression: &Expression) -> Self {
        let non_literal = observable_after_mutation(expression);
        let capture = if setup.is_empty() && non_literal {
            CapturePolicy::IfLaterSetup
        } else {
            CapturePolicy::Never
        };
        Self {
            setup,
            value,
            capture,
            non_literal,
        }
    }

    /// From typed setup + value, for sites that built their setup directly.
    pub(crate) fn from_typed_setup(
        setup: Vec<LoweredStatement>,
        value: String,
        expression: &Expression,
    ) -> Self {
        Self::build(setup, value, expression)
    }

    /// From a planned value, preserving its typed setup.
    pub(crate) fn from_plan(plan: ValuePlan, expression: &Expression) -> Self {
        let (setup, value) = plan.into_parts();
        Self::build(setup, value, expression)
    }
}
