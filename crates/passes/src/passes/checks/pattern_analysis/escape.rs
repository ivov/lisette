use std::borrow::Cow;
use syntax::ast::Literal;
use syntax::lex::string_bytes;

pub(crate) fn runtime_bytes(literal: &Literal) -> Option<Cow<'_, [u8]>> {
    if let Literal::String { value, raw } = literal {
        string_bytes(value, *raw)
    } else {
        None
    }
}

pub(crate) fn equals_target(
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
}
