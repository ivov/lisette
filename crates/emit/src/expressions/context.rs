use syntax::types::Type;

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
pub(crate) enum GoArrayReturnPolicy {
    #[default]
    WrapArrayReturn,
    KeepRawArrayReturn,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GoFunctionValuePolicy {
    #[default]
    AllowLoweredIdentity,
    ForceTaggedWrapper,
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
    go_array_return_policy: GoArrayReturnPolicy,
    go_function_value_policy: GoFunctionValuePolicy,
    argument_target: ArgumentTarget,
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

    pub(crate) fn with_raw_go_array_return(mut self) -> Self {
        self.go_array_return_policy = GoArrayReturnPolicy::KeepRawArrayReturn;
        self
    }

    pub(crate) fn with_forced_tagged_go_function(self, force: bool) -> Self {
        if force {
            Self {
                go_function_value_policy: GoFunctionValuePolicy::ForceTaggedWrapper,
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
            self.go_function_value_policy,
            GoFunctionValuePolicy::ForceTaggedWrapper
        )
    }

    pub(crate) fn keeps_raw_go_array_return(self) -> bool {
        matches!(
            self.go_array_return_policy,
            GoArrayReturnPolicy::KeepRawArrayReturn
        )
    }

    pub(crate) fn argument_flows_to_unknown(self) -> bool {
        matches!(self.argument_target, ArgumentTarget::Unknown)
    }
}
