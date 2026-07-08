//! Canonical catalog of recognized attributes, shared by the `unknown_attribute`
//! lint and LSP completions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeTarget {
    Struct,
    StructField,
    Enum,
    Function,
    Method,
}

pub struct AttributeInfo {
    pub name: &'static str,
    pub detail: &'static str,
    targets: &'static [AttributeTarget],
}

impl AttributeInfo {
    pub fn applies_to(&self, target: AttributeTarget) -> bool {
        self.targets.contains(&target)
    }
}

use AttributeTarget::*;

pub const SERIALIZATION_KEYS: &[&str] = &[
    "json",
    "xml",
    "yaml",
    "toml",
    "db",
    "bson",
    "mapstructure",
    "msgpack",
];

/// In `unknown_attribute` display order. `#[go(...)]` is accepted (see
/// [`is_known_attribute`]) but deliberately not advertised here.
pub const ATTRIBUTES: &[AttributeInfo] = &[
    AttributeInfo {
        name: "json",
        detail: "JSON serialization tag",
        // Enums support `#[json]` (Marshal/UnmarshalJSON); other keys do not.
        targets: &[Struct, StructField, Enum],
    },
    AttributeInfo {
        name: "xml",
        detail: "XML serialization tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "yaml",
        detail: "YAML serialization tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "toml",
        detail: "TOML serialization tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "db",
        detail: "database column tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "bson",
        detail: "BSON serialization tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "mapstructure",
        detail: "mapstructure decoding tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "msgpack",
        detail: "MessagePack serialization tag",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "tag",
        detail: "Go struct tag (struct-level defaults or per-field)",
        targets: &[Struct, StructField],
    },
    AttributeInfo {
        name: "allow",
        detail: "suppress a lint",
        targets: &[Function, Method],
    },
    AttributeInfo {
        name: "iterate",
        detail: "synthesize `variants` for an enum",
        targets: &[Enum],
    },
    AttributeInfo {
        name: "display",
        detail: "derive `to_string`",
        targets: &[Struct, Enum],
    },
    AttributeInfo {
        name: "equality",
        detail: "derive `equals`",
        targets: &[Struct, Enum],
    },
    AttributeInfo {
        name: "test",
        detail: "mark a test function",
        targets: &[Function],
    },
];

/// `go` is accepted but absent from [`ATTRIBUTES`] (the Go-interop rail).
pub fn is_known_attribute(name: &str) -> bool {
    name == "go" || ATTRIBUTES.iter().any(|a| a.name == name)
}

pub fn is_serialization_key(key: &str) -> bool {
    SERIALIZATION_KEYS.contains(&key)
}

pub fn struct_attribute_forces_field_export(attribute: &crate::ast::Attribute) -> bool {
    if attribute.name == "tag" {
        return matches!(
            attribute.args.first(),
            Some(crate::ast::AttributeArg::String(_))
        );
    }
    is_serialization_key(&attribute.name)
}

pub fn known_attribute_names() -> Vec<&'static str> {
    ATTRIBUTES.iter().map(|a| a.name).collect()
}

pub fn attributes_for(target: AttributeTarget) -> impl Iterator<Item = &'static AttributeInfo> {
    ATTRIBUTES.iter().filter(move |a| a.applies_to(target))
}

pub fn test_attribute(attributes: &[crate::ast::Attribute]) -> Option<&crate::ast::Attribute> {
    attributes.iter().find(|a| a.name == "test")
}

pub fn has_test_attribute(attributes: &[crate::ast::Attribute]) -> bool {
    test_attribute(attributes).is_some()
}
