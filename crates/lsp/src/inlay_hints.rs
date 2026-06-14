use syntax::ast::{Expression, Pattern, Span};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::position::LineIndex;

/// Type hints for `let` bindings that lack an explicit annotation, within the
/// requested byte range.
pub(crate) fn collect(
    items: &[Expression],
    range: (u32, u32),
    line_index: &LineIndex,
) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    for item in items {
        walk(item, range, line_index, &mut hints);
    }
    hints
}

fn walk(
    expression: &Expression,
    range: (u32, u32),
    line_index: &LineIndex,
    hints: &mut Vec<InlayHint>,
) {
    if !overlaps(expression.get_span(), range) {
        return;
    }

    if let Expression::Let { binding, .. } = expression
        && binding.annotation.is_none()
        && let Pattern::Identifier {
            span: name_span, ..
        } = &binding.pattern
        && !binding.ty.is_type_var()
        && !binding.ty.is_error()
    {
        let at = name_span.byte_offset + name_span.byte_length;
        if at >= range.0 && at < range.1 {
            hints.push(InlayHint {
                position: line_index.offset_to_position(at),
                label: InlayHintLabel::String(format!(": {}", binding.ty)),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: None,
                data: None,
            });
        }
    }

    for child in expression.children() {
        walk(child, range, line_index, hints);
    }
}

fn overlaps(span: Span, range: (u32, u32)) -> bool {
    span.byte_offset < range.1 && span.byte_offset + span.byte_length > range.0
}
