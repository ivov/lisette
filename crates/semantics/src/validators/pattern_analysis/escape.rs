use syntax::ast::Literal;

pub(super) fn runtime_bytes(literal: &Literal) -> Option<Vec<u8>> {
    if let Literal::String { value, raw } = literal {
        Some(decode(value, *raw))
    } else {
        None
    }
}

pub(super) fn equals_target(
    candidate: &Literal,
    target: &Literal,
    target_bytes: Option<&[u8]>,
) -> bool {
    if let Literal::String { value: cv, raw: cr } = candidate
        && let Literal::String { value: tv, raw: tr } = target
    {
        if cr == tr && cv == tv {
            return true;
        }
        if let Some(target_bytes) = target_bytes {
            return decode(cv, *cr) == target_bytes;
        }
    }
    candidate == target
}

fn decode(value: &str, raw: bool) -> Vec<u8> {
    if raw {
        return value.as_bytes().to_vec();
    }

    let mut out = Vec::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\\' {
            push_char_utf8(&mut out, c);
            continue;
        }

        match chars.next() {
            None => out.push(b'\\'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('r') => out.push(b'\r'),
            Some('\\') => out.push(b'\\'),
            Some('\'') => out.push(b'\''),
            Some('"') => out.push(b'"'),
            Some('a') => out.push(0x07),
            Some('b') => out.push(0x08),
            Some('f') => out.push(0x0C),
            Some('v') => out.push(0x0B),
            Some('x') => {
                let mut byte: u8 = 0;
                for _ in 0..2 {
                    match chars.peek().and_then(|d| d.to_digit(16)) {
                        Some(d) => {
                            byte = byte.wrapping_mul(16) + d as u8;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push(byte);
            }
            Some('u') => {
                if chars.next_if_eq(&'{').is_some() {
                    let mut codepoint: u32 = 0;
                    while let Some(&d) = chars.peek() {
                        if d == '}' {
                            chars.next();
                            break;
                        }
                        codepoint = codepoint.wrapping_mul(16) + d.to_digit(16).unwrap_or(0);
                        chars.next();
                    }
                    if let Some(c) = char::from_u32(codepoint) {
                        push_char_utf8(&mut out, c);
                    }
                }
            }
            Some(d @ '0'..='7') => {
                let mut value: u16 = d.to_digit(8).unwrap() as u16;
                for _ in 0..2 {
                    match chars.peek().and_then(|n| n.to_digit(8)) {
                        Some(n) => {
                            value = value * 8 + n as u16;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push((value & 0xFF) as u8);
            }
            Some(other) => {
                out.push(b'\\');
                push_char_utf8(&mut out, other);
            }
        }
    }

    out
}

fn push_char_utf8(out: &mut Vec<u8>, c: char) {
    if c.is_ascii() {
        out.push(c as u8);
        return;
    }
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(value: &str, raw: bool) -> Literal {
        Literal::String {
            value: value.to_string(),
            raw,
        }
    }

    fn equal(a: &Literal, b: &Literal) -> bool {
        equals_target(a, b, runtime_bytes(b).as_deref())
    }

    #[test]
    fn raw_and_escaped_with_same_runtime_value_are_equal() {
        assert!(equal(&s("a\\nb", true), &s("a\\\\nb", false)));
    }

    #[test]
    fn unicode_escape_equals_literal_char() {
        assert!(equal(&s("A", false), &s("\\u{0041}", false)));
    }

    #[test]
    fn hex_escape_equals_literal_char() {
        assert!(equal(&s("A", false), &s("\\x41", false)));
    }

    #[test]
    fn octal_escape_equals_literal_char() {
        assert!(equal(&s("A", false), &s("\\101", false)));
    }

    #[test]
    fn newline_spellings_are_equal() {
        assert!(equal(&s("\\n", false), &s("\\u{000A}", false)));
        assert!(equal(&s("\\n", false), &s("\\x0A", false)));
        assert!(equal(&s("\\n", false), &s("\\012", false)));
    }

    #[test]
    fn distinct_strings_are_not_equal() {
        assert!(!equal(&s("a", false), &s("b", false)));
        assert!(!equal(&s("\\n", false), &s("n", false)));
    }

    #[test]
    fn unicode_escape_for_multibyte_codepoint() {
        assert!(equal(&s("\u{1F600}", false), &s("\\u{1F600}", false)));
    }

    #[test]
    fn raw_string_preserves_backslashes() {
        assert_eq!(decode("a\\nb", true), b"a\\nb".to_vec());
    }

    #[test]
    fn identical_source_short_circuits() {
        let a = s("hello", false);
        let b = s("hello", false);
        assert!(equals_target(&a, &b, None));
    }
}
