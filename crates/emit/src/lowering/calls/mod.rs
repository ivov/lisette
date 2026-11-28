use crate::lowering::planner::queries::PlannerQueries;
mod arguments;
pub(in crate::lowering) mod bound_value;
pub(in crate::lowering) mod bounds_checks;
pub(in crate::lowering) mod call_plan;
pub(in crate::lowering) mod call_site;
mod clone;
pub(in crate::lowering) mod comma_ok;
mod direct_calls;
pub(in crate::lowering) mod dispatch;
pub(in crate::lowering) mod go_interop;
mod native_constructors;
pub(in crate::lowering) mod native_methods;
pub(in crate::lowering) mod predicates;
pub(in crate::lowering) mod slice_loop;
mod type_arguments;
mod ufcs;
pub(in crate::lowering) mod unwrap_or;
pub(in crate::lowering) mod wrap_err;

use crate::lookup::facts::callables::CallableDeclaration;
use crate::lowering::abi::callable::CallableAbi;
use crate::lowering::calls::call_plan::{CallableOrigin, ResolvedCallee};
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::values::CaptureBoundary;
use syntax::ast::Expression;
use syntax::program::{CallOperation, NativeTypeKind};
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(super) enum NativeCallForm<'a> {
    Constructor,
    Dot { receiver: &'a Expression },
    Identifier,
}

impl<'a> NativeCallForm<'a> {
    pub(super) fn method(function: &'a Expression) -> Self {
        let Expression::DotAccess { expression, .. } = function else {
            return Self::Identifier;
        };
        Self::Dot {
            receiver: expression,
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
    pub writes_type_arguments: bool,
    pub declared_type: Option<&'a Type>,
    pub abi: CallableAbi,
    pub call_ty: Option<&'a Type>,
    pub native_type: NativeTypeKind,
    pub method: &'a str,
    pub capture_boundary: CaptureBoundary,
    pub assigned_place: Option<&'a Expression>,
}

impl<'a> NativeCallContext<'a> {
    pub(in crate::lowering) fn new(
        site: CallSite<'a>,
        callee: &ResolvedCallee<'a>,
        native_type: NativeTypeKind,
        method: &'a str,
    ) -> Self {
        Self {
            function: site.callee,
            form: NativeCallForm::method(site.callee),
            args: site.args,
            spread: site.spread,
            writes_type_arguments: site.writes_type_arguments(),
            declared_type: callee.declaration.map(CallableDeclaration::ty),
            abi: callee.abi.clone(),
            call_ty: None,
            native_type,
            method,
            capture_boundary: CaptureBoundary::SiblingSequence,
            assigned_place: None,
        }
    }

    fn placed(self, call_ty: Option<&'a Type>, ctx: ExpressionContext<'a>) -> Self {
        Self {
            call_ty,
            capture_boundary: ctx.capture_boundary(),
            assigned_place: ctx.assigned_place(),
            ..self
        }
    }

    pub(in crate::lowering) fn receiver_and_arguments(
        &self,
    ) -> Option<(&'a Expression, &'a [Expression])> {
        self.form.split(self.args)
    }
}

impl<'p> PlannerQueries<'_, 'p> {
    pub(in crate::lowering) fn native_method_call<'a>(
        &self,
        value: &'a Expression,
    ) -> Option<NativeCallContext<'a>>
    where
        'p: 'a,
    {
        let site = CallSite::of(value)?;
        if !matches!(
            self.facts.checked(site.callee).callee().operation,
            CallOperation::Native { .. }
        ) {
            return None;
        }
        let plan = self.plan_call(site);
        let CallableOrigin::Source(CallOperation::Native { kind, method }) = plan.resolved.origin
        else {
            return None;
        };
        let call = NativeCallContext {
            call_ty: Some(site.ty),
            ..NativeCallContext::new(site, &plan.resolved, *kind, method)
        };
        call.receiver_and_arguments()?;
        Some(call)
    }
}
