//! Reject user type declarations and generic parameters named after the Go
//! predeclared identifiers `any` and `comparable`. The emitter prints those
//! names directly for `Unknown` and for unconstrained / map-key generic
//! constraints, so a user declaration of the same name shadows the builtin and
//! the generated Go fails to compile.

use crate::passes::walk::NodeCtx;
use diagnostics::LocalSink;
use syntax::ast::{Expression, Generic};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    check_type_name(expression, ctx.sink);
    for generic in generics_of(expression) {
        if is_reserved(&generic.name) {
            ctx.sink.push(diagnostics::infer::predeclared_type_shadowed(
                &generic.name,
                generic.span,
            ));
        }
    }
}

fn check_type_name(expression: &Expression, sink: &LocalSink) {
    let (name, name_span) = match expression {
        Expression::Enum {
            name, name_span, ..
        }
        | Expression::Struct {
            name, name_span, ..
        }
        | Expression::TypeAlias {
            name, name_span, ..
        }
        | Expression::Interface {
            name, name_span, ..
        } => (name, name_span),
        _ => return,
    };
    if is_reserved(name) {
        sink.push(diagnostics::infer::predeclared_type_shadowed(
            name, *name_span,
        ));
    }
}

fn generics_of(expression: &Expression) -> &[Generic] {
    match expression {
        Expression::Function { generics, .. }
        | Expression::ImplBlock { generics, .. }
        | Expression::Enum { generics, .. }
        | Expression::Struct { generics, .. }
        | Expression::TypeAlias { generics, .. }
        | Expression::Interface { generics, .. } => generics,
        _ => &[],
    }
}

fn is_reserved(name: &str) -> bool {
    matches!(name, "any" | "comparable")
}
