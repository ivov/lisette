use syntax::ast::{Annotation, Binding, Expression, Pattern, RestPattern, Span, TypedPattern};
use syntax::types::Type;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::patterns::get_pattern_element_type;
use crate::position::LineIndex;

/// Inlay hints for `items` within the range.
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

    match expression {
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            binding_type_hint(binding, range, line_index, hints);
        }

        Expression::Match { subject, arms, .. } => {
            let fallback = subject.get_type();
            for arm in arms {
                pattern_hints(
                    &arm.pattern,
                    arm.typed_pattern.as_ref(),
                    &fallback,
                    range,
                    line_index,
                    hints,
                );
            }
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            typed_pattern,
            ..
        }
        | Expression::WhileLet {
            pattern,
            scrutinee,
            typed_pattern,
            ..
        } => {
            pattern_hints(
                pattern,
                typed_pattern.as_ref(),
                &scrutinee.get_type(),
                range,
                line_index,
                hints,
            );
        }

        Expression::Call {
            expression: callee,
            args,
            ..
        } => collect_parameter_hints(callee, args, range, line_index, hints),

        Expression::Lambda {
            params,
            return_annotation,
            body,
            ty,
            ..
        } => lambda_hints(
            params,
            return_annotation,
            body,
            ty,
            range,
            line_index,
            hints,
        ),

        _ => {}
    }

    for child in expression.children() {
        walk(child, range, line_index, hints);
    }
}

fn binding_type_hint(
    binding: &Binding,
    range: (u32, u32),
    line_index: &LineIndex,
    hints: &mut Vec<InlayHint>,
) {
    if binding.annotation.is_some() {
        return;
    }
    // Identifier patterns only; destructuring is handled via the pattern path.
    let Pattern::Identifier { span, .. } = &binding.pattern else {
        return;
    };
    let at = span.byte_offset + span.byte_length;
    push_type_hint(at, &binding.ty, range, line_index, hints);
}

fn pattern_hints(
    pattern: &Pattern,
    typed_pattern: Option<&TypedPattern>,
    fallback: &Type,
    range: (u32, u32),
    line_index: &LineIndex,
    hints: &mut Vec<InlayHint>,
) {
    let mut spans = Vec::new();
    collect_identifier_spans(pattern, &mut spans);
    for span in spans {
        if let Some((ty, name_span)) =
            get_pattern_element_type(pattern, typed_pattern, fallback, span.byte_offset)
        {
            let at = name_span.byte_offset + name_span.byte_length;
            push_type_hint(at, &ty, range, line_index, hints);
        }
    }
}

fn collect_identifier_spans(pattern: &Pattern, out: &mut Vec<Span>) {
    match pattern {
        Pattern::Identifier { span, .. } => out.push(*span),
        Pattern::Tuple { elements, .. }
        | Pattern::EnumVariant {
            fields: elements, ..
        } => elements
            .iter()
            .for_each(|p| collect_identifier_spans(p, out)),
        Pattern::Struct { fields, .. } => fields
            .iter()
            .for_each(|f| collect_identifier_spans(&f.value, out)),
        Pattern::Slice { prefix, rest, .. } => {
            prefix.iter().for_each(|p| collect_identifier_spans(p, out));
            if let RestPattern::Bind { span, .. } = rest {
                out.push(*span);
            }
        }
        // Alternatives are distinct occurrences (same names, different spans).
        Pattern::Or { patterns, .. } => patterns
            .iter()
            .for_each(|p| collect_identifier_spans(p, out)),
        Pattern::AsBinding {
            pattern,
            name,
            span,
        } => {
            collect_identifier_spans(pattern, out);
            let name_len = name.len() as u32;
            out.push(Span::new(
                span.file_id,
                span.byte_offset + span.byte_length - name_len,
                name_len,
            ));
        }
        Pattern::Literal { .. } | Pattern::WildCard { .. } | Pattern::Unit { .. } => {}
    }
}

/// `: T` hints for a lambda's un-annotated params, plus `-> T` when the return is omitted.
fn lambda_hints(
    params: &[Binding],
    return_annotation: &Annotation,
    body: &Expression,
    ty: &Type,
    range: (u32, u32),
    line_index: &LineIndex,
    hints: &mut Vec<InlayHint>,
) {
    for param in params {
        binding_type_hint(param, range, line_index, hints);
    }

    // Skip a curried lambda's return; the inner lambda's own hints convey the shape.
    if matches!(return_annotation, Annotation::Unknown)
        && !matches!(body, Expression::Lambda { .. })
        && let Some(ret) = ty.get_function_ret()
        && !ret.is_unit()
        && !ret.is_type_var()
        && !ret.is_error()
    {
        let at = leftmost_offset(body);
        if at >= range.0 && at < range.1 {
            hints.push(InlayHint {
                position: line_index.offset_to_position(at),
                label: InlayHintLabel::String(format!("-> {ret}")),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                // No left padding: the body is already preceded by a source space.
                padding_left: None,
                padding_right: Some(true),
                data: None,
            });
        }
    }
}

fn push_type_hint(
    at: u32,
    ty: &Type,
    range: (u32, u32),
    line_index: &LineIndex,
    hints: &mut Vec<InlayHint>,
) {
    if ty.is_type_var() || ty.is_error() {
        return;
    }
    if at >= range.0 && at < range.1 {
        hints.push(InlayHint {
            position: line_index.offset_to_position(at),
            label: InlayHintLabel::String(format!(": {ty}")),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        });
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

    // Zipping against `param_names` stops at the last param, so a trailing variadic labels
    // only its first argument.
    for (arg, name) in args.iter().zip(f.param_names.iter()) {
        let Some(name) = name else { continue };

        if let Expression::Identifier { value, .. } = arg.unwrap_parens()
            && value.as_str() == name.as_str()
        {
            continue;
        }

        let at = leftmost_offset(arg);
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

/// Leftmost source offset of an expression. Postfix nodes (`x[i]`, `x.*`, `x?`) carry a
/// span that starts at the operator, so take the min across the whole subtree.
fn leftmost_offset(expr: &Expression) -> u32 {
    let mut min = expr.get_span().byte_offset;
    for child in expr.children() {
        min = min.min(leftmost_offset(child));
    }
    min
}
