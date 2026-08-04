//! Flags f-string interpolation of a first-party struct or enum that has no stringer.

use diagnostics::LocalSink;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::types::Type;

use crate::passes::walk::visit_ast;
use semantics::store::Store;

pub(crate) fn run(items: &[Expression], store: &Store, sink: &LocalSink) {
    visit_ast(
        items,
        &mut |expression, _| check_expression(expression, store, sink),
        &mut |_, _| {},
    );
}

fn check_expression(expression: &Expression, store: &Store, sink: &LocalSink) {
    if let Expression::Literal {
        literal: Literal::FormatString(parts),
        ..
    } = expression
    {
        for part in parts {
            if let FormatStringPart::Expression(inner) = part {
                check_interpolation(inner, store, sink);
            }
        }
    }
}

fn check_interpolation(inner: &Expression, store: &Store, sink: &LocalSink) {
    let ty = inner.get_type();
    if store.is_interpolatable(&ty) {
        return;
    }
    let peeled = store.peel_alias(&ty);
    let Type::Nominal { id, .. } = &peeled else {
        return;
    };
    let Some(definition) = store.get_definition(id.as_str()) else {
        return;
    };
    sink.push(diagnostics::infer::interpolation_without_stringer(
        id.last_segment(),
        inner.get_span(),
        definition.is_pointer_backed_newtype(|id| store.get_definition(id)),
    ));
}
