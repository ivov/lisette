use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, FormatStringPart, Literal};

pub fn check_nested_fstring(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Literal {
        literal: Literal::FormatString(parts),
        ..
    } = expression
    else {
        return;
    };

    // Solo f-strings are owned by `expression_only_fstring`.
    if parts.len() < 2 {
        return;
    }

    for part in parts {
        let FormatStringPart::Expression(value) = part else {
            continue;
        };
        let Expression::Literal {
            literal: Literal::FormatString(inner_parts),
            span: inner_span,
            ..
        } = value.unwrap_parens()
        else {
            continue;
        };
        // Cede a text-only inner to `uninterpolated_fstring` and a solo
        // string-typed inner to `expression_only_fstring`.
        let has_interpolation = inner_parts
            .iter()
            .any(|part| matches!(part, FormatStringPart::Expression(_)));
        if !has_interpolation
            || ctx
                .facts
                .expression_only_fstrings
                .iter()
                .any(|fact| fact.span == *inner_span)
        {
            continue;
        }
        ctx.sink.push(diagnostics::lint::nested_fstring(inner_span));
    }
}
