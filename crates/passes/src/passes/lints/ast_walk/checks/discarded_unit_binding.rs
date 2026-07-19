use crate::passes::lints::span_edit::statement_deletion;
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Pattern, Span};

/// Flags `let _ = expr` where `expr` has unit type, so the discard binds nothing.
pub fn check_discarded_unit_binding(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Let {
        binding,
        value,
        else_block: None,
        assert: false,
        span,
        ..
    } = expression
    else {
        return;
    };

    // An annotated wildcard (`let _: () = f()`) is a deliberate type assertion.
    if binding.annotation.is_some() {
        return;
    }

    if !matches!(binding.pattern, Pattern::WildCard { .. }) {
        return;
    }

    // Strictly unit, so never/error/variable types (e.g. `let _ = panic(...)`)
    // are left alone and the lint stays sound on checker-rejected code.
    if !value.get_type().is_unit() {
        return;
    }

    let (fix_message, edit) = match value.unwrap_parens() {
        // `let _ = ()` has nothing to keep as a statement.
        Expression::Unit { .. } => (
            "Remove the discard",
            Edit::deletion(statement_deletion(ctx.source, *span)),
        ),
        _ => {
            let value_span = value.get_span();
            let prefix = Span::new(
                span.file_id,
                span.byte_offset,
                value_span.byte_offset - span.byte_offset,
            );
            (
                "Remove the discard and keep the expression as a statement",
                Edit::deletion(prefix),
            )
        }
    };

    ctx.sink.push(
        diagnostics::lint::discarded_unit_binding(span).with_fix(Fix::new(fix_message, edit)),
    );
}
