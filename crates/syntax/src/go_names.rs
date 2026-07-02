//! Go identifier computation shared by the checker and the emitter, so
//! neither has to mirror the other's naming policy.

use std::borrow::Cow;

/// Go reserved keywords that cannot be used as identifiers.
/// See: https://go.dev/ref/spec#Keywords
pub const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

pub const ENUM_TAG_FIELD: &str = "Tag";

pub const ENUM_STRINGER_METHOD: &str = "String";
pub const ENUM_GO_STRINGER_METHOD: &str = "GoString";

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub fn snake_to_camel(s: &str) -> String {
    s.split('_').map(capitalize_first).collect()
}

pub fn escape_keyword(name: &str) -> Cow<'_, str> {
    if GO_KEYWORDS.contains(&name) {
        Cow::Owned(format!("{}_", name))
    } else {
        Cow::Borrowed(name)
    }
}

/// Go struct field name for an enum variant field. Emit's enum layout and
/// the checker's cross-variant conflict check must both use this single
/// authority so their notions of a field's Go name cannot drift.
pub fn enum_field_go_name(
    variant_name: &str,
    field_name: &str,
    field_index: usize,
    is_struct: bool,
    single_field: bool,
    enum_name: &str,
) -> String {
    if is_struct {
        let base = snake_to_camel(field_name);
        if base == ENUM_TAG_FIELD || base == ENUM_STRINGER_METHOD || base == ENUM_GO_STRINGER_METHOD
        {
            escape_keyword(&format!("{}{}", variant_name, base)).into_owned()
        } else {
            escape_keyword(&base).into_owned()
        }
    } else if single_field {
        let base = variant_name.to_string();
        if base == ENUM_TAG_FIELD || base == ENUM_STRINGER_METHOD || base == ENUM_GO_STRINGER_METHOD
        {
            format!("{}{}_", enum_name, base)
        } else {
            base
        }
    } else {
        let base = format!("{}{}", variant_name, field_index);
        if base == ENUM_TAG_FIELD || base == ENUM_STRINGER_METHOD || base == ENUM_GO_STRINGER_METHOD
        {
            format!("{}{}_{}", enum_name, variant_name, field_index)
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_to_camel_converts_and_normalizes() {
        assert_eq!(snake_to_camel("user_id"), "UserId");
        assert_eq!(snake_to_camel("foo_bar"), "FooBar");
        assert_eq!(snake_to_camel("fooBar"), "FooBar");
        assert_eq!(snake_to_camel("x"), "X");
        assert_eq!(snake_to_camel("x_"), "X");
    }

    #[test]
    fn escape_keyword_appends_underscore() {
        assert_eq!(escape_keyword("type"), "type_");
        assert_eq!(escape_keyword("Type"), "Type");
        assert_eq!(escape_keyword("target"), "target");
    }

    #[test]
    fn enum_field_go_name_struct_fields() {
        assert_eq!(
            enum_field_go_name("Click", "target_id", 0, true, true, "Event"),
            "TargetId"
        );
        assert_eq!(
            enum_field_go_name("Click", "tag", 0, true, true, "Event"),
            "ClickTag"
        );
        assert_eq!(
            enum_field_go_name("Click", "string", 0, true, true, "Event"),
            "ClickString"
        );
        assert_eq!(
            enum_field_go_name("Click", "go_string", 0, true, true, "Event"),
            "ClickGoString"
        );
    }

    #[test]
    fn enum_field_go_name_tuple_fields() {
        assert_eq!(
            enum_field_go_name("Click", "0", 0, false, true, "Event"),
            "Click"
        );
        assert_eq!(
            enum_field_go_name("Tag", "0", 0, false, true, "Event"),
            "EventTag_"
        );
        assert_eq!(
            enum_field_go_name("String", "0", 0, false, true, "Event"),
            "EventString_"
        );
        assert_eq!(
            enum_field_go_name("Click", "1", 1, false, false, "Event"),
            "Click1"
        );
    }
}
