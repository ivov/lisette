use syntax::ast::Expression;
use syntax::types::Type;

use crate::plan::values::CaptureBoundary;

/// Whether the expression is being emitted as the callee of a call.
/// Independent of [`SyntaxContext`]: a callee can appear inside a condition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CalleeRole {
    #[default]
    Value,
    Callee,
}

/// The enclosing syntactic context. `Condition` is `if`/`for`/`switch` head,
/// where Go forbids unparenthesized composite literals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SyntaxContext {
    #[default]
    Plain,
    Condition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FunctionValueAbiTarget {
    #[default]
    Natural,
    Tagged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArgumentTarget {
    #[default]
    Typed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ExpressionContext<'a> {
    callee_role: CalleeRole,
    syntax_context: SyntaxContext,
    expected_slot_type: Option<&'a Type>,
    function_value_abi_target: FunctionValueAbiTarget,
    argument_target: ArgumentTarget,
    capture_boundary: CaptureBoundary,
    retired_receiver: Option<&'a Expression>,
}

impl<'a> ExpressionContext<'a> {
    pub(crate) fn value() -> Self {
        Self::default()
    }

    pub(crate) fn callee(mut self) -> Self {
        self.callee_role = CalleeRole::Callee;
        self
    }

    pub(crate) fn condition(mut self) -> Self {
        self.syntax_context = SyntaxContext::Condition;
        self
    }

    pub(crate) fn with_expected_slot_type(mut self, ty: Option<&'a Type>) -> Self {
        self.expected_slot_type = ty;
        self
    }

    pub(crate) fn with_forced_tagged_go_function(self, force: bool) -> Self {
        if force {
            Self {
                function_value_abi_target: FunctionValueAbiTarget::Tagged,
                ..self
            }
        } else {
            self
        }
    }

    pub(crate) fn with_unknown_argument_target(self, flows: bool) -> Self {
        if flows {
            Self {
                argument_target: ArgumentTarget::Unknown,
                ..self
            }
        } else {
            self
        }
    }

    pub(crate) fn with_capture_boundary(mut self, boundary: CaptureBoundary) -> Self {
        self.capture_boundary = boundary;
        self
    }

    pub(crate) fn expected_slot_type(self) -> Option<&'a Type> {
        self.expected_slot_type
    }

    pub(crate) fn is_callee(self) -> bool {
        matches!(self.callee_role, CalleeRole::Callee)
    }

    pub(crate) fn is_condition(self) -> bool {
        matches!(self.syntax_context, SyntaxContext::Condition)
    }

    pub(crate) fn forces_tagged_go_function(self) -> bool {
        matches!(
            self.function_value_abi_target,
            FunctionValueAbiTarget::Tagged
        )
    }

    pub(crate) fn argument_flows_to_unknown(self) -> bool {
        matches!(self.argument_target, ArgumentTarget::Unknown)
    }

    pub(crate) fn capture_boundary(self) -> CaptureBoundary {
        self.capture_boundary
    }

    pub(crate) fn with_retired_receiver(mut self, target: &'a Expression) -> Self {
        self.retired_receiver = Some(target);
        self
    }

    pub(crate) fn retired_receiver(self) -> Option<&'a Expression> {
        self.retired_receiver
    }
}
