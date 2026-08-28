//! Validate that the first parameter of every method in an `impl` block is
//! named `self` when its type matches the receiver type, and that a parameter
//! named `self` has the right type. Methods with an unrelated first parameter
//! are treated as static methods and skipped.

use crate::passes::walk::NodeCtx;
use diagnostics::LocalSink;
use syntax::ast::{Expression, Pattern};
use syntax::types::Type;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::ImplBlock {
        ty: impl_ty,
        methods,
        ..
    } = expression
    {
        for method in methods {
            check_method_receiver(method, impl_ty, ctx.sink);
        }
    }
}

fn check_method_receiver(method: &Expression, impl_ty: &Type, sink: &LocalSink) {
    let Expression::Function { params, .. } = method else {
        return;
    };
    let Some(first_param) = params.first() else {
        return;
    };
    let Pattern::Identifier { identifier, span } = &first_param.pattern else {
        return;
    };

    let receiver_ty = first_param.ty.strip_refs();
    let types_match = receiver_ty.shallow_demoted() == impl_ty.shallow_demoted();

    if types_match && identifier != "self" {
        sink.push(diagnostics::infer::receiver_must_be_named_self(
            identifier, *span,
        ));
    }

    if !types_match && identifier == "self" && !receiver_ty.contains_error() {
        let annotation_span = first_param
            .annotation
            .as_ref()
            .map(|a| a.get_span())
            .unwrap_or(*span);
        // `stringify` keeps the type arguments, which `get_name` drops.
        sink.push(diagnostics::infer::receiver_type_mismatch(
            &impl_ty.stringify(),
            &receiver_ty.stringify(),
            annotation_span,
        ));
    }
}
