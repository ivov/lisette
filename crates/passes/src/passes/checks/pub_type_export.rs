//! Reject public type declarations whose name cannot be exported from Go.
//! Lisette maps a `pub` type to an exported Go identifier, which must begin
//! with an uppercase letter. A public type with a lowercase name emits an
//! unexported Go definition while cross-module references reach for the
//! exported spelling, so downstream packages fail to compile.

use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Span, Visibility};

use crate::passes::lints::ast_walk::casing::to_pascal_case;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Some((name, name_span, visibility)) = type_declaration(expression)
        && visibility.is_public()
        && !is_exportable(name)
    {
        ctx.sink.push(diagnostics::infer::pub_type_not_exportable(
            name,
            &to_pascal_case(name),
            *name_span,
        ));
    }
}

fn type_declaration(expression: &Expression) -> Option<(&str, &Span, &Visibility)> {
    match expression {
        Expression::Struct {
            name,
            name_span,
            visibility,
            ..
        }
        | Expression::Enum {
            name,
            name_span,
            visibility,
            ..
        }
        | Expression::TypeAlias {
            name,
            name_span,
            visibility,
            ..
        }
        | Expression::Interface {
            name,
            name_span,
            visibility,
            ..
        } => Some((name.as_str(), name_span, visibility)),
        _ => None,
    }
}

fn is_exportable(name: &str) -> bool {
    name.chars().next().unwrap_or('A').is_uppercase()
}
