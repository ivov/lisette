use super::helpers::{is_empty_block, mentions_identifier, replacement_drops_comment};
use crate::passes::walk::NodeCtx;
use diagnostics::{Edit, Fix};
use syntax::ast::{Expression, Pattern, Span};

pub fn check_redundant_else(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Block { items, .. } = expression else {
        return;
    };

    // Only a non-tail `if` is in statement position; the block's tail item is a
    // value, where the idiomatic form is `if c { a } else { b }` and dropping the
    // `else` would conflict with the `unnecessary_return` advice on the branch.
    if items.len() < 2 {
        return;
    }
    let leading = &items[..items.len() - 1];

    for (index, item) in leading.iter().enumerate() {
        let Expression::If {
            consequence,
            alternative,
            span: if_span,
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
        let mut diagnostic = diagnostics::lint::redundant_else(&else_span);
        if let Some(fix) = denest_fix(
            ctx.source,
            if_span,
            consequence_span,
            alternative,
            &items[index + 1..],
        ) {
            diagnostic = diagnostic.with_fix(fix);
        }
        ctx.sink.push(diagnostic);
    }
}

fn denest_fix(
    source: &str,
    if_span: &Span,
    consequence_span: Span,
    alternative: &Expression,
    following: &[Expression],
) -> Option<Fix> {
    let Expression::Block { items, .. } = alternative else {
        return None;
    };
    if else_body_leaks_binding(items, following) {
        return None;
    }
    let alternative_span = alternative.get_span();
    let inner = source
        .get(alternative_span.byte_offset as usize + 1..alternative_span.end() as usize - 1)?;
    let if_indent = line_indent(source, if_span.byte_offset);
    let body = reindent(inner, if_indent)?;

    let removed = Span::new(
        consequence_span.file_id,
        consequence_span.end(),
        alternative_span.end() - consequence_span.end(),
    );
    let replacement = format!("\n{body}");
    if replacement_drops_comment(source, removed, &replacement) {
        return None;
    }
    Some(Fix::new(
        "Drop the `else` and lift its body out",
        Edit::replacement(removed, replacement),
    ))
}

fn else_body_leaks_binding(else_items: &[Expression], following: &[Expression]) -> bool {
    else_items.iter().any(|item| match item {
        Expression::Let { binding, .. } => match &binding.pattern {
            Pattern::Identifier { identifier, .. } => following
                .iter()
                .any(|after| mentions_identifier(after, identifier)),
            _ => true,
        },
        Expression::Const { .. } | Expression::Function { .. } => true,
        _ => false,
    })
}

fn line_indent(source: &str, offset: u32) -> &str {
    let bytes = source.as_bytes();
    let mut start = offset as usize;
    while start > 0 && bytes[start - 1] != b'\n' {
        start -= 1;
    }
    let prefix = &source[start..offset as usize];
    &prefix[..prefix.len() - prefix.trim_start().len()]
}

fn reindent(inner: &str, target_indent: &str) -> Option<String> {
    let lines: Vec<&str> = inner
        .lines()
        .skip_while(|line| line.trim().is_empty())
        .collect();
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)?;
    let content = &lines[..end];

    let common = content
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()?;

    Some(
        content
            .iter()
            .map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    format!("{target_indent}{}", line[common..].trim_end())
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Offset of the `else` keyword between a consequence and its alternative, or
/// `None` when a comment intervenes so the keyword cannot be located by trimming.
fn else_keyword_offset(source: &str, start: u32, end: u32) -> Option<u32> {
    let gap = source.get(start as usize..end as usize)?;
    let trimmed = gap.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '}');
    if trimmed.starts_with("else") {
        Some(end - trimmed.len() as u32)
    } else {
        None
    }
}
