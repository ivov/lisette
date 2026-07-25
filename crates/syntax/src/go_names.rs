//! Go identifier computation shared by the checker and the emitter, so
//! neither has to mirror the other's naming policy.

use std::borrow::Cow;

use crate::EcoString;
use crate::ast::VariantFields;
use crate::program::MethodSignatures;
use crate::types::{GO_IMPORT_PREFIX, Type};

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

/// Go predeclared identifiers (builtin functions, types, constants).
/// See: https://go.dev/ref/spec#Predeclared_identifiers
pub const GO_BUILTINS: &[&str] = &[
    // Builtin functions
    "any",
    "append",
    "cap",
    "clear",
    "close",
    "complex",
    "copy",
    "delete",
    "imag",
    "init",
    "len",
    "make",
    "max",
    "min",
    "new",
    "panic",
    "print",
    "println",
    "real",
    "recover",
    // Predeclared types
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    // Predeclared constants
    "false",
    "iota",
    "nil",
    "true",
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
    let camel: String = s.split('_').map(capitalize_first).collect();
    if camel.is_empty() || camel.starts_with(char::is_uppercase) {
        camel
    } else {
        format!("X{}", camel)
    }
}

fn split_underscore_prefix(s: &str) -> (&str, &str) {
    s.split_at(s.len() - s.trim_start_matches('_').len())
}

fn camel_segment(segment: &str) -> String {
    if segment.chars().any(char::is_lowercase) {
        return capitalize_first(segment);
    }
    let mut chars = segment.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
    }
}

pub fn screaming_snake_to_camel(s: &str) -> String {
    let (prefix, rest) = split_underscore_prefix(s);
    let converted: String = rest.split('_').map(camel_segment).collect();
    format!("{}{}", prefix, converted)
}

pub fn snake_to_lower_camel(s: &str) -> String {
    let (prefix, rest) = split_underscore_prefix(s);
    let mut segments = rest.split('_');
    let mut out = String::from(prefix);
    if let Some(first) = segments.next() {
        out.push_str(first);
    }
    for segment in segments {
        out.push_str(&capitalize_first(segment));
    }
    out
}

/// The emitted Go name of an unexported method.
pub fn unexported_method_go_name(name: &str) -> String {
    escape_keyword(&snake_to_lower_camel(name)).into_owned()
}

pub fn escape_keyword(name: &str) -> Cow<'_, str> {
    if GO_KEYWORDS.contains(&name) {
        Cow::Owned(format!("{}_", name))
    } else {
        Cow::Borrowed(name)
    }
}

pub fn is_go_reserved_word(name: &str) -> bool {
    GO_KEYWORDS.contains(&name) || GO_BUILTINS.contains(&name)
}

pub fn escape_type_name(name: &str) -> Cow<'_, str> {
    if is_go_reserved_word(name) {
        Cow::Owned(format!("{}_", name))
    } else {
        Cow::Borrowed(name)
    }
}

/// Whether a struct field emits its camelized Go name.
pub fn struct_field_is_exported(
    field: &crate::ast::StructFieldDefinition,
    struct_forces_export: bool,
) -> bool {
    !field.is_embedded()
        && (field.visibility.is_public()
            || struct_forces_export
            || field
                .attributes()
                .iter()
                .any(crate::attributes::field_attribute_forces_export))
}

/// A struct field's emitted Go name under the shared export policy.
pub fn struct_field_go_name(
    field: &crate::ast::StructFieldDefinition,
    struct_forces_export: bool,
) -> Cow<'_, str> {
    if struct_field_is_exported(field, struct_forces_export) {
        Cow::Owned(escape_keyword(&snake_to_camel(&field.name)).into_owned())
    } else if field.is_embedded() {
        escape_keyword(&field.name)
    } else {
        Cow::Owned(escape_keyword(&snake_to_lower_camel(&field.name)).into_owned())
    }
}

/// A candidate method's standing, resolved by the caller on its declaring owner.
#[derive(Clone)]
pub enum ConformanceCandidate {
    /// The method exists in the available method set, but its owner metadata is unavailable.
    Unresolved,
    Resolved {
        exported: bool,
        depth: usize,
        owner: EcoString,
        shadowed: bool,
    },
}

/// The three payload layouts that affect an enum field's emitted Go name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumFieldShape {
    Struct,
    TupleSingle,
    TupleMultiple,
}

#[cfg(test)]
mod shape_tests {
    use super::*;
    use crate::ast::{EnumFieldDefinition, VariantFields};
    use crate::types::Type;

    #[test]
    fn enum_field_shape_captures_only_name_relevant_layouts() {
        let field = EnumFieldDefinition {
            name: "field0".into(),
            name_span: crate::ast::Span::dummy(),
            annotation: crate::ast::Annotation::Unknown,
            ty: Type::uninferred(),
        };

        assert_eq!(enum_field_shape(&VariantFields::Unit), None);
        assert_eq!(
            enum_field_shape(&VariantFields::Tuple(vec![field.clone()])),
            Some(EnumFieldShape::TupleSingle)
        );
        assert_eq!(
            enum_field_shape(&VariantFields::Tuple(vec![field.clone(), field.clone()])),
            Some(EnumFieldShape::TupleMultiple)
        );
        assert_eq!(
            enum_field_shape(&VariantFields::Struct(vec![field])),
            Some(EnumFieldShape::Struct)
        );
    }
}

pub fn enum_field_shape(fields: &VariantFields) -> Option<EnumFieldShape> {
    match fields {
        VariantFields::Unit => None,
        VariantFields::Struct(_) => Some(EnumFieldShape::Struct),
        VariantFields::Tuple(fields) if fields.len() == 1 => Some(EnumFieldShape::TupleSingle),
        VariantFields::Tuple(_) => Some(EnumFieldShape::TupleMultiple),
    }
}

/// Whether an interface's requirements match implementations by exact source
/// spelling rather than by emitted Go name.
pub fn interface_matches_by_source_name(interface_id: &str, interface_is_public: bool) -> bool {
    !interface_id.starts_with(GO_IMPORT_PREFIX)
        && (interface_id.starts_with("prelude.") || !interface_is_public)
}

/// Resolve which implementing method satisfies an interface requirement, by
/// emitted Go name under Go's selector rules.
pub fn conformance_method<'a>(
    methods: &'a MethodSignatures,
    interface_id: &str,
    interface_is_public: bool,
    method_name: &str,
    candidate: &dyn Fn(&str) -> ConformanceCandidate,
) -> Option<(&'a EcoString, &'a Type)> {
    if interface_matches_by_source_name(interface_id, interface_is_public) {
        return methods.get_key_value(method_name);
    }
    select_by_emitted_name(methods, interface_id, method_name, candidate, false)
}

pub fn conformance_method_if_public<'a>(
    methods: &'a MethodSignatures,
    interface_id: &str,
    interface_is_public: bool,
    method_name: &str,
    candidate: &dyn Fn(&str) -> ConformanceCandidate,
) -> Option<(&'a EcoString, &'a Type)> {
    if interface_matches_by_source_name(interface_id, interface_is_public) {
        return None;
    }
    select_by_emitted_name(methods, interface_id, method_name, candidate, true)
}

fn select_by_emitted_name<'a>(
    methods: &'a MethodSignatures,
    interface_id: &str,
    method_name: &str,
    candidate: &dyn Fn(&str) -> ConformanceCandidate,
    as_if_public: bool,
) -> Option<(&'a EcoString, &'a Type)> {
    let want = if interface_id.starts_with(GO_IMPORT_PREFIX) {
        Cow::Borrowed(method_name)
    } else {
        Cow::Owned(snake_to_camel(method_name))
    };
    let mut matches: Vec<(usize, bool, Option<EcoString>, &EcoString, &Type)> = Vec::new();
    for (name, ty) in methods {
        let exact = name == method_name;
        let (exported, depth, owner, shadowed) = match candidate(name) {
            ConformanceCandidate::Unresolved => (false, 0, None, false),
            ConformanceCandidate::Resolved {
                exported,
                depth,
                owner,
                shadowed,
            } => (exported, depth, Some(owner), shadowed),
        };
        if shadowed {
            continue;
        }
        if as_if_public && (exported || exact) {
            continue;
        }
        let emitted = if exported || as_if_public {
            Cow::Owned(snake_to_camel(name))
        } else {
            Cow::Owned(snake_to_lower_camel(name))
        };
        if !exact && emitted != *want {
            continue;
        }
        matches.push((depth, exact, owner, name, ty));
    }
    let depth = matches.iter().map(|m| m.0).min()?;
    matches.retain(|m| m.0 == depth);
    if matches.iter().any(|m| m.2 != matches[0].2) {
        return None;
    }
    matches
        .into_iter()
        .min_by_key(|(_, exact, _, name, _)| (!exact, (*name).clone()))
        .map(|(_, _, _, name, ty)| (name, ty))
}

/// Go struct field name for an enum variant field. Emit's enum layout and
/// the checker's cross-variant conflict check must both use this single
/// authority so their notions of a field's Go name cannot drift.
pub fn enum_field_go_name(
    variant_name: &str,
    field_name: &str,
    field_index: usize,
    shape: EnumFieldShape,
    enum_name: &str,
) -> String {
    if shape == EnumFieldShape::Struct {
        let base = snake_to_camel(field_name);
        if base == ENUM_TAG_FIELD || base == ENUM_STRINGER_METHOD || base == ENUM_GO_STRINGER_METHOD
        {
            escape_keyword(&format!("{}{}", variant_name, base)).into_owned()
        } else {
            escape_keyword(&base).into_owned()
        }
    } else if shape == EnumFieldShape::TupleSingle {
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
    fn screaming_snake_to_camel_converts_constants() {
        assert_eq!(screaming_snake_to_camel("MAX_SIZE"), "MaxSize");
        assert_eq!(screaming_snake_to_camel("HTTP_TIMEOUT"), "HttpTimeout");
        assert_eq!(screaming_snake_to_camel("A"), "A");
        assert_eq!(screaming_snake_to_camel("MAX_SIZE_2"), "MaxSize2");
        assert_eq!(screaming_snake_to_camel("max_size"), "MaxSize");
    }

    #[test]
    fn screaming_snake_to_camel_preserves_visibility_and_tails() {
        assert_eq!(screaming_snake_to_camel("_INTERNAL"), "_Internal");
        assert_eq!(screaming_snake_to_camel("HTTPTimeout"), "HTTPTimeout");
        assert_eq!(screaming_snake_to_camel("定数"), "定数");
    }

    #[test]
    fn snake_to_lower_camel_converts_private_names() {
        assert_eq!(snake_to_lower_camel("retry_count"), "retryCount");
        assert_eq!(snake_to_lower_camel("used_private"), "usedPrivate");
        assert_eq!(snake_to_lower_camel("helper"), "helper");
        assert_eq!(snake_to_lower_camel("foo_bar_"), "fooBar");
    }

    #[test]
    fn snake_to_lower_camel_preserves_prefix_and_first_segment() {
        assert_eq!(snake_to_lower_camel("_temp_val"), "_tempVal");
        assert_eq!(snake_to_lower_camel("挨拶_する"), "挨拶する");
        assert_eq!(snake_to_lower_camel("Read"), "Read");
    }

    #[test]
    fn unexported_method_go_name_escapes_keywords() {
        assert_eq!(unexported_method_go_name("select"), "select_");
        assert_eq!(unexported_method_go_name("do_select"), "doSelect");
    }

    #[test]
    fn snake_to_camel_prefixes_uncased_names() {
        assert_eq!(snake_to_camel("挨拶"), "X挨拶");
        assert_eq!(snake_to_camel("挨拶_する"), "X挨拶する");
        assert_eq!(snake_to_camel("épée"), "Épée");
    }

    #[test]
    fn escape_keyword_appends_underscore() {
        assert_eq!(escape_keyword("type"), "type_");
        assert_eq!(escape_keyword("Type"), "Type");
        assert_eq!(escape_keyword("target"), "target");
    }

    #[test]
    fn escape_type_name_covers_keywords_and_predeclared() {
        assert_eq!(escape_type_name("range"), "range_");
        assert_eq!(escape_type_name("len"), "len_");
        assert_eq!(escape_type_name("init"), "init_");
        assert_eq!(escape_type_name("iota"), "iota_");
        assert_eq!(escape_type_name("int"), "int_");
        assert_eq!(escape_type_name("Len"), "Len");
        assert_eq!(escape_type_name("Point"), "Point");
    }

    #[test]
    fn enum_field_go_name_struct_fields() {
        assert_eq!(
            enum_field_go_name("Click", "target_id", 0, EnumFieldShape::Struct, "Event",),
            "TargetId"
        );
        assert_eq!(
            enum_field_go_name("Click", "tag", 0, EnumFieldShape::Struct, "Event"),
            "ClickTag"
        );
        assert_eq!(
            enum_field_go_name("Click", "string", 0, EnumFieldShape::Struct, "Event"),
            "ClickString"
        );
        assert_eq!(
            enum_field_go_name("Click", "go_string", 0, EnumFieldShape::Struct, "Event"),
            "ClickGoString"
        );
    }

    fn exported_at(depth: usize) -> impl Fn(&str) -> ConformanceCandidate {
        move |_| ConformanceCandidate::Resolved {
            exported: true,
            depth,
            owner: "main.T".into(),
            shadowed: false,
        }
    }

    const UNEXPORTED: fn(&str) -> ConformanceCandidate = |_| ConformanceCandidate::Resolved {
        exported: false,
        depth: 0,
        owner: "main.T".into(),
        shadowed: false,
    };

    #[test]
    fn conformance_method_matches_source_then_emitted_name() {
        let mut methods = MethodSignatures::default();
        methods.insert("read".into(), Type::Error);
        methods.insert("close".into(), Type::Error);

        let via_emitted = conformance_method(&methods, "go:io", true, "Read", &exported_at(0));
        assert_eq!(via_emitted.map(|(name, _)| name.as_str()), Some("read"));

        let private_method = conformance_method(&methods, "go:io", true, "Read", &UNEXPORTED);
        assert_eq!(private_method, None);

        let initialism =
            conformance_method(&methods, "go:net/http", true, "ServeHTTP", &exported_at(0));
        assert_eq!(initialism, None);

        methods.insert("Read".into(), Type::Error);
        let via_source = conformance_method(&methods, "go:io", true, "Read", &UNEXPORTED);
        assert_eq!(via_source.map(|(name, _)| name.as_str()), Some("Read"));
    }

    #[test]
    fn conformance_method_accepts_unresolved_candidates() {
        let mut methods = MethodSignatures::default();
        methods.insert("read".into(), Type::Error);

        let selected = conformance_method_if_public(&methods, "go:io", true, "Read", &|_| {
            ConformanceCandidate::Unresolved
        });

        assert_eq!(selected.map(|(name, _)| name.as_str()), Some("read"));
    }

    #[test]
    fn conformance_method_if_public_finds_private_near_misses() {
        let mut methods = MethodSignatures::default();
        methods.insert("write".into(), Type::Error);

        let private_hit =
            conformance_method_if_public(&methods, "go:io", true, "Write", &UNEXPORTED);
        assert_eq!(private_hit.map(|(name, _)| name.as_str()), Some("write"));

        let already_exported =
            conformance_method_if_public(&methods, "go:io", true, "Write", &exported_at(0));
        assert_eq!(already_exported, None);

        let exact_name =
            conformance_method_if_public(&methods, "main.W", true, "write", &UNEXPORTED);
        assert_eq!(exact_name, None);

        let source_matched =
            conformance_method_if_public(&methods, "main.W", false, "write", &UNEXPORTED);
        assert_eq!(source_matched, None);
    }

    #[test]
    fn conformance_method_prefers_shallow_over_exact() {
        let mut methods = MethodSignatures::default();
        methods.insert("describe".into(), Type::Error);
        methods.insert("Describe".into(), Type::Error);
        let candidate = |name: &str| ConformanceCandidate::Resolved {
            exported: true,
            depth: if name == "Describe" { 1 } else { 0 },
            owner: if name == "Describe" {
                "main.Base"
            } else {
                "main.Outer"
            }
            .into(),
            shadowed: false,
        };

        let shallow = conformance_method(&methods, "go:reg", true, "Describe", &candidate);
        assert_eq!(shallow.map(|(name, _)| name.as_str()), Some("describe"));

        let same_depth = conformance_method(&methods, "go:reg", true, "Describe", &exported_at(0));
        assert_eq!(same_depth.map(|(name, _)| name.as_str()), Some("Describe"));
    }

    #[test]
    fn conformance_method_rejects_equal_depth_cross_owner_ambiguity() {
        let mut methods = MethodSignatures::default();
        methods.insert("get_item".into(), Type::Error);
        methods.insert("getItem".into(), Type::Error);
        let promoted = |name: &str| ConformanceCandidate::Resolved {
            exported: true,
            depth: 1,
            owner: if name == "get_item" {
                "main.A"
            } else {
                "main.B"
            }
            .into(),
            shadowed: false,
        };

        let ambiguous = conformance_method(&methods, "go:reg", true, "GetItem", &promoted);
        assert_eq!(ambiguous, None);
    }

    #[test]
    fn conformance_method_skips_field_shadowed_candidates() {
        let mut methods = MethodSignatures::default();
        methods.insert("getItem".into(), Type::Error);
        let shadowed = |_: &str| ConformanceCandidate::Resolved {
            exported: true,
            depth: 1,
            owner: "main.Base".into(),
            shadowed: true,
        };

        let hidden = conformance_method(&methods, "go:reg", true, "GetItem", &shadowed);
        assert_eq!(hidden, None);
    }

    #[test]
    fn conformance_method_gates_on_interface_kind() {
        let mut methods = MethodSignatures::default();
        methods.insert("run".into(), Type::Error);

        let public_lisette =
            conformance_method(&methods, "main.Runner", true, "Run", &exported_at(0));
        assert_eq!(public_lisette.map(|(name, _)| name.as_str()), Some("run"));

        let private_lisette =
            conformance_method(&methods, "main.Runner", false, "Run", &exported_at(0));
        assert_eq!(private_lisette, None);

        let prelude = conformance_method(&methods, "prelude.Runner", true, "Run", &exported_at(0));
        assert_eq!(prelude, None);
    }

    #[test]
    fn enum_field_go_name_tuple_fields() {
        assert_eq!(
            enum_field_go_name("Click", "0", 0, EnumFieldShape::TupleSingle, "Event"),
            "Click"
        );
        assert_eq!(
            enum_field_go_name("Tag", "0", 0, EnumFieldShape::TupleSingle, "Event"),
            "EventTag_"
        );
        assert_eq!(
            enum_field_go_name("String", "0", 0, EnumFieldShape::TupleSingle, "Event"),
            "EventString_"
        );
        assert_eq!(
            enum_field_go_name("Click", "1", 1, EnumFieldShape::TupleMultiple, "Event"),
            "Click1"
        );
    }
}
