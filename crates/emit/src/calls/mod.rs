mod arguments;
pub(crate) mod bound_value;
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
pub(crate) mod unwrap_or;
pub(crate) mod wrap_err;

use crate::Planner;
use crate::abi::callable::CallableAbi;
use crate::calls::dispatch::extract_native_method_name;
use crate::plan::calls::CallableOrigin;
use crate::plan::values::CaptureBoundary;
use syntax::ast::{Expression, ResolvedCallTypeArguments};
use syntax::program::{CallKind, NativeTypeKind};
use syntax::types::Type;

pub(crate) struct NativeMethodCall<'a> {
    pub kind: NativeTypeKind,
    pub method: &'a str,
    pub function: &'a Expression,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub resolved_type_args: ResolvedCallTypeArguments<'a>,
    pub abi: CallableAbi,
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
        let CallableOrigin::Source(
            CallKind::NativeMethod(kind) | CallKind::NativeMethodIdentifier(kind),
        ) = &plan.resolved.origin
        else {
            return None;
        };
        let function = callee.unwrap_parens();
        let (receiver, arguments) = split_native_receiver(function, args)?;
        Some(NativeMethodCall {
            kind: *kind,
            method: extract_native_method_name(function),
            function,
            args,
            spread: spread.as_deref(),
            resolved_type_args: type_arguments.resolved_types()?,
            abi: plan.resolved.abi,
            receiver,
            arguments,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) enum NativeCallForm<'a> {
    /// `Map.new()`, `Channel.buffered(n)`
    Constructor,
    /// `xs.append(x)` keeps the receiver in the callee.
    Dot { receiver: &'a Expression },
    /// `Slice.append(xs, x)` keeps the receiver in the first argument.
    Identifier,
}

impl<'a> NativeCallForm<'a> {
    pub(super) fn method(function: &'a Expression) -> Self {
        match function {
            Expression::DotAccess { expression, .. } => Self::Dot {
                receiver: expression,
            },
            _ => Self::Identifier,
        }
    }

    pub(super) fn split(
        self,
        args: &'a [Expression],
    ) -> Option<(&'a Expression, &'a [Expression])> {
        match self {
            Self::Dot { receiver } => Some((receiver, args)),
            Self::Identifier | Self::Constructor => args.split_first(),
        }
    }
}

pub(crate) fn split_native_receiver<'a>(
    function: &'a Expression,
    args: &'a [Expression],
) -> Option<(&'a Expression, &'a [Expression])> {
    NativeCallForm::method(function).split(args)
}

pub(super) struct NativeCallContext<'a> {
    pub function: &'a Expression,
    pub form: NativeCallForm<'a>,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub resolved_type_args: ResolvedCallTypeArguments<'a>,
    pub abi: &'a CallableAbi,
    pub call_ty: Option<&'a Type>,
    pub native_type: &'a NativeTypeKind,
    pub method: &'a str,
    pub capture_boundary: CaptureBoundary,
    pub retired_receiver: Option<&'a Expression>,
}

impl<'a> NativeCallContext<'a> {
    pub(super) fn receiver_and_arguments(&self) -> Option<(&'a Expression, &'a [Expression])> {
        self.form.split(self.args)
    }
}
