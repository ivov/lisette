use crate::passes::walk::NodeCtx;
use diagnostics::lint::PrintfOperand;
use std::str;
use syntax::ast::{Expression, Literal};
use syntax::lex::string_bytes;

/// (package_id, function_name, index of the format-string argument)
const PRINTF_TARGETS: &[(&str, &str, usize)] = &[
    ("go:fmt", "Appendf", 1),
    ("go:fmt", "Errorf", 0),
    ("go:fmt", "Fprintf", 1),
    ("go:fmt", "Printf", 0),
    ("go:fmt", "Sprintf", 0),
    ("go:log", "Fatalf", 0),
    ("go:log", "Panicf", 0),
    ("go:log", "Printf", 0),
];

pub fn check_printf_verb_mismatch(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        span,
        ..
    } = expression
    else {
        return;
    };

    if spread.is_some() {
        return;
    }

    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    let namespace_ty = namespace.get_type();
    let Some(package_id) = namespace_ty.as_import_namespace() else {
        return;
    };

    let Some((_, function, format_index)) = PRINTF_TARGETS
        .iter()
        .find(|(package, function, _)| package_id == *package && member.as_str() == *function)
    else {
        return;
    };

    let Some(Expression::Literal {
        literal: Literal::String { value: format, raw },
        ..
    }) = args.get(*format_index).map(Expression::unwrap_parens)
    else {
        return;
    };

    let Some(runtime_format) = string_bytes(format, *raw) else {
        return;
    };
    let Some(operands) = format_operands(&runtime_format) else {
        return;
    };

    let supplied = args.len() - format_index - 1;
    if operands.len() == supplied {
        return;
    }

    ctx.sink.push(diagnostics::lint::printf_verb_mismatch(
        span, package_id, function, &operands, supplied,
    ));
}

/// The parts of `format` that consume an argument, or `None` for an explicit
/// argument index or a verbless directive, neither of which this check models.
fn format_operands(format: &[u8]) -> Option<Vec<PrintfOperand>> {
    let mut operands = Vec::new();
    let mut cursor = 0;

    while cursor < format.len() {
        if format[cursor] != b'%' {
            cursor += 1;
            continue;
        }
        cursor += 1;

        while matches!(format.get(cursor), Some(b'+' | b'-' | b'#' | b' ' | b'0')) {
            cursor += 1;
        }

        if format.get(cursor) == Some(&b'[') {
            return None;
        }
        if format.get(cursor) == Some(&b'*') {
            operands.push(PrintfOperand::StarWidth);
            cursor += 1;
        } else {
            while matches!(format.get(cursor), Some(b'0'..=b'9')) {
                cursor += 1;
            }
        }

        if format.get(cursor) == Some(&b'.') {
            cursor += 1;
            if format.get(cursor) == Some(&b'[') {
                return None;
            }
            if format.get(cursor) == Some(&b'*') {
                operands.push(PrintfOperand::StarPrecision);
                cursor += 1;
            } else {
                while matches!(format.get(cursor), Some(b'0'..=b'9')) {
                    cursor += 1;
                }
            }
        }

        if format.get(cursor) == Some(&b'[') {
            return None;
        }

        // A `%` verb consumes nothing, but a `*` before it still takes one.
        let (verb, width) = read_verb(format.get(cursor..)?)?;
        cursor += width;
        if verb != '%' {
            operands.push(PrintfOperand::Verb(verb));
        }
    }

    Some(operands)
}

/// The verb at the start of `tail` and its byte length, as `fmt` reads it.
fn read_verb(tail: &[u8]) -> Option<(char, usize)> {
    let first = *tail.first()?;
    if first < 0x80 {
        return Some((first as char, 1));
    }
    for width in 2..=4 {
        if let Some(sequence) = tail.get(..width)
            && let Ok(text) = str::from_utf8(sequence)
            && let Some(verb) = text.chars().next()
        {
            return Some((verb, width));
        }
    }
    Some((char::REPLACEMENT_CHARACTER, 1))
}
