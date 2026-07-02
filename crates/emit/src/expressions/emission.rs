use syntax::ast::Expression;

use crate::plan::bodies::LoweredStatement;
use crate::plan::values::ValuePlan;
use crate::utils::{contains_call, observable_after_mutation};

/// Whether an inline-valued emission needs to be captured to a temp when a
/// later sibling produces setup statements or contains a call. Setup-bearing
/// emissions are already pinned and use `Never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapturePolicy {
    Never,
    IfLaterEffect,
}

/// Result of staging a sub-expression to a separate buffer.
pub(crate) struct StagedExpression {
    pub setup: Vec<LoweredStatement>,
    pub value: String,
    pub capture: CapturePolicy,
    pub non_literal: bool,
    pub has_call: bool,
    /// `has_call` refined down by the planner for pure calls.
    pub has_effectful_call: bool,
    /// A value no later sibling call can invalidate. Planner-computed,
    /// since it depends on the lowered value.
    pub call_pin_exempt: bool,
}

impl StagedExpression {
    /// Derive the capture policy and observability flags from `expression`,
    /// given already-structured setup and value text. The single place those
    /// fields are computed.
    fn build(setup: Vec<LoweredStatement>, value: String, expression: &Expression) -> Self {
        let non_literal = observable_after_mutation(expression);
        let capture = if setup.is_empty() && non_literal {
            CapturePolicy::IfLaterEffect
        } else {
            CapturePolicy::Never
        };
        let has_call = contains_call(expression);
        Self {
            setup,
            value,
            capture,
            non_literal,
            has_call,
            has_effectful_call: has_call,
            call_pin_exempt: false,
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
