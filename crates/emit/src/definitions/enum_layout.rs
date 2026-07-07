use rustc_hash::FxHashMap as HashMap;

use crate::definitions::structs::{debug_verb, stringer_verb};
use crate::names::go_name;
use crate::utils::{synthesized_local_name, synthesized_receiver_name};
use syntax::ast::{EnumVariant, Generic};

pub(crate) use syntax::go_names::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD, ENUM_TAG_FIELD};

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
    pub(crate) is_recursive: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldTypeInfo {
    pub(crate) go_type: String,
    pub(crate) is_function: bool,
    pub(crate) is_recursive: bool,
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

                let go_name = syntax::go_names::enum_field_go_name(
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
                let is_recursive = info.is_some_and(|i| i.is_recursive);

                FieldLayout {
                    source_name,
                    go_name,
                    go_type,
                    is_function,
                    is_recursive,
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
        let receiver = synthesized_receiver_name(&self.enum_name, receiver_generics);
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

    pub(crate) fn emit_debug_method(&self, receiver_generics: &str, prelude: &str) -> String {
        let receiver = synthesized_receiver_name(&self.enum_name, receiver_generics);
        let go_type_name = go_name::escape_keyword(&self.enum_name);
        let receiver_type = format!("{}{}", go_type_name, receiver_generics);

        let mut lines = Vec::new();
        lines.push(format!(
            "func ({receiver} {receiver_type}) DebugString() string {{"
        ));
        lines.push(format!("switch {receiver}.Tag {{"));

        for variant in &self.variants {
            lines.push(format!("case {}:", variant.tag_constant));
            lines.push(self.build_variant_debug_line(variant, &receiver, prelude));
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

    pub(crate) fn debug_uses_prelude(&self) -> bool {
        self.variants
            .iter()
            .flat_map(|v| v.fields.iter())
            .any(|f| !f.is_function)
    }

    fn build_variant_debug_line(
        &self,
        variant: &VariantLayout,
        receiver: &str,
        prelude: &str,
    ) -> String {
        if variant.fields.is_empty() {
            return format!("return \"{}\"", variant.name);
        }
        let args: Vec<String> = variant
            .fields
            .iter()
            .map(|f| {
                if f.is_function {
                    format!("{receiver}.{}", f.go_name)
                } else {
                    format!("{prelude}.Debug({receiver}.{})", f.go_name)
                }
            })
            .collect();
        let (open, close, placeholders) = if variant.is_struct_variant {
            let parts: Vec<String> = variant
                .fields
                .iter()
                .map(|f| format!("{}: {}", f.source_name, debug_verb(f.is_function)))
                .collect();
            (" { ", " }", parts.join(", "))
        } else {
            let parts: Vec<&str> = variant
                .fields
                .iter()
                .map(|f| debug_verb(f.is_function))
                .collect();
            ("(", ")", parts.join(", "))
        };
        format!(
            "return fmt.Sprintf(\"{}{}{}{}\", {})",
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
        let receiver = synthesized_receiver_name(&self.enum_name, receiver_generics);
        let go_type_name = go_name::escape_keyword(&self.enum_name);
        let receiver_type = format!("{}{}", go_type_name, receiver_generics);
        let names = UnmarshalNames::new(&receiver, receiver_generics);

        let marshal = self.emit_marshal_json(&receiver, &receiver_type);
        let unmarshal = self.emit_unmarshal_json(&receiver, &receiver_type, &names);

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

    fn emit_unmarshal_json(
        &self,
        receiver: &str,
        receiver_type: &str,
        names: &UnmarshalNames,
    ) -> String {
        let (no_payload, with_payload): (Vec<&VariantLayout>, Vec<&VariantLayout>) =
            self.variants.iter().partition(|v| v.fields.is_empty());

        let data = &names.data;
        let mut lines = Vec::new();
        lines.push(format!(
            "func ({receiver} *{receiver_type}) UnmarshalJSON({data} []byte) error {{"
        ));

        if !no_payload.is_empty() {
            self.emit_unmarshal_no_payload_block(
                &mut lines,
                &no_payload,
                !with_payload.is_empty(),
                receiver,
                names,
            );
        }

        if !with_payload.is_empty() {
            self.emit_unmarshal_with_payload_block(&mut lines, &with_payload, receiver, names);
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
        names: &UnmarshalNames,
    ) {
        let (data, name) = (&names.data, &names.name);
        lines.push(format!("var {name} string"));
        if has_with_payload {
            lines.push(format!(
                "if err := json.Unmarshal({data}, &{name}); err == nil {{"
            ));
        } else {
            lines.push(format!(
                "if err := json.Unmarshal({data}, &{name}); err != nil {{"
            ));
            lines.push(format!(
                "return fmt.Errorf(\"invalid {} JSON: expected string\")",
                self.enum_name
            ));
            lines.push("}".to_string());
        }
        lines.push(format!("switch {name} {{"));
        for variant in no_payload {
            lines.push(format!("case \"{}\":", variant.name));
            lines.push(format!("{receiver}.Tag = {}", variant.tag_constant));
            lines.push("return nil".to_string());
        }
        lines.push("default:".to_string());
        lines.push(format!(
            "return fmt.Errorf(\"unknown {} variant: %s\", {name})",
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
        names: &UnmarshalNames,
    ) {
        let (data, obj, key, val) = (&names.data, &names.obj, &names.key, &names.val);
        lines.push(format!("var {obj} map[string]json.RawMessage"));
        lines.push(format!(
            "if err := json.Unmarshal({data}, &{obj}); err != nil {{"
        ));
        lines.push(format!(
            "return fmt.Errorf(\"invalid {} JSON\")",
            self.enum_name
        ));
        lines.push("}".to_string());
        lines.push(format!("for {key}, {val} := range {obj} {{"));
        lines.push(format!("switch {key} {{"));

        for variant in with_payload {
            lines.push(format!("case \"{}\":", variant.name));
            lines.push(format!("{receiver}.Tag = {}", variant.tag_constant));
            emit_unmarshal_variant_payload(lines, variant, receiver, names);
        }

        lines.push("default:".to_string());
        lines.push(format!(
            "return fmt.Errorf(\"unknown {} variant: %s\", {key})",
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

/// The fixed local/param names `UnmarshalJSON` generates, each freshened away
/// from the receiver variable and receiver type-parameter names.
struct UnmarshalNames {
    data: String,
    name: String,
    obj: String,
    key: String,
    val: String,
    inner: String,
    v: String,
    arr: String,
}

impl UnmarshalNames {
    fn new(receiver: &str, receiver_generics: &str) -> Self {
        let fresh = |base| synthesized_local_name(base, receiver, receiver_generics);
        Self {
            data: fresh("data"),
            name: fresh("name"),
            obj: fresh("obj"),
            key: fresh("key"),
            val: fresh("val"),
            inner: fresh("inner"),
            v: fresh("v"),
            arr: fresh("arr"),
        }
    }
}

/// Per-variant payload decoding dispatched on shape.
fn emit_unmarshal_variant_payload(
    lines: &mut Vec<String>,
    variant: &VariantLayout,
    receiver: &str,
    names: &UnmarshalNames,
) {
    if variant.is_struct_variant {
        emit_unmarshal_struct_variant(lines, variant, receiver, names);
    } else if variant.fields.len() == 1 {
        emit_unmarshal_single_field_variant(lines, variant, receiver, names);
    } else {
        emit_unmarshal_tuple_variant(lines, variant, receiver, names);
    }
}

fn emit_unmarshal_struct_variant(
    lines: &mut Vec<String>,
    variant: &VariantLayout,
    receiver: &str,
    names: &UnmarshalNames,
) {
    let (val, inner, v) = (&names.val, &names.inner, &names.v);
    lines.push(format!("var {inner} map[string]json.RawMessage"));
    lines.push(format!(
        "if err := json.Unmarshal({val}, &{inner}); err != nil {{"
    ));
    lines.push("return err".to_string());
    lines.push("}".to_string());
    for field in &variant.fields {
        lines.push(format!(
            "if {v}, ok := {inner}[\"{}\"]; ok {{",
            field.source_name
        ));
        lines.push(format!(
            "if err := json.Unmarshal({v}, &{receiver}.{}); err != nil {{",
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
    names: &UnmarshalNames,
) {
    lines.push(format!(
        "return json.Unmarshal({}, &{receiver}.{})",
        names.val, variant.fields[0].go_name
    ));
}

fn emit_unmarshal_tuple_variant(
    lines: &mut Vec<String>,
    variant: &VariantLayout,
    receiver: &str,
    names: &UnmarshalNames,
) {
    let (val, arr) = (&names.val, &names.arr);
    let arity = variant.fields.len();
    lines.push(format!("var {arr} []json.RawMessage"));
    lines.push(format!(
        "if err := json.Unmarshal({val}, &{arr}); err != nil {{"
    ));
    lines.push("return err".to_string());
    lines.push("}".to_string());
    lines.push(format!("if len({arr}) != {} {{", arity));
    lines.push(format!(
        "return fmt.Errorf(\"{} expects {} fields, got %d\", len({arr}))",
        variant.name, arity,
    ));
    lines.push("}".to_string());

    for (i, field) in variant.fields.iter().enumerate() {
        let is_last = i == arity - 1;
        if is_last {
            lines.push(format!(
                "return json.Unmarshal({arr}[{}], &{receiver}.{})",
                i, field.go_name
            ));
        } else {
            lines.push(format!(
                "if err := json.Unmarshal({arr}[{}], &{receiver}.{}); err != nil {{",
                i, field.go_name
            ));
            lines.push("return err".to_string());
            lines.push("}".to_string());
        }
    }
}
