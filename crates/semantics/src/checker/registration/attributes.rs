use super::*;
use crate::checker::sealing::unexported_key;

const KNOWN_GO_HINTS: &[&str] = &[
    "anon_struct",
    "bit_flag_set",
    "closed_domain",
    "collapsed_type_params",
    "comma_ok",
    "comparable_if",
    "constraint",
    "hidden_embed",
    "hidden_fields",
    "incomparable",
    "read_only",
    "sentinel_minus_one",
    "superseded_by",
    "unexported",
    "value_method_set",
    "zero_safe",
    "zero_unsafe",
];

pub(super) fn extract_go_name(attributes: &[Attribute]) -> Option<String> {
    attributes
        .iter()
        .filter(|a| a.name == "go")
        .filter(|a| {
            !a.args
                .iter()
                .any(|arg| matches!(arg, AttributeArg::Flag(_)))
        })
        .find_map(|a| {
            a.args.iter().find_map(|arg| match arg {
                AttributeArg::String(name) => Some(name.clone()),
                _ => None,
            })
        })
}

/// The recipe string from `#[go(collapsed_type_params, "...")]`. This is
/// Go's full type-param in declaration order, each entry as a Lisette type.
pub(super) fn extract_go_type_param_recipe(attributes: &[Attribute]) -> Option<String> {
    extract_go_flag_string(attributes, "collapsed_type_params")
}

pub(super) fn extract_go_superseded_by(attributes: &[Attribute]) -> Option<String> {
    extract_go_flag_string(attributes, "superseded_by")
}

/// The string argument of `#[go(<flag>, "...")]`.
fn extract_go_flag_string(attributes: &[Attribute], flag: &str) -> Option<String> {
    attributes
        .iter()
        .filter(|a| a.name == "go")
        .filter(|a| {
            a.args
                .iter()
                .any(|arg| matches!(arg, AttributeArg::Flag(f) if f == flag))
        })
        .find_map(|a| {
            a.args.iter().find_map(|arg| match arg {
                AttributeArg::String(value) => Some(value.clone()),
                _ => None,
            })
        })
}

pub(super) fn has_display_attribute(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "display")
}

pub(super) fn has_go_flag(attributes: &[Attribute], flag: &str) -> bool {
    attributes
        .iter()
        .filter(|attribute| attribute.name == "go")
        .flat_map(|attribute| &attribute.args)
        .any(|argument| matches!(argument, AttributeArg::Flag(name) if name == flag))
}

pub(super) fn has_serialization_attribute(attributes: &[Attribute]) -> bool {
    attributes.iter().any(struct_attribute_forces_field_export)
}

pub(super) fn collect_interface_attributes(attributes: &[Attribute]) -> Attributes {
    let mut map = Attributes::default();
    if has_go_flag(attributes, "constraint") {
        map.insert(TypeAttribute::Constraint);
    }
    map
}

pub(super) fn collect_enum_attributes(attributes: &[Attribute]) -> Attributes {
    let mut map = Attributes::default();
    if has_display_attribute(attributes) {
        map.insert(TypeAttribute::Display);
    }
    map
}

pub(super) fn collect_struct_attributes(
    attributes: &[Attribute],
    generics: &[Generic],
) -> Attributes {
    let mut map = Attributes::default();
    if let Some(names) = extract_go_flag_string(attributes, "comparable_if") {
        for name in names.split(',').map(str::trim) {
            if let Some(position) = generics.iter().position(|g| g.name == name) {
                map.require_comparable_argument(position);
            }
        }
    }
    if has_display_attribute(attributes) {
        map.insert(TypeAttribute::Display);
    }
    if has_serialization_attribute(attributes) {
        map.insert(TypeAttribute::Serialized);
    }
    for flag in extract_attribute_flags(attributes, "go") {
        let attribute = match flag.as_str() {
            "closed_domain" => TypeAttribute::ClosedDomain,
            "anon_struct" => TypeAttribute::AnonStruct,
            "hidden_embed" => TypeAttribute::HiddenEmbed,
            "zero_safe" => TypeAttribute::ZeroSafe,
            "zero_unsafe" => TypeAttribute::ZeroUnsafe,
            "hidden_fields" => TypeAttribute::HiddenFields,
            "incomparable" => TypeAttribute::Incomparable,
            "constraint" => TypeAttribute::Constraint,
            _ => continue,
        };
        map.insert(attribute);
    }
    map
}

pub(super) fn check_go_hints(attributes: &[Attribute], sink: &diagnostics::LocalSink) {
    for attribute in attributes.iter().filter(|a| a.name == "go") {
        for arg in &attribute.args {
            if let AttributeArg::Flag(flag) = arg
                && !KNOWN_GO_HINTS.contains(&flag.as_str())
            {
                sink.push(diagnostics::attribute::unknown_go_hint(
                    &attribute.span,
                    flag,
                ));
            }
        }
    }
}

pub(super) fn extract_attribute_flags(attributes: &[Attribute], name: &str) -> Vec<String> {
    attributes
        .iter()
        .filter(|a| a.name == name)
        .flat_map(|a| {
            a.args.iter().filter_map(|arg| {
                if let AttributeArg::Flag(name) = arg {
                    Some(name.clone())
                } else {
                    None
                }
            })
        })
        .collect()
}

pub(super) fn extract_attribute_string(attributes: &[Attribute], name: &str) -> Option<String> {
    attributes.iter().filter(|a| a.name == name).find_map(|a| {
        a.args.iter().find_map(|arg| match arg {
            AttributeArg::String(s) => Some(s.clone()),
            _ => None,
        })
    })
}

pub(super) fn seal_method_key(
    is_d_lis: bool,
    attributes: &[Attribute],
    package_id: &str,
    name: &str,
) -> ecow::EcoString {
    let id = if is_d_lis {
        extract_attribute_string(attributes, "go").unwrap_or_else(|| format!("{package_id}.{name}"))
    } else {
        format!("{package_id}.{name}")
    };
    unexported_key(&id)
}
