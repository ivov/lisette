//! Reject impl methods whose name maps to Go's `fmt.Stringer` /
//! `fmt.GoStringer` (`String` or `GoString` after Lisette → Go name mangling)
//! when the signature is not `(self) -> string`. Without this check the
//! emitted Go would have two methods named `String` (or `GoString`) and fail
//! to compile with "method redeclared".

use crate::passes::walk::NodeCtx;
use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::types::{SimpleKind, Type};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::ImplBlock { methods, .. } = expression {
        for method in methods {
            check_method(method, ctx.sink);
        }
    }
}

fn check_method(method: &Expression, sink: &LocalSink) {
    let Expression::Function {
        name,
        name_span,
        params,
        return_type,
        ..
    } = method
    else {
        return;
    };

    if !is_reserved_stringer_name(name) {
        return;
    }

    let returns_string = matches!(return_type, Type::Simple(SimpleKind::String));
    if params.len() == 1 && returns_string {
        return;
    }

    sink.push(diagnostics::infer::stringer_signature_mismatch(
        name, *name_span,
    ));
}

fn is_reserved_stringer_name(name: &str) -> bool {
    matches!(name, "string" | "String" | "goString" | "GoString")
}
