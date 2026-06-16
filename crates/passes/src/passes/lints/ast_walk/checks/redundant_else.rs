use super::helpers::is_empty_block;
use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Span};

pub fn check_redundant_else(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Block { items, .. } = expression else {
        return;
    };

    // Only a non-tail `if` is in statement position; the block's tail item is a
    // value, where the idiomatic form is `if c { a } else { b }` and dropping the
    // `else` would conflict with the `unnecessary_return` advice on the branch.
    let Some((_, leading)) = items.split_last() else {
        return;
    };

    for item in leading {
        let Expression::If {
            consequence,
            alternative,
            ..
        } = item
        else {
            continue;
        };

        if matches!(alternative.as_ref(), Expression::Unit { .. }) || is_empty_block(alternative) {
            continue;
        }

        if consequence.diverges().is_none() {
            continue;
        }

        let consequence_span = consequence.get_span();
        let Some(else_offset) = else_keyword_offset(
            ctx.source,
            consequence_span.end(),
            alternative.get_span().byte_offset,
        ) else {
            continue;
        };

        let else_span = Span::new(consequence_span.file_id, else_offset, 4);
        ctx.sink.push(diagnostics::lint::redundant_else(&else_span));
    }
}

/// Byte offset of the `else` keyword between the consequence and the alternative,
/// skipping whitespace and the consequence's closing brace. Returns `None` when a
/// comment intervenes, so the lint never mislocates the keyword.
fn else_keyword_offset(source: &str, start: u32, end: u32) -> Option<u32> {
    let gap = source.get(start as usize..end as usize)?;
    let trimmed = gap.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '}');
    if trimmed.starts_with("else") {
        Some(end - trimmed.len() as u32)
    } else {
        None
    }
}
