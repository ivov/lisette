use crate::passes::lints::span_edit::statement_deletion;
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Span};

pub fn check_needless_continue(expression: &Expression, ctx: &NodeCtx) {
    let (Expression::Loop { body, .. }
    | Expression::While { body, .. }
    | Expression::WhileLet { body, .. }
    | Expression::For { body, .. }) = expression
    else {
        return;
    };

    let Expression::Block { items, .. } = body.as_ref() else {
        return;
    };
    let Some(Expression::Continue { span }) = items.last() else {
        return;
    };

    let keyword = Span::new(span.file_id, span.byte_offset, "continue".len() as u32);

    let mut diagnostic = diagnostics::lint::needless_continue(&keyword);
    if !same_line_comment_follows(ctx.source, keyword) {
        diagnostic = diagnostic.with_fix(Fix::new(
            "Remove the redundant `continue`",
            Edit::deletion(statement_deletion(ctx.source, keyword)),
        ));
    }
    ctx.sink.push(diagnostic);
}

fn same_line_comment_follows(source: &str, span: Span) -> bool {
    let Some(rest) = source.get(span.end() as usize..) else {
        return false;
    };
    let line_end = rest.find('\n').unwrap_or(rest.len());
    rest[..line_end].contains("//")
}
