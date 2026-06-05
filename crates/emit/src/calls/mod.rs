pub(crate) mod dispatch;
pub(crate) mod go_interop;
mod native;
mod regular;
mod ufcs;

use crate::GoCallStrategy;
use crate::Planner;
use crate::abi::AbiShape;
use crate::plan::calls::{CallReturnShape, CalleePlan};
use crate::types::native::NativeGoType;
use syntax::ast::{Annotation, Expression};
use syntax::types::Type;

/// How a call's result must be adapted at the call site. The cases are mutually
/// exclusive: a Go-interop call has a Go receiver, which `CalleePlan::Regular`
/// excludes, so a call is at most one of these. Derived from `CallPlan` —
/// renderers should consume the plan directly where possible.
pub(crate) enum CallBoundary {
    /// Go-interop call wrapped per its strategy (multi-return, nullable, ...).
    GoWrapped(GoCallStrategy),
    /// Lisette call whose callee returns a lowered ABI shape.
    LoweredCallee(AbiShape),
    /// Plain call rendered as-is.
    Plain,
}

impl Planner<'_> {
    pub(crate) fn classify_call(&self, call_expression: &Expression) -> CallBoundary {
        let Some(plan) = self.plan_call(call_expression) else {
            return CallBoundary::Plain;
        };
        match plan.callee {
            CalleePlan::GoInterop(strategy) => CallBoundary::GoWrapped(strategy),
            _ => match plan.return_shape {
                CallReturnShape::Lowered(shape) => CallBoundary::LoweredCallee(shape),
                CallReturnShape::Direct => CallBoundary::Plain,
            },
        }
    }
}

pub(super) struct NativeCallContext<'a> {
    pub function: &'a Expression,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub type_args: &'a [Annotation],
    pub call_ty: Option<&'a Type>,
    pub native_type: &'a NativeGoType,
    pub method: &'a str,
}
