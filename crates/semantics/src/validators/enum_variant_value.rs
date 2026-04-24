use diagnostics::LocalSink;
use syntax::ast::{Expression, Span};
use syntax::types::{Type, unqualified_name};

use crate::store::Store;

pub(super) fn run(typed_ast: &[Expression], store: &Store, sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, store, sink);
    }
}

fn visit_expression(expression: &Expression, store: &Store, sink: &LocalSink) {
    match expression {
        Expression::Identifier {
            qualified: Some(qualified),
            value,
            span,
            ..
        } => {
            if let Some((enum_id, variant_name)) = qualified.rsplit_once('.') {
                check(enum_id, variant_name, value, *span, store, sink);
            }
        }
        Expression::DotAccess {
            expression: base,
            member,
            span,
            ..
        } => {
            if let Type::Nominal { id, .. } = base.get_type().strip_refs() {
                let display = format!("{}.{}", unqualified_name(&id), member);
                check(&id, member, &display, *span, store, sink);
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, store, sink);
    }
}

fn check(
    enum_qualified: &str,
    variant_name: &str,
    display: &str,
    span: Span,
    store: &Store,
    sink: &LocalSink,
) {
    let Some(variant) = store.variant_of(enum_qualified, variant_name) else {
        return;
    };
    if !variant.fields.is_struct() {
        return;
    }
    sink.push(diagnostics::infer::enum_variant_constructor_value(
        display, span,
    ));
}
