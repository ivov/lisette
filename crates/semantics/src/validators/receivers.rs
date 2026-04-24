//! Validate that the first parameter of every method in an `impl` block is
//! named `self` when its type matches the receiver type, and that a parameter
//! named `self` has the right type. Methods with an unrelated first parameter
//! are treated as static methods and skipped.

use diagnostics::LocalSink;
use syntax::ast::{Expression, Pattern};
use syntax::types::Type;

pub(super) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    if let Expression::ImplBlock {
        ty: impl_ty,
        methods,
        ..
    } = expression
    {
        for method in methods {
            check_method_receiver(method, impl_ty, sink);
        }
    }
    for child in expression.children() {
        visit_expression(child, sink);
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
    let types_match = receiver_ty == *impl_ty;

    if types_match && identifier != "self" {
        sink.push(diagnostics::infer::receiver_must_be_named_self(
            identifier, *span,
        ));
    }

    if !types_match && identifier == "self" {
        let annotation_span = first_param
            .annotation
            .as_ref()
            .map(|a| a.get_span())
            .unwrap_or(*span);
        let impl_type_name = impl_ty.get_name().unwrap_or_default();
        let receiver_type_name = receiver_ty.get_name().unwrap_or_default();
        sink.push(diagnostics::infer::receiver_type_mismatch(
            impl_type_name,
            receiver_type_name,
            annotation_span,
        ));
    }
}
