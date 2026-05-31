use rustc_hash::FxHashMap as HashMap;

use crate::definitions::structs::stringer_verb;
use crate::names::go_name;
use crate::utils::receiver_name;
use syntax::ast::{EnumVariant, Generic};

pub(crate) const ENUM_TAG_FIELD: &str = "Tag";

pub(crate) const ENUM_STRINGER_METHOD: &str = "String";
pub(crate) const ENUM_GO_STRINGER_METHOD: &str = "GoString";

#[derive(Debug, Clone)]
pub(crate) struct EnumLayout {
    pub(crate) enum_name: String,
    pub(crate) tag_type: String,
    pub(crate) variants: Vec<VariantLayout>,
    pub(crate) generics: Vec<Generic>,
}

#[derive(Debug, Clone)]
pub(crate) struct VariantLayout {
    pub(crate) name: String,
    pub(crate) tag_constant: String,
    pub(crate) is_struct_variant: bool,
    pub(crate) fields: Vec<FieldLayout>,
    pub(crate) doc: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldLayout {
    pub(crate) source_name: String,
    pub(crate) go_name: String,
    pub(crate) go_type: String,
    pub(crate) is_function: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldTypeInfo {
    pub(crate) go_type: String,
    pub(crate) is_function: bool,
}

pub(crate) type FieldTypeMap = HashMap<(usize, usize), FieldTypeInfo>;

impl EnumLayout {
    pub(crate) fn new(
        enum_id: &str,
        generics: &[Generic],
        variants: &[EnumVariant],
        field_types: &FieldTypeMap,
    ) -> Self {
        let enum_name = go_name::unqualified_name(enum_id).to_string();
        let tag_type = format!("{}Tag", enum_name);

        let variants = variants
            .iter()
            .enumerate()
            .map(|(vi, v)| Self::compute_variant_layout(vi, v, &enum_name, field_types))
            .collect();

        Self {
            enum_name,
            tag_type,
            variants,
            generics: generics.to_vec(),
        }
    }

    fn compute_variant_layout(
        variant_index: usize,
        variant: &EnumVariant,
        enum_name: &str,
        field_types: &FieldTypeMap,
    ) -> VariantLayout {
        let tag_constant = if variant.name == ENUM_TAG_FIELD {
            format!("{}Tag_", enum_name)
        } else {
            format!("{}{}", enum_name, variant.name)
        };

        let is_struct = variant.fields.is_struct();
        let single_field = variant.fields.len() == 1;

        let fields = variant
            .fields
            .iter()
            .enumerate()
            .map(|(fi, field)| {
                let source_name = if is_struct {
                    field.name.to_string()
                } else {
                    fi.to_string()
                };

                let go_name = Self::compute_field_go_name(
                    &variant.name,
                    &field.name,
                    fi,
                    is_struct,
                    single_field,
                    enum_name,
                );

                let info = field_types.get(&(variant_index, fi));
                let go_type = info
                    .map(|i| i.go_type.clone())
                    .unwrap_or_else(|| "any".to_string());
                let is_function = info.is_some_and(|i| i.is_function);

                FieldLayout {
                    source_name,
                    go_name,
                    go_type,
                    is_function,
                }
            })
            .collect();

        VariantLayout {
            name: variant.name.to_string(),
            tag_constant,
            is_struct_variant: is_struct,
            fields,
            doc: variant.doc.clone(),
        }
    }

    fn compute_field_go_name(
        variant_name: &str,
        field_name: &str,
        field_index: usize,
        is_struct: bool,
        single_field: bool,
        enum_name: &str,
    ) -> String {
        if is_struct {
            let base = go_name::snake_to_camel(field_name);
            if base == ENUM_TAG_FIELD
                || base == ENUM_STRINGER_METHOD
                || base == ENUM_GO_STRINGER_METHOD
            {
                go_name::escape_keyword(&format!("{}{}", variant_name, base)).into_owned()
            } else {
                go_name::escape_keyword(&base).into_owned()
            }
        } else if single_field {
            let base = variant_name.to_string();
            if base == ENUM_TAG_FIELD
                || base == ENUM_STRINGER_METHOD
                || base == ENUM_GO_STRINGER_METHOD
            {
                format!("{}{}_", enum_name, base)
            } else {
                base
            }
        } else {
            let base = format!("{}{}", variant_name, field_index);
            if base == ENUM_TAG_FIELD
                || base == ENUM_STRINGER_METHOD
                || base == ENUM_GO_STRINGER_METHOD
            {
                format!("{}{}_{}", enum_name, variant_name, field_index)
            } else {
                base
            }
        }
    }

    pub(crate) fn get_variant(&self, name: &str) -> Option<&VariantLayout> {
        self.variants.iter().find(|v| v.name == name)
    }

    pub(crate) fn struct_field_name(&self, variant_name: &str, field_name: &str) -> Option<String> {
        let variant = self.get_variant(variant_name)?;
        variant
            .fields
            .iter()
            .find(|f| f.source_name == field_name)
            .map(|f| f.go_name.clone())
    }

    pub(crate) fn tuple_field_name(&self, variant_name: &str, index: usize) -> Option<String> {
        let variant = self.get_variant(variant_name)?;
        variant.fields.get(index).map(|f| f.go_name.clone())
    }

    pub(crate) fn emit_definition(&self, generics_string: &str) -> String {
        let mut output = Vec::new();

        output.push(format!("type {} int", self.tag_type));
        output.push("const (".to_string());

        for (i, variant) in self.variants.iter().enumerate() {
            if let Some(doc) = &variant.doc {
                for line in doc.lines() {
                    if line.is_empty() {
                        output.push("//".to_string());
                    } else {
                        output.push(format!("// {}", line));
                    }
                }
            }

            if i == 0 {
                output.push(format!("{} {} = iota", variant.tag_constant, self.tag_type));
            } else {
                output.push(variant.tag_constant.clone());
            }
        }

        output.push(")".to_string());

        let go_type_name = go_name::escape_keyword(&self.enum_name);
        output.push(format!(
            "type {}{} struct {{",
            go_type_name, generics_string
        ));
        output.push(format!("Tag {}", self.tag_type));

        let mut seen_fields = rustc_hash::FxHashSet::default();
        for variant in &self.variants {
            for field in &variant.fields {
                if seen_fields.insert(&field.go_name) {
                    output.push(format!("{} {}", field.go_name, field.go_type));
                }
            }
        }

        output.push("}".to_string());

        output.join("\n")
    }

    pub(crate) fn emit_stringer_method(
        &self,
        receiver_generics: &str,
        method_name: &str,
        qualified: bool,
    ) -> String {
        let receiver = receiver_name(&self.enum_name);
        let go_type_name = go_name::escape_keyword(&self.enum_name);
        let receiver_type = format!("{}{}", go_type_name, receiver_generics);

        let mut lines = Vec::new();
        lines.push(format!(
            "func ({receiver} {receiver_type}) {method_name}() string {{"
        ));
        lines.push(format!("switch {receiver}.Tag {{"));

        for variant in &self.variants {
            lines.push(format!("case {}:", variant.tag_constant));
            lines.push(self.build_variant_stringer_line(variant, &receiver, qualified));
        }

        lines.push("default:".to_string());
        lines.push(format!(
            "return fmt.Sprintf(\"{}(%d)\", {receiver}.Tag)",
            self.enum_name
        ));
        lines.push("}".to_string());
        lines.push("}".to_string());

        lines.join("\n")
    }

    fn build_variant_stringer_line(
        &self,
        variant: &VariantLayout,
        receiver: &str,
        qualified: bool,
    ) -> String {
        let prefix = if qualified {
            format!("{}.", self.enum_name)
        } else {
            String::new()
        };
        if variant.fields.is_empty() {
            return format!("return \"{}{}\"", prefix, variant.name);
        }
        let args: Vec<String> = variant
            .fields
            .iter()
            .map(|f| format!("{receiver}.{}", f.go_name))
            .collect();
        let (open, close, placeholders) = if variant.is_struct_variant {
            let parts: Vec<String> = variant
                .fields
                .iter()
                .map(|f| format!("{}: {}", f.source_name, stringer_verb(f.is_function)))
                .collect();
            (" { ", " }", parts.join(", "))
        } else {
            let parts: Vec<&str> = variant
                .fields
                .iter()
                .map(|f| stringer_verb(f.is_function))
                .collect();
            ("(", ")", parts.join(", "))
        };
        format!(
            "return fmt.Sprintf(\"{}{}{}{}{}\", {})",
            prefix,
            variant.name,
            open,
            placeholders,
            close,
            args.join(", ")
        )
    }

    pub(crate) fn emit_variants_function(&self, fn_name: &str) -> String {
        let go_type_name = go_name::escape_keyword(&self.enum_name);

        let mut lines = Vec::new();
        lines.push(format!("func {fn_name}() []{go_type_name} {{"));
        lines.push(format!("return []{go_type_name}{{"));
        for variant in &self.variants {
            lines.push(format!("{{Tag: {}}},", variant.tag_constant));
        }
        lines.push("}".to_string());
        lines.push("}".to_string());

        lines.join("\n")
    }

    pub(crate) fn emit_json_methods(&self, receiver_generics: &str) -> String {
        let receiver = receiver_name(&self.enum_name);
        let go_type_name = go_name::escape_keyword(&self.enum_name);
        let receiver_type = format!("{}{}", go_type_name, receiver_generics);

        let marshal = self.emit_marshal_json(&receiver, &receiver_type);
        let unmarshal = self.emit_unmarshal_json(&receiver, &receiver_type);

        format!("{}\n\n{}", marshal, unmarshal)
    }

    fn emit_marshal_json(&self, receiver: &str, receiver_type: &str) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "func ({receiver} {receiver_type}) MarshalJSON() ([]byte, error) {{"
        ));
        lines.push(format!("switch {receiver}.Tag {{"));

        for variant in &self.variants {
            lines.push(format!("case {}:", variant.tag_constant));

            if variant.fields.is_empty() {
                lines.push(format!("return json.Marshal(\"{}\")", variant.name));
            } else if variant.is_struct_variant {
                let pairs: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|f| format!("\"{}\": {receiver}.{}", f.source_name, f.go_name))
                    .collect();
                lines.push(format!(
                    "return json.Marshal(map[string]any{{\"{}\": map[string]any{{{}}}}})",
                    variant.name,
                    pairs.join(", ")
                ));
            } else if variant.fields.len() == 1 {
                lines.push(format!(
                    "return json.Marshal(map[string]any{{\"{}\": {receiver}.{}}})",
                    variant.name, variant.fields[0].go_name
                ));
            } else {
                let values: Vec<String> = variant
                    .fields
                    .iter()
                    .map(|f| format!("{receiver}.{}", f.go_name))
                    .collect();
                lines.push(format!(
                    "return json.Marshal(map[string]any{{\"{}\": []any{{{}}}}})",
                    variant.name,
                    values.join(", ")
                ));
            }
        }

        lines.push("default:".to_string());
        lines.push(format!(
            "return nil, fmt.Errorf(\"unknown {} tag: %d\", {receiver}.Tag)",
            self.enum_name
        ));
        lines.push("}".to_string());
        lines.push("}".to_string());

        lines.join("\n")
    }

    fn emit_unmarshal_json(&self, receiver: &str, receiver_type: &str) -> String {
        let (no_payload, with_payload): (Vec<&VariantLayout>, Vec<&VariantLayout>) =
            self.variants.iter().partition(|v| v.fields.is_empty());

        let mut lines = Vec::new();
        lines.push(format!(
            "func ({receiver} *{receiver_type}) UnmarshalJSON(data []byte) error {{"
        ));

        if !no_payload.is_empty() {
            self.emit_unmarshal_no_payload_block(
                &mut lines,
                &no_payload,
                !with_payload.is_empty(),
                receiver,
            );
        }

        if !with_payload.is_empty() {
            self.emit_unmarshal_with_payload_block(&mut lines, &with_payload, receiver);
        }

        lines.push("}".to_string());

        lines.join("\n")
    }

    /// String-shape decoder for payload-less variants. Wrapped in
    /// `if err == nil` when with-payload variants also exist (so the object
    /// shape is the fallback).
    fn emit_unmarshal_no_payload_block(
        &self,
        lines: &mut Vec<String>,
        no_payload: &[&VariantLayout],
        has_with_payload: bool,
        receiver: &str,
    ) {
        lines.push("var name string".to_string());
        if has_with_payload {
            lines.push("if err := json.Unmarshal(data, &name); err == nil {".to_string());
        } else {
            lines.push("if err := json.Unmarshal(data, &name); err != nil {".to_string());
            lines.push(format!(
                "return fmt.Errorf(\"invalid {} JSON: expected string\")",
                self.enum_name
            ));
            lines.push("}".to_string());
        }
        lines.push("switch name {".to_string());
        for variant in no_payload {
            lines.push(format!("case \"{}\":", variant.name));
            lines.push(format!("{receiver}.Tag = {}", variant.tag_constant));
            lines.push("return nil".to_string());
        }
        lines.push("default:".to_string());
        lines.push(format!(
            "return fmt.Errorf(\"unknown {} variant: %s\", name)",
            self.enum_name
        ));
        lines.push("}".to_string());
        if has_with_payload {
            lines.push("}".to_string());
        }
    }

    /// Object-shape decoder; per-variant decoding dispatches on shape.
    fn emit_unmarshal_with_payload_block(
        &self,
        lines: &mut Vec<String>,
        with_payload: &[&VariantLayout],
        receiver: &str,
    ) {
        lines.push("var obj map[string]json.RawMessage".to_string());
        lines.push("if err := json.Unmarshal(data, &obj); err != nil {".to_string());
        lines.push(format!(
            "return fmt.Errorf(\"invalid {} JSON\")",
            self.enum_name
        ));
        lines.push("}".to_string());
        lines.push("for key, val := range obj {".to_string());
        lines.push("switch key {".to_string());

        for variant in with_payload {
            lines.push(format!("case \"{}\":", variant.name));
            lines.push(format!("{receiver}.Tag = {}", variant.tag_constant));
            emit_unmarshal_variant_payload(lines, variant, receiver);
        }

        lines.push("default:".to_string());
        lines.push(format!(
            "return fmt.Errorf(\"unknown {} variant: %s\", key)",
            self.enum_name
        ));
        lines.push("}".to_string());
        lines.push("}".to_string());
        lines.push(format!(
            "return fmt.Errorf(\"empty {} JSON object\")",
            self.enum_name
        ));
    }
}

/// Per-variant payload decoding dispatched on shape.
fn emit_unmarshal_variant_payload(
    lines: &mut Vec<String>,
    variant: &VariantLayout,
    receiver: &str,
) {
    if variant.is_struct_variant {
        emit_unmarshal_struct_variant(lines, variant, receiver);
    } else if variant.fields.len() == 1 {
        emit_unmarshal_single_field_variant(lines, variant, receiver);
    } else {
        emit_unmarshal_tuple_variant(lines, variant, receiver);
    }
}

fn emit_unmarshal_struct_variant(lines: &mut Vec<String>, variant: &VariantLayout, receiver: &str) {
    lines.push("var inner map[string]json.RawMessage".to_string());
    lines.push("if err := json.Unmarshal(val, &inner); err != nil {".to_string());
    lines.push("return err".to_string());
    lines.push("}".to_string());
    for field in &variant.fields {
        lines.push(format!(
            "if v, ok := inner[\"{}\"]; ok {{",
            field.source_name
        ));
        lines.push(format!(
            "if err := json.Unmarshal(v, &{receiver}.{}); err != nil {{",
            field.go_name
        ));
        lines.push("return err".to_string());
        lines.push("}".to_string());
        lines.push("}".to_string());
    }
    lines.push("return nil".to_string());
}

fn emit_unmarshal_single_field_variant(
    lines: &mut Vec<String>,
    variant: &VariantLayout,
    receiver: &str,
) {
    lines.push(format!(
        "return json.Unmarshal(val, &{receiver}.{})",
        variant.fields[0].go_name
    ));
}

fn emit_unmarshal_tuple_variant(lines: &mut Vec<String>, variant: &VariantLayout, receiver: &str) {
    let arity = variant.fields.len();
    lines.push("var arr []json.RawMessage".to_string());
    lines.push("if err := json.Unmarshal(val, &arr); err != nil {".to_string());
    lines.push("return err".to_string());
    lines.push("}".to_string());
    lines.push(format!("if len(arr) != {} {{", arity));
    lines.push(format!(
        "return fmt.Errorf(\"{} expects {} fields, got %d\", len(arr))",
        variant.name, arity,
    ));
    lines.push("}".to_string());

    for (i, field) in variant.fields.iter().enumerate() {
        let is_last = i == arity - 1;
        if is_last {
            lines.push(format!(
                "return json.Unmarshal(arr[{}], &{receiver}.{})",
                i, field.go_name
            ));
        } else {
            lines.push(format!(
                "if err := json.Unmarshal(arr[{}], &{receiver}.{}); err != nil {{",
                i, field.go_name
            ));
            lines.push("return err".to_string());
            lines.push("}".to_string());
        }
    }
}
