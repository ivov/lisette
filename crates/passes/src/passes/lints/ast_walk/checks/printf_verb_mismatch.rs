use crate::passes::walk::NodeCtx;
use diagnostics::lint::PrintfOperand;
use std::borrow::Cow;
use std::iter::Peekable;
use std::str::{self, Chars};
use syntax::ast::{Expression, Literal};

/// (module_id, function_name, index of the format-string argument)
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
    let Some(module_id) = namespace_ty.as_import_namespace() else {
        return;
    };

    let Some((_, function, format_index)) = PRINTF_TARGETS
        .iter()
        .find(|(module, function, _)| module_id == *module && member.as_str() == *function)
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

    let Some(runtime_format) = decode_to_runtime_bytes(format, *raw) else {
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
        span, module_id, function, &operands, supplied,
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

/// The bytes the format holds at runtime. A literal's value arrives with its
/// escapes unresolved, while a raw string keeps its backslashes literally.
fn decode_to_runtime_bytes(format: &str, raw: bool) -> Option<Cow<'_, [u8]>> {
    if raw || !format.contains('\\') {
        return Some(Cow::Borrowed(format.as_bytes()));
    }

    let mut decoded = Vec::with_capacity(format.len());
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            push_char(&mut decoded, ch);
            continue;
        }
        match chars.next()? {
            'a' => decoded.push(0x07),
            'b' => decoded.push(0x08),
            'f' => decoded.push(0x0c),
            'n' => decoded.push(b'\n'),
            'r' => decoded.push(b'\r'),
            't' => decoded.push(b'\t'),
            'v' => decoded.push(0x0b),
            '\\' => decoded.push(b'\\'),
            '"' => decoded.push(b'"'),
            '\'' => decoded.push(b'\''),
            'x' => decoded.push(byte_from_hex(&mut chars)?),
            'u' => push_char(&mut decoded, char_from_braced_hex(&mut chars)?),
            'U' => push_char(&mut decoded, char_from_eight_hex(&mut chars)?),
            first @ '0'..='7' => decoded.push(byte_from_octal(&mut chars, first)?),
            _ => return None,
        }
    }

    Some(Cow::Owned(decoded))
}

fn push_char(decoded: &mut Vec<u8>, ch: char) {
    let mut buffer = [0u8; 4];
    decoded.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
}

fn byte_from_hex(chars: &mut Peekable<Chars<'_>>) -> Option<u8> {
    let high = chars.next()?.to_digit(16)?;
    let low = chars.next()?.to_digit(16)?;
    Some((high * 16 + low) as u8)
}

/// Up to three digits, as the lexer reads `\ooo`.
fn byte_from_octal(chars: &mut Peekable<Chars<'_>>, first: char) -> Option<u8> {
    let mut value = first.to_digit(8)?;
    for _ in 0..2 {
        let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(8)) else {
            break;
        };
        chars.next();
        value = value * 8 + digit;
    }
    u8::try_from(value).ok()
}

/// `\UXXXXXXXX`, the eight-digit form of the unicode escape.
fn char_from_eight_hex(chars: &mut Peekable<Chars<'_>>) -> Option<char> {
    let mut value = 0u32;
    for _ in 0..8 {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    char::from_u32(value)
}

/// One to six digits, as the lexer reads `\u{HEX}`.
fn char_from_braced_hex(chars: &mut Peekable<Chars<'_>>) -> Option<char> {
    if chars.next()? != '{' {
        return None;
    }
    let mut value = 0u32;
    let mut digits = 0;
    while let Some(digit) = chars.peek().and_then(|ch| ch.to_digit(16)) {
        chars.next();
        value = value * 16 + digit;
        digits += 1;
        if digits > 6 {
            return None;
        }
    }
    if digits == 0 || chars.next()? != '}' {
        return None;
    }
    char::from_u32(value)
}
