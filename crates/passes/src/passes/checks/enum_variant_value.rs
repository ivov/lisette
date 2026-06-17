use diagnostics::LocalSink;
use syntax::ast::{Expression, Span};
use syntax::types::{Type, unqualified_name};

use crate::passes::walk::NodeCtx;
use semantics::store::Store;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Identifier {
            qualified: Some(qualified),
            value,
            span,
            ..
        } => {
            if let Some((enum_id, variant_name)) = qualified.rsplit_once('.') {
                check_variant(enum_id, variant_name, value, *span, ctx.store, ctx.sink);
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
                check_variant(&id, member, &display, *span, ctx.store, ctx.sink);
            }
        }
        _ => {}
    }
}

fn check_variant(
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
