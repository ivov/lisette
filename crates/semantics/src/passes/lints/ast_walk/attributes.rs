use diagnostics::LisetteDiagnostic;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Attribute, AttributeArg, Expression, StructFieldDefinition};

pub(crate) const SERIALIZATION_KEYS: &[&str] = &[
    "json",
    "xml",
    "yaml",
    "toml",
    "db",
    "bson",
    "mapstructure",
    "msgpack",
];

pub fn check_attributes(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let attributes = match expression {
        Expression::Function { attributes, .. } => attributes,
        _ => return,
    };

    for attribute in attributes {
        check_unknown_attribute(attribute, diagnostics);
    }
}

pub fn check_struct_attributes(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Struct {
        attributes: struct_attributes,
        fields,
        ..
    } = expression
    else {
        return;
    };

    for attribute in struct_attributes {
        check_unknown_attribute(attribute, diagnostics);
        check_unknown_tag_options(attribute, diagnostics);
        check_conflicting_case_transforms(attribute, diagnostics);
    }

    let struct_keys: HashSet<&str> = struct_attributes
        .iter()
        .filter_map(|a| get_attribute_key(a))
        .filter(|k| is_serialization_key(k))
        .collect();

    for field in fields {
        check_field_attributes(field, &struct_keys, diagnostics);
    }
}

fn check_unknown_attribute(attribute: &Attribute, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let name = &attribute.name;

    if !is_known_attribute(name) {
        diagnostics.push(diagnostics::lint::unknown_attribute(&attribute.span, name));
    }
}

fn check_field_attributes(
    field: &StructFieldDefinition,
    struct_keys: &HashSet<&str>,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let mut seen_keys: Vec<(&str, &Attribute)> = Vec::new();

    for attribute in &field.attributes {
        let attribute_key = get_attribute_key(attribute);

        check_unknown_attribute(attribute, diagnostics);
        check_unknown_tag_options(attribute, diagnostics);

        if let Some(key) = attribute_key
            && is_serialization_key(key)
            && !struct_keys.contains(key)
        {
            diagnostics.push(
                diagnostics::attribute::field_attribute_without_struct_attribute(
                    &attribute.span,
                    key,
                ),
            );
        }

        if let Some(key) = attribute_key {
            if let Some((_, first_attribute)) = seen_keys.iter().find(|(k, _)| *k == key) {
                diagnostics.push(diagnostics::attribute::duplicate_tag_key(
                    &attribute.span,
                    key,
                    &first_attribute.span,
                ));
            } else {
                seen_keys.push((key, attribute));
            }
        }

        // Check for conflicting case transforms
        check_conflicting_case_transforms(attribute, diagnostics);

        // Check for raw tags that should use predefined aliases
        check_tag_with_alias(attribute, diagnostics);
    }
}

/// Gets the effective key for an attribute (for deduplication).
/// Returns None if the key cannot be determined (should not happen for valid attributes).
fn get_attribute_key(attribute: &Attribute) -> Option<&str> {
    if attribute.name == "tag" {
        // For #[tag], the key is the first argument
        match attribute.args.first() {
            // Structured mode: #[tag("json", ...)]
            Some(AttributeArg::String(key)) => Some(key),
            // Raw mode: #[tag(`json:"name"`)] - extract key before colon
            Some(AttributeArg::Raw(raw)) => extract_key_from_raw(raw),
            _ => None,
        }
    } else {
        Some(&attribute.name)
    }
}

/// Extracts the tag key from a raw tag value like `json:"name"`.
fn extract_key_from_raw(raw: &str) -> Option<&str> {
    // Format is: key:"value" or key:"value,options"
    raw.split(':').next().filter(|k| !k.is_empty())
}

/// Known tag options.
const KNOWN_TAG_OPTIONS: &[&str] = &["snake_case", "camel_case", "omitempty", "skip", "string"];

fn check_unknown_tag_options(attribute: &Attribute, diagnostics: &mut Vec<LisetteDiagnostic>) {
    // Only check serialization attributes (json, db, etc.) and structured #[tag("key", ...)]
    let is_serialization = is_serialization_key(&attribute.name);
    let is_structured_tag = attribute.name == "tag"
        && attribute
            .args
            .first()
            .map(|a| matches!(a, AttributeArg::String(_)))
            .unwrap_or(false);

    if !is_serialization && !is_structured_tag {
        return;
    }

    // For structured tag, skip the first argument (key name) and second if it's a name override
    let skip_count = if is_structured_tag { 1 } else { 0 };

    for (i, arg) in attribute.args.iter().enumerate() {
        // Skip the key (and potential name override) for structured tags
        if is_structured_tag && i < skip_count {
            continue;
        }

        match arg {
            AttributeArg::Flag(flag) => {
                if !KNOWN_TAG_OPTIONS.contains(&flag.as_str()) {
                    diagnostics.push(diagnostics::lint::unknown_tag_option(&attribute.span, flag));
                }
            }
            AttributeArg::NegatedFlag(flag) => {
                // Only omitempty can be negated
                if flag != "omitempty" {
                    diagnostics.push(diagnostics::lint::unknown_tag_option(
                        &attribute.span,
                        &format!("!{}", flag),
                    ));
                }
            }
            // String and Raw args are valid (name override and raw values)
            _ => {}
        }
    }
}

fn check_conflicting_case_transforms(
    attribute: &Attribute,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let mut has_snake_case = false;
    let mut has_camel_case = false;

    for arg in &attribute.args {
        if let AttributeArg::Flag(flag) = arg {
            match flag.as_str() {
                "snake_case" => has_snake_case = true,
                "camel_case" => has_camel_case = true,
                _ => {}
            }
        }
    }

    if has_snake_case && has_camel_case {
        diagnostics.push(diagnostics::attribute::conflicting_case_transforms(
            &attribute.span,
        ));
    }
}

/// Checks if a #[tag(...)] uses a key that has a predefined alias.
fn check_tag_with_alias(attribute: &Attribute, diagnostics: &mut Vec<LisetteDiagnostic>) {
    // Only check #[tag(...)] attributes
    if attribute.name != "tag" {
        return;
    }

    let key = match attribute.args.first() {
        // Raw mode: #[tag(`json:"name"`)]
        Some(AttributeArg::Raw(raw)) => extract_key_from_raw(raw),
        // Structured mode: #[tag("json", ...)]
        Some(AttributeArg::String(s)) => Some(s.as_str()),
        _ => None,
    };

    if let Some(key) = key
        && is_serialization_key(key)
    {
        diagnostics.push(diagnostics::lint::tag_has_alias(&attribute.span, key));
    }
}

fn is_known_attribute(name: &str) -> bool {
    is_serialization_key(name) || name == "tag" || name == "allow"
}

fn is_serialization_key(key: &str) -> bool {
    SERIALIZATION_KEYS.contains(&key)
}
