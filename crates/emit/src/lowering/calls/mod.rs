mod arguments;
pub(in crate::lowering) mod bound_value;
pub(in crate::lowering) mod bounds_checks;
pub(in crate::lowering) mod call_plan;
mod clone;
pub(in crate::lowering) mod comma_ok;
pub(in crate::lowering) mod dispatch;
pub(in crate::lowering) mod go_interop;
mod native_constructors;
pub(in crate::lowering) mod native_methods;
pub(in crate::lowering) mod predicates;
mod regular;
pub(in crate::lowering) mod slice_loop;
mod type_arguments;
mod ufcs;
pub(in crate::lowering) mod unwrap_or;
pub(in crate::lowering) mod wrap_err;

use crate::Planner;
use crate::lowering::abi::callable::CallableAbi;
use crate::lowering::calls::call_plan::CallableOrigin;
use crate::target::ir::values::CaptureBoundary;
use syntax::ast::Expression;
use syntax::program::{CallOperation, NativeTypeKind};
use syntax::types::Type;

impl<'p> Planner<'p> {
    pub(in crate::lowering) fn native_method_call<'a>(
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
        } = value
        else {
            return None;
        };

        let plan = self.plan_call(value)?;
        let CallableOrigin::Source(CallOperation::Native { kind, method }) = plan.resolved.origin
        else {
            return None;
        };
        let form = NativeCallForm::method(callee);
        form.split(args)?;
        Some(NativeCallContext {
            native_type: *kind,
            form,
            method,
            function: callee,
            args,
            spread: spread.as_deref(),
            written_type_arguments: type_arguments.annotations().len(),
            abi: plan.resolved.abi,
            call_ty: Some(ty),
            capture_boundary: CaptureBoundary::SiblingSequence,
            assigned_place: None,
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

pub(in crate::lowering) fn split_native_receiver<'a>(
    function: &'a Expression,
    args: &'a [Expression],
) -> Option<(&'a Expression, &'a [Expression])> {
    NativeCallForm::method(function).split(args)
}

pub(in crate::lowering) struct NativeCallContext<'a> {
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
    pub assigned_place: Option<&'a Expression>,
}

impl<'a> NativeCallContext<'a> {
    pub(in crate::lowering) fn receiver_and_arguments(
        &self,
    ) -> Option<(&'a Expression, &'a [Expression])> {
        self.form.split(self.args)
    }
}
