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
use crate::ir::values::CaptureBoundary;
use crate::plan::calls::CallableOrigin;
use syntax::ast::Expression;
use syntax::program::{CallOperation, NativeTypeKind};
use syntax::types::Type;

impl<'p> Planner<'p> {
    pub(crate) fn native_method_call<'a>(
        &self,
        value: &'a Expression,
    ) -> Option<NativeCallContext<'a>>
    where
        'p: 'a,
    {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            type_arguments,
            ty,
            ..
        } = value.unwrap_parens()
        else {
            return None;
        };
        let plan = self.plan_call(value.unwrap_parens())?;
        let CallableOrigin::Source(CallOperation::Native { kind, method }) = plan.resolved.origin
        else {
            return None;
        };
        let function = callee.unwrap_parens();
        let form = NativeCallForm::method(function);
        form.split(args)?;
        Some(NativeCallContext {
            native_type: *kind,
            form,
            method,
            function,
            args,
            spread: spread.as_deref(),
            written_type_arguments: type_arguments.annotations().len(),
            abi: plan.resolved.abi,
            call_ty: Some(ty),
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) enum NativeCallForm<'a> {
    Constructor,
    Dot { receiver: &'a Expression },
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

pub(crate) struct NativeCallContext<'a> {
    pub function: &'a Expression,
    pub form: NativeCallForm<'a>,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub written_type_arguments: usize,
    pub abi: CallableAbi,
    pub call_ty: Option<&'a Type>,
    pub native_type: NativeTypeKind,
    pub method: &'a str,
    pub capture_boundary: CaptureBoundary,
    pub retired_receiver: Option<&'a Expression>,
}

impl<'a> NativeCallContext<'a> {
    pub(crate) fn receiver_and_arguments(&self) -> Option<(&'a Expression, &'a [Expression])> {
        self.form.split(self.args)
    }
}
