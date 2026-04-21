use syntax::ast::{Attribute, AttributeArg, StructFieldDefinition};

#[derive(Default)]
pub(super) struct TagConfig {
    pub(super) key: String,
    pub(super) name_override: Option<String>,
    pub(super) case_transform: Option<CaseTransform>,
    pub(super) omitempty: bool,
    pub(super) skip: bool,
    pub(super) string_encoding: bool,
    pub(super) raw_value: Option<String>,
}

impl TagConfig {
    fn merge_from(&mut self, other: TagConfig) {
        if other.case_transform.is_some() {
            self.case_transform = other.case_transform;
        }
        if other.omitempty {
            self.omitempty = true;
        }
        if other.skip {
            self.skip = true;
        }
        if other.string_encoding {
            self.string_encoding = true;
        }
        if other.name_override.is_some() {
            self.name_override = other.name_override;
        }
        if other.raw_value.is_some() {
            self.raw_value = other.raw_value;
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum CaseTransform {
    SnakeCase,
    CamelCase,
}

pub(super) fn interpret_field_attributes(
    field: &StructFieldDefinition,
    struct_attrs: &[Attribute],
) -> Vec<TagConfig> {
    let mut configs = Vec::new();

    let mut struct_defaults: Vec<TagConfig> = Vec::new();
    for attribute in struct_attrs {
        if let Some(config) = interpret_struct_attribute(attribute) {
            if let Some(existing) = struct_defaults.iter_mut().find(|c| c.key == config.key) {
                existing.merge_from(config);
            } else {
                struct_defaults.push(config);
            }
        }
    }

    for attribute in &field.attributes {
        if let Some(config) = interpret_field_attribute(attribute, &struct_defaults) {
            configs.push(config);
        }
    }

    for mut default in struct_defaults {
        if !configs.iter().any(|c| c.key == default.key) {
            default.name_override = None;
            configs.push(default);
        }
    }

    configs
}

fn interpret_struct_attribute(attribute: &Attribute) -> Option<TagConfig> {
    let key = &attribute.name;

    if key == "tag" {
        return interpret_struct_tag_attribute(attribute);
    }

    if !is_serialization_key(key) {
        return None;
    }

    let mut config = TagConfig {
        key: key.clone(),
        ..Default::default()
    };

    for arg in &attribute.args {
        match arg {
            AttributeArg::Flag(flag) => match flag.as_str() {
                "snake_case" => config.case_transform = Some(CaseTransform::SnakeCase),
                "camel_case" => config.case_transform = Some(CaseTransform::CamelCase),
                "omitempty" => config.omitempty = true,
                _ => {}
            },
            AttributeArg::NegatedFlag(flag) => {
                if flag == "omitempty" {
                    config.omitempty = false;
                }
            }
            _ => {}
        }
    }

    Some(config)
}

fn interpret_struct_tag_attribute(attribute: &Attribute) -> Option<TagConfig> {
    if attribute.args.is_empty() {
        return None;
    }

    let AttributeArg::String(key) = &attribute.args[0] else {
        return None;
    };

    let mut config = TagConfig {
        key: key.clone(),
        ..Default::default()
    };

    for arg in attribute.args.iter().skip(1) {
        match arg {
            AttributeArg::Flag(flag) => match flag.as_str() {
                "snake_case" => config.case_transform = Some(CaseTransform::SnakeCase),
                "camel_case" => config.case_transform = Some(CaseTransform::CamelCase),
                "omitempty" => config.omitempty = true,
                _ => {}
            },
            AttributeArg::NegatedFlag(flag) => {
                if flag == "omitempty" {
                    config.omitempty = false;
                }
            }
            _ => {}
        }
    }

    Some(config)
}

fn interpret_field_attribute(
    attribute: &Attribute,
    struct_defaults: &[TagConfig],
) -> Option<TagConfig> {
    let key = &attribute.name;

    if key == "tag" {
        return interpret_tag_attribute(attribute);
    }

    if !is_serialization_key(key) {
        return None;
    }

    let mut config = struct_defaults
        .iter()
        .find(|c| c.key == *key)
        .map(|c| TagConfig {
            key: c.key.clone(),
            case_transform: c.case_transform,
            omitempty: c.omitempty,
            ..Default::default()
        })
        .unwrap_or_else(|| TagConfig {
            key: key.clone(),
            ..Default::default()
        });

    for arg in &attribute.args {
        match arg {
            AttributeArg::Flag(flag) => match flag.as_str() {
                "snake_case" => config.case_transform = Some(CaseTransform::SnakeCase),
                "camel_case" => config.case_transform = Some(CaseTransform::CamelCase),
                "omitempty" => config.omitempty = true,
                "skip" => config.skip = true,
                "string" => config.string_encoding = true,
                _ => {}
            },
            AttributeArg::NegatedFlag(flag) => {
                if flag == "omitempty" {
                    config.omitempty = false;
                }
            }
            AttributeArg::String(name) => {
                config.name_override = Some(name.clone());
            }
            AttributeArg::Raw(raw) => {
                config.raw_value = Some(raw.clone());
            }
        }
    }

    Some(config)
}

fn interpret_tag_attribute(attribute: &Attribute) -> Option<TagConfig> {
    if attribute.args.is_empty() {
        return None;
    }

    let first_arg = &attribute.args[0];

    match first_arg {
        AttributeArg::Raw(raw) => {
            let key = raw
                .split(':')
                .next()
                .filter(|k| !k.is_empty())
                .unwrap_or("tag")
                .to_string();
            Some(TagConfig {
                key,
                raw_value: Some(raw.clone()),
                ..Default::default()
            })
        }

        AttributeArg::String(key) => {
            let mut config = TagConfig {
                key: key.clone(),
                ..Default::default()
            };

            for (i, arg) in attribute.args.iter().enumerate().skip(1) {
                match arg {
                    AttributeArg::String(name) if i == 1 => {
                        config.name_override = Some(name.clone());
                    }
                    AttributeArg::Flag(flag) => match flag.as_str() {
                        "snake_case" => config.case_transform = Some(CaseTransform::SnakeCase),
                        "camel_case" => config.case_transform = Some(CaseTransform::CamelCase),
                        "omitempty" => config.omitempty = true,
                        "skip" => config.skip = true,
                        _ => {}
                    },
                    AttributeArg::NegatedFlag(flag) => {
                        if flag == "omitempty" {
                            config.omitempty = false;
                        }
                    }
                    _ => {}
                }
            }

            Some(config)
        }

        _ => None,
    }
}

fn is_serialization_key(key: &str) -> bool {
    matches!(
        key,
        "json" | "xml" | "yaml" | "toml" | "db" | "bson" | "mapstructure" | "msgpack"
    )
}

pub(super) fn format_tag_string(
    field_name: &str,
    configs: &[TagConfig],
    needs_omitzero: bool,
) -> Option<String> {
    if configs.is_empty() {
        return None;
    }

    let mut sorted_configs: Vec<&TagConfig> = configs.iter().collect();
    sorted_configs.sort_by_key(|config| tag_sort_key(config));

    let parts: Vec<String> = sorted_configs
        .iter()
        .filter_map(|config| format_single_tag(field_name, config, needs_omitzero))
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(format!("`{}`", parts.join(" ")))
    }
}

fn tag_sort_key(config: &TagConfig) -> (u8, &str) {
    if config.raw_value.is_some() {
        return (3, &config.key);
    }

    match config.key.as_str() {
        "json" => (0, ""),
        "db" => (1, ""),
        _ => (2, &config.key),
    }
}

fn format_single_tag(field_name: &str, config: &TagConfig, needs_omitzero: bool) -> Option<String> {
    if let Some(ref raw) = config.raw_value {
        let key_prefix = format!("{}:", config.key);
        if raw.starts_with(&key_prefix) {
            return Some(raw.clone());
        }
        return Some(format!("{}:{}", config.key, raw));
    }

    if config.skip {
        return Some(format!("{}:\"-\"", config.key));
    }

    let name = if let Some(ref override_name) = config.name_override {
        override_name.clone()
    } else {
        apply_case_transform(field_name, config.case_transform)
    };

    let mut options = Vec::new();
    if config.omitempty {
        options.push("omitempty");
        if needs_omitzero {
            options.push("omitzero");
        }
    }
    if config.string_encoding {
        options.push("string");
    }

    let value = if options.is_empty() {
        name
    } else {
        format!("{},{}", name, options.join(","))
    };

    Some(format!("{}:\"{}\"", config.key, value))
}

fn apply_case_transform(name: &str, transform: Option<CaseTransform>) -> String {
    match transform {
        Some(CaseTransform::SnakeCase) => to_snake_case(name),
        Some(CaseTransform::CamelCase) => to_camel_case(name),
        None => name.to_string(),
    }
}

fn to_snake_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for i in 0..len {
        let c = chars[i];

        if c == '_' {
            result.push('_');
            continue;
        }

        if c.is_uppercase() {
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            let next_lower = i + 1 < len && chars[i + 1].is_lowercase();

            if (i > 0 && !prev_upper) || (i > 1 && prev_upper && next_lower) {
                result.push('_');
            }

            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for (i, c) in s.chars().enumerate() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else if i == 0 {
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}
