use super::*;

const KNOWN_GO_HINTS: &[&str] = &[
    "anon_struct",
    "bit_flag_set",
    "closed_domain",
    "collapsed_type_params",
    "comma_ok",
    "hidden_embed",
    "hidden_fields",
    "sentinel_minus_one",
    "unexported",
    "value_method_set",
    "zero_safe",
];

pub(crate) fn extract_package_directive(source: &str) -> Option<String> {
    for line in source.lines().take(10) {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("// Package:") {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        if !line.starts_with("//") && !line.is_empty() {
            break;
        }
    }
    None
}

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
    attributes
        .iter()
        .filter(|a| a.name == "go")
        .filter(|a| {
            a.args
                .iter()
                .any(|arg| matches!(arg, AttributeArg::Flag(f) if f == "collapsed_type_params"))
        })
        .find_map(|a| {
            a.args.iter().find_map(|arg| match arg {
                AttributeArg::String(recipe) => Some(recipe.clone()),
                _ => None,
            })
        })
}

pub(super) fn has_display_attribute(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "display")
}

pub(super) fn has_closed_domain_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "closed_domain")
}

pub(super) fn has_anon_struct_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "anon_struct")
}

pub(super) fn has_hidden_embed_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "hidden_embed")
}

pub(super) fn has_zero_safe_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "zero_safe")
}

pub(super) fn has_hidden_fields_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "hidden_fields")
}

pub(super) fn has_unexported_attribute(attributes: &[Attribute]) -> bool {
    extract_attribute_flags(attributes, "go")
        .iter()
        .any(|flag| flag == "unexported")
}

pub(super) fn has_serialization_attribute(attributes: &[Attribute]) -> bool {
    attributes.iter().any(struct_attribute_forces_field_export)
}

pub(super) fn collect_enum_attributes(attributes: &[Attribute]) -> Attributes {
    let mut map = Attributes::default();
    if has_display_attribute(attributes) {
        map.insert(TypeAttribute::Display);
    }
    map
}

pub(super) fn collect_struct_attributes(attributes: &[Attribute]) -> Attributes {
    let mut map = Attributes::default();
    if has_display_attribute(attributes) {
        map.insert(TypeAttribute::Display);
    }
    if has_closed_domain_attribute(attributes) {
        map.insert(TypeAttribute::ClosedDomain);
    }
    if has_anon_struct_attribute(attributes) {
        map.insert(TypeAttribute::AnonStruct);
    }
    if has_hidden_embed_attribute(attributes) {
        map.insert(TypeAttribute::HiddenEmbed);
    }
    if has_serialization_attribute(attributes) {
        map.insert(TypeAttribute::Serialized);
    }
    if has_zero_safe_attribute(attributes) {
        map.insert(TypeAttribute::ZeroSafe);
    }
    if has_hidden_fields_attribute(attributes) {
        map.insert(TypeAttribute::HiddenFields);
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
    module_id: &str,
    name: &str,
) -> ecow::EcoString {
    let id = if is_d_lis {
        extract_attribute_string(attributes, "go").unwrap_or_else(|| format!("{module_id}.{name}"))
    } else {
        format!("{module_id}.{name}")
    };
    crate::checker::sealing::unexported_key(&id)
}
