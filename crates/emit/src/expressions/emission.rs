use syntax::ast::Expression;

use crate::Renderer;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{ValuePlan, setup_from_string};
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
    /// From raw setup + value; derives capture policy from `expression`.
    pub(crate) fn new(setup: String, value: String, expression: &Expression) -> Self {
        let capture = if setup.is_empty() && observable_after_mutation(expression) {
            CapturePolicy::IfLaterSetup
        } else {
            CapturePolicy::Never
        };
        Self {
            setup: setup_from_string(setup),
            value,
            capture,
            non_literal: observable_after_mutation(expression),
        }
    }

    /// From typed setup + value, for sites that built their setup directly.
    pub(crate) fn from_typed_setup(
        setup: Vec<LoweredStatement>,
        value: String,
        expression: &Expression,
    ) -> Self {
        let capture = if setup.is_empty() && observable_after_mutation(expression) {
            CapturePolicy::IfLaterSetup
        } else {
            CapturePolicy::Never
        };
        Self {
            setup,
            value,
            capture,
            non_literal: observable_after_mutation(expression),
        }
    }

    /// From a planned value, preserving its typed setup.
    pub(crate) fn from_plan(plan: ValuePlan, expression: &Expression) -> Self {
        match plan {
            ValuePlan::Composite { setup, value } => Self {
                setup,
                value,
                capture: CapturePolicy::Never,
                non_literal: observable_after_mutation(expression),
            },
            ValuePlan::Operand(value) => {
                let capture = if observable_after_mutation(expression) {
                    CapturePolicy::IfLaterSetup
                } else {
                    CapturePolicy::Never
                };
                Self {
                    setup: Vec::new(),
                    value,
                    capture,
                    non_literal: observable_after_mutation(expression),
                }
            }
            other => {
                let mut setup = String::new();
                let value = Renderer.render_value(&mut setup, &other);
                Self::new(setup, value, expression)
            }
        }
    }
}
