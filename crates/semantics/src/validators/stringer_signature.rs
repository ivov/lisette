//! Reject impl methods whose name maps to Go's `fmt.Stringer` /
//! `fmt.GoStringer` (`String` or `GoString` after Lisette → Go name mangling)
//! when the signature is not `(self) -> string`. Without this check the
//! emitted Go would have two methods named `String` (or `GoString`) and fail
//! to compile with "method redeclared".

use diagnostics::LocalSink;
use syntax::ast::Expression;
use syntax::types::{SimpleKind, Type};

pub(super) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit(item, sink);
    }
}

fn visit(expression: &Expression, sink: &LocalSink) {
    if let Expression::ImplBlock { methods, .. } = expression {
        for method in methods {
            check(method, sink);
        }
    }
    for child in expression.children() {
        visit(child, sink);
    }
}

fn check(method: &Expression, sink: &LocalSink) {
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
