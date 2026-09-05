use std::borrow::Cow;
use syntax::ast::Literal;
use syntax::lex::{rune_codepoint, string_bytes};

pub(crate) fn runtime_bytes(literal: &Literal) -> Option<Cow<'_, [u8]>> {
    if let Literal::String { value, raw } = literal {
        string_bytes(value, *raw)
    } else {
        None
    }
}

fn integer_value(literal: &Literal) -> Option<u64> {
    match literal {
        Literal::Integer { value, .. } => Some(*value),
        Literal::Char(text) => rune_codepoint(text).map(u64::from),
        _ => None,
    }
}

pub(crate) fn equals_target(
    candidate: &Literal,
    target: &Literal,
    target_bytes: Option<&[u8]>,
) -> bool {
    if let (Some(candidate_value), Some(target_value)) =
        (integer_value(candidate), integer_value(target))
    {
        return candidate_value == target_value;
    }
    if let Literal::String { value: cv, raw: cr } = candidate
        && let Literal::String { value: tv, raw: tr } = target
    {
        if cr == tr && cv == tv {
            return true;
        }
        if let Some(target_bytes) = target_bytes
            && let Some(candidate_bytes) = string_bytes(cv, *cr)
        {
            return candidate_bytes.as_ref() == target_bytes;
        }
    }
    candidate == target
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
    fn capital_unicode_escape_equals_literal_char() {
        assert!(equal(&s("A", false), &s("\\U00000041", false)));
        assert!(!equal(&s("\\\\U00000041", false), &s("\\U00000041", false)));
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
    fn identical_source_short_circuits() {
        let a = s("hello", false);
        let b = s("hello", false);
        assert!(equals_target(&a, &b, None));
    }

    fn integer(value: u64, text: Option<&str>) -> Literal {
        Literal::Integer {
            value,
            text: text.map(str::to_string),
        }
    }

    fn c(text: &str) -> Literal {
        Literal::Char(text.to_string())
    }

    #[test]
    fn integer_spellings_with_same_value_are_equal() {
        assert!(equal(&integer(1, None), &integer(1, Some("0x1"))));
        assert!(equal(&integer(1, Some("0b1")), &integer(1, Some("0o1"))));
        assert!(!equal(&integer(1, None), &integer(2, Some("0x2"))));
    }

    #[test]
    fn negative_spellings_with_same_value_are_equal() {
        let minus_one = 1u64.wrapping_neg();
        assert!(equal(
            &integer(minus_one, Some("-1")),
            &integer(minus_one, Some("-0x1"))
        ));
        assert!(!equal(&integer(minus_one, Some("-1")), &integer(1, None)));
    }

    #[test]
    fn char_equals_integer_with_its_codepoint() {
        assert!(equal(&c("a"), &integer(97, None)));
        assert!(equal(&integer(97, None), &c("a")));
        assert!(!equal(&c("b"), &integer(97, None)));
    }

    #[test]
    fn char_spellings_with_same_codepoint_are_equal() {
        assert!(equal(&c("a"), &c("\\x61")));
        assert!(equal(&c("\\n"), &c("\\u{000A}")));
        assert!(equal(&c("\\n"), &c("\\012")));
        assert!(!equal(&c("a"), &c("b")));
    }

    #[test]
    fn integer_and_string_are_not_equal() {
        assert!(!equal(&integer(1, None), &s("1", false)));
    }
}
