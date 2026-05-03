//! Newtype-related invariants. A newtype (single-field, non-generic tuple
//! struct) compiles to a Go named scalar. `.0` is a cast, not a field — so
//! it's read-only (can't assign to) and non-addressable (can't take `&`).

use diagnostics::LocalSink;
use syntax::ast::{Expression, Span, UnaryOperator};
use syntax::types::{Type, unqualified_name};

use crate::store::Store;

pub(super) fn run(typed_ast: &[Expression], store: &Store, sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, store, sink);
    }
}

fn visit_expression(expression: &Expression, store: &Store, sink: &LocalSink) {
    match expression {
        Expression::Assignment { target, span, .. } => {
            check_newtype_field_assignment(target, *span, store, sink);
            if has_map_field_in_chain(target) {
                sink.push(diagnostics::infer::map_field_chain_assignment(*span));
            }
        }
        Expression::Reference {
            expression, span, ..
        } if targets_newtype_field(expression, store) => {
            sink.push(diagnostics::infer::reference_through_newtype(*span));
        }
        _ => {}
    }
    for child in expression.children() {
        visit_expression(child, store, sink);
    }
}

fn check_newtype_field_assignment(
    target: &Expression,
    span: Span,
    store: &Store,
    sink: &LocalSink,
) {
    match target {
        Expression::DotAccess {
            expression, member, ..
        } => {
            if member == "0"
                && let Type::Nominal { id, .. } = expression.get_type().strip_refs()
                && let Some(def) = store.get_definition(id.as_str())
                && def.is_newtype()
            {
                let type_name = unqualified_name(&id);
                sink.push(diagnostics::infer::newtype_field_assignment(
                    type_name, span,
                ));
                return;
            }
            check_newtype_field_assignment(expression, span, store, sink);
        }
        Expression::IndexedAccess { expression, .. } => {
            check_newtype_field_assignment(expression, span, store, sink);
        }
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression,
            ..
        } => {
            check_newtype_field_assignment(expression, span, store, sink);
        }
        _ => {}
    }
}

fn targets_newtype_field(expression: &Expression, store: &Store) -> bool {
    let mut current = expression.unwrap_parens();
    while let Expression::DotAccess {
        expression: inner,
        member,
        ..
    } = current
    {
        if member.parse::<usize>().is_ok()
            && let Type::Nominal { id, .. } = inner.get_type().strip_refs()
            && let Some(def) = store.get_definition(id.as_str())
            && def.is_newtype()
        {
            return true;
        }
        current = inner.unwrap_parens();
    }
    false
}

fn has_map_field_in_chain(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::DotAccess { expression, .. } => {
            is_map_indexed_access(expression) || has_map_field_in_chain(expression)
        }
        _ => false,
    }
}

fn is_map_indexed_access(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::IndexedAccess { expression, .. } => expression.get_type().has_name("Map"),
        _ => false,
    }
}
