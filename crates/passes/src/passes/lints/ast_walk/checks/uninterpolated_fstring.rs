use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, FormatStringPart, Literal};

pub fn check_uninterpolated_fstring(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Literal {
        literal: Literal::FormatString(parts),
        span,
        ..
    } = expression
    else {
        return;
    };

    let has_interpolation = parts
        .iter()
        .any(|p| matches!(p, FormatStringPart::Expression(_)));

    if has_interpolation {
        return;
    }

    let mut diagnostic = diagnostics::lint::uninterpolated_fstring(span);

    if let Some(source) = ctx
        .source
        .get(span.byte_offset as usize..span.end() as usize)
        && let Some(without_prefix) = source.strip_prefix('f')
    {
        let replacement = without_prefix.replace("{{", "{").replace("}}", "}");
        diagnostic = diagnostic.with_fix(Fix::new(
            "Convert to a regular string",
            Edit::replacement(*span, replacement),
        ));
    }

    ctx.sink.push(diagnostic);
}
