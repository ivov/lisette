use syntax::ast::{Expression, Pattern, Span};
use syntax::types::{CompoundKind, Type};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::position::LineIndex;

/// `let`-binding type hints and call-site parameter-name hints in the range.
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

    if let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    {
        collect_parameter_hints(callee, args, range, line_index, hints);
    }

    for child in expression.children() {
        walk(child, range, line_index, hints);
    }
}

/// Parameter-name hints before each argument that maps to a named parameter.
fn collect_parameter_hints(
    callee: &Expression,
    args: &[Expression],
    range: (u32, u32),
    line_index: &LineIndex,
    hints: &mut Vec<InlayHint>,
) {
    let callee_ty = callee.get_type();
    let func = match &callee_ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let Type::Function(f) = func else { return };

    let fixed_param_count = match f.params.last() {
        Some(Type::Compound {
            kind: CompoundKind::VarArgs,
            ..
        }) => f.params.len() - 1,
        _ => f.params.len(),
    };

    for (arg, name) in args
        .iter()
        .zip(f.param_names.iter())
        .take(fixed_param_count)
    {
        let Some(name) = name else { continue };

        if let Expression::Identifier { value, .. } = arg.unwrap_parens()
            && value.as_str() == name.as_str()
        {
            continue;
        }

        let at = arg.get_span().byte_offset;
        if at >= range.0 && at < range.1 {
            hints.push(InlayHint {
                position: line_index.offset_to_position(at),
                label: InlayHintLabel::String(format!("{name}:")),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: Some(true),
                data: None,
            });
        }
    }
}

fn overlaps(span: Span, range: (u32, u32)) -> bool {
    span.byte_offset < range.1 && span.byte_offset + span.byte_length > range.0
}
