pub(crate) mod bounds;
mod clone;
pub(crate) mod comma_ok;
pub(crate) mod dispatch;
pub(crate) mod go_interop;
pub(crate) mod native;
pub(crate) mod predicates;
mod regular;
pub(crate) mod slice_loop;
mod ufcs;
mod unwrap_or;
pub(crate) mod wrap_err;

use crate::Planner;
use crate::calls::dispatch::extract_native_method_name;
use crate::plan::calls::CallableOrigin;
use crate::plan::values::CaptureBoundary;
use crate::types::native::NativeGoType;
use syntax::ast::{Expression, ResolvedCallTypeArguments};
use syntax::program::NativeTypeKind;
use syntax::types::Type;

pub(crate) struct NativeMethodCall<'a> {
    pub kind: NativeTypeKind,
    pub method: &'a str,
    pub function: &'a Expression,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub resolved_type_args: ResolvedCallTypeArguments<'a>,
    pub receiver: &'a Expression,
    pub arguments: &'a [Expression],
}

impl Planner<'_> {
    pub(crate) fn native_method_call<'a>(
        &self,
        value: &'a Expression,
    ) -> Option<NativeMethodCall<'a>> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            type_arguments,
            ..
        } = value.unwrap_parens()
        else {
            return None;
        };
        let plan = self.plan_call(value.unwrap_parens())?;
        let (CallableOrigin::NativeMethod(kind) | CallableOrigin::NativeMethodIdentifier(kind)) =
            &plan.resolved.origin
        else {
            return None;
        };
        let function = callee.unwrap_parens();
        let (receiver, arguments) = match function {
            Expression::DotAccess { expression, .. } => (expression.as_ref(), args.as_slice()),
            _ => args.split_first()?,
        };
        Some(NativeMethodCall {
            kind: *kind,
            method: extract_native_method_name(function),
            function,
            args,
            spread: spread.as_deref(),
            resolved_type_args: type_arguments.resolved_types()?,
            receiver,
            arguments,
        })
    }
}

pub(super) struct NativeCallContext<'a> {
    pub function: &'a Expression,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub resolved_type_args: ResolvedCallTypeArguments<'a>,
    pub call_ty: Option<&'a Type>,
    pub native_type: &'a NativeGoType,
    pub method: &'a str,
    pub capture_boundary: CaptureBoundary,
    pub retired_receiver: Option<&'a Expression>,
    pub result_name: Option<&'a str>,
}
