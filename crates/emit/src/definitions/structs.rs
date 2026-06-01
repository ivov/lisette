use crate::EmitEffects;
use crate::Planner;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::expressions::top_items::emit_doc;
use crate::names::generics::receiver_generics_string;
use crate::names::go_name;
use crate::utils::receiver_name;
use syntax::ast::{Attribute, Generic, StructFieldDefinition, StructKind};
use syntax::program::DefinitionBody;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn emit_struct_definition(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
        kind: &StructKind,
        struct_attrs: &[Attribute],
        fx: &mut EmitEffects,
    ) -> String {
        let symbol = self.facts.qualified_current(name);
        let generics_string = self.generics_to_string_for_symbol(&symbol, generics, fx);

        if *kind == StructKind::Tuple {
            return self.emit_tuple_struct(
                name,
                &generics_string,
                fields,
                generics,
                struct_attrs,
                fx,
            );
        }

        let mut field_strings: Vec<String> = Vec::with_capacity(fields.len());
        let mut stringer_fields: Vec<StringerField> = Vec::with_capacity(fields.len());
        for f in fields {
            let (field_string, stringer_field) = self.emit_struct_field(f, name, struct_attrs, fx);
            field_strings.push(field_string);
            stringer_fields.push(stringer_field);
        }

        let receiver_generics = receiver_generics_string(generics);
        let go_type_name = go_name::escape_keyword(name);

        let definition = if field_strings.is_empty() {
            format!("type {}{} struct{{}}", go_type_name, generics_string)
        } else {
            format!(
                "type {}{} struct {{\n{}\n}}",
                go_type_name,
                generics_string,
                field_strings.join("\n")
            )
        };

        if let Some(stringer_name) = self.stringer_method_name(name, struct_attrs) {
            let string_method = emit_struct_stringer_method(
                name,
                &receiver_generics,
                &stringer_fields,
                stringer_name,
            );
            if !stringer_fields.is_empty() {
                fx.require_fmt();
            }
            format!("{definition}\n\n{string_method}")
        } else {
            definition
        }
    }

    /// Emit a tuple struct and its optional Stringer.
    fn emit_tuple_struct(
        &mut self,
        name: &str,
        generics_string: &str,
        fields: &[StructFieldDefinition],
        generics: &[Generic],
        struct_attrs: &[Attribute],
        fx: &mut EmitEffects,
    ) -> String {
        let definition = self.emit_tuple_struct_definition(name, generics_string, fields, fx);
        let Some(stringer_name) = self.stringer_method_name(name, struct_attrs) else {
            return definition;
        };
        let receiver_generics = receiver_generics_string(generics);
        let is_type_alias = fields.len() == 1 && generics_string.is_empty();
        let underlying_go_type = is_type_alias.then(|| self.go_type_string(&fields[0].ty, fx));
        let string_method = emit_tuple_struct_stringer_method(
            name,
            &receiver_generics,
            fields.len(),
            underlying_go_type.as_deref(),
            stringer_name,
        );
        if string_method.is_empty() {
            return definition;
        }
        if string_method.contains("fmt.") {
            fx.require_fmt();
        }
        format!("{definition}\n\n{string_method}")
    }

    /// Emit one Go struct field with its stringer metadata.
    fn emit_struct_field(
        &mut self,
        f: &StructFieldDefinition,
        struct_name: &str,
        struct_attrs: &[Attribute],
        fx: &mut EmitEffects,
    ) -> (String, StringerField) {
        let tag_configs = interpret_field_attributes(f, struct_attrs);
        let needs_omitzero = is_option_type(&f.ty);
        let tag_string = format_tag_string(&f.name, &tag_configs, needs_omitzero);

        let has_tags = !tag_configs.is_empty();
        let field_name = struct_field_go_name(f, struct_attrs);

        if has_tags && !f.visibility.is_public() {
            let key = self.facts.qualified_current_member(struct_name, &f.name);
            self.module.record_tag_exported_field(key);
        }

        let field_definition = if let Some(tags) = tag_string {
            format!("{} {} {}", field_name, self.go_type_string(&f.ty, fx), tags)
        } else {
            format!("{} {}", field_name, self.go_type_string(&f.ty, fx))
        };

        let field_with_doc = format!("{}{}", emit_doc(&f.doc), field_definition);

        let stringer_field = StringerField {
            source_name: f.name.to_string(),
            go_name: field_name,
            is_function: is_raw_function_type(&f.ty),
        };
        (field_with_doc, stringer_field)
    }

    fn emit_tuple_struct_definition(
        &mut self,
        name: &str,
        generics_string: &str,
        fields: &[StructFieldDefinition],
        fx: &mut EmitEffects,
    ) -> String {
        let go_type_name = go_name::escape_keyword(name);

        if fields.is_empty() {
            return format!("type {}{} struct{{}}", go_type_name, generics_string);
        }

        if fields.len() == 1 && generics_string.is_empty() {
            let underlying = self.go_type_string(&fields[0].ty, fx);
            return format!("type {} {}", go_type_name, underlying);
        }

        let field_strings: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| format!("F{} {}", i, self.go_type_string(&f.ty, fx)))
            .collect();

        format!(
            "type {}{} struct {{\n{}\n}}",
            go_type_name,
            generics_string,
            field_strings.join("\n")
        )
    }

    /// Whether the user already supplies `(String, GoString)` via real receiver
    /// methods (UFCS-emitted free functions do not satisfy Go interfaces, so
    /// they don't count). Drives which stringers the compiler synthesizes.
    pub(crate) fn stringer_overrides(&self, name: &str) -> (bool, bool) {
        let qualified = self.facts.qualified_current(name);
        let methods = self
            .facts
            .definition(qualified.as_str())
            .and_then(|definition| match &definition.body {
                DefinitionBody::Struct { methods, .. }
                | DefinitionBody::Enum { methods, .. }
                | DefinitionBody::ValueEnum { methods, .. }
                | DefinitionBody::TypeAlias { methods, .. } => Some(methods),
                _ => None,
            });

        let is_user_stringer = |method_name: &str| {
            methods.is_some_and(|m| m.get(method_name).is_some_and(Type::is_stringer_signature))
                && !self.facts.is_ufcs_method(&qualified, method_name)
        };

        let has_stringer = is_user_stringer("string") || is_user_stringer(ENUM_STRINGER_METHOD);
        let has_go_stringer =
            is_user_stringer("goString") || is_user_stringer(ENUM_GO_STRINGER_METHOD);
        (has_stringer, has_go_stringer)
    }

    /// Single stringer to synthesize for structs: `String` by default,
    /// `GoString` when the user already supplies `String`, none when both
    /// exist. Enums use [`Self::stringer_overrides`] directly, since they
    /// synthesize both a bare `String` and a qualified `GoString`.
    pub(crate) fn stringer_method_name(
        &self,
        name: &str,
        attributes: &[Attribute],
    ) -> Option<&'static str> {
        if !should_synthesize_stringer(attributes) {
            return None;
        }
        match self.stringer_overrides(name) {
            (true, true) => None,
            (true, false) => Some(ENUM_GO_STRINGER_METHOD),
            _ => Some(ENUM_STRINGER_METHOD),
        }
    }
}

pub(crate) fn should_synthesize_stringer(_attributes: &[Attribute]) -> bool {
    true
}

pub(crate) fn struct_field_go_name(
    field: &StructFieldDefinition,
    struct_attrs: &[Attribute],
) -> String {
    let needs_export =
        field.visibility.is_public() || !interpret_field_attributes(field, struct_attrs).is_empty();
    if needs_export {
        go_name::make_exported(&field.name)
    } else {
        go_name::escape_keyword(&field.name).into_owned()
    }
}

pub(crate) struct StringerField {
    source_name: String,
    go_name: String,
    is_function: bool,
}

pub(crate) fn is_raw_function_type(ty: &Type) -> bool {
    match ty {
        Type::Function(_) => true,
        Type::Forall { body, .. } => is_raw_function_type(body),
        _ => false,
    }
}

pub(crate) fn stringer_verb(is_function: bool) -> &'static str {
    if is_function { "%p" } else { "%v" }
}

fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Nominal {
            id, underlying_ty, ..
        } => {
            if id == "Option" || id.ends_with(".Option") {
                return true;
            }
            underlying_ty.as_deref().is_some_and(is_option_type)
        }
        _ => false,
    }
}

fn emit_struct_stringer_method(
    name: &str,
    receiver_generics: &str,
    fields: &[StringerField],
    method_name: &str,
) -> String {
    let receiver = receiver_name(name);
    let go_type_name = go_name::escape_keyword(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    if fields.is_empty() {
        return format!(
            "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn \"{name}\"\n}}"
        );
    }
    let format_parts: Vec<String> = fields
        .iter()
        .map(|f| format!("{}: {}", f.source_name, stringer_verb(f.is_function)))
        .collect();
    let args: Vec<String> = fields
        .iter()
        .map(|f| format!("{receiver}.{}", f.go_name))
        .collect();
    format!(
        "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn fmt.Sprintf(\"{name} {{ {} }}\", {})\n}}",
        format_parts.join(", "),
        args.join(", ")
    )
}

fn emit_tuple_struct_stringer_method(
    name: &str,
    receiver_generics: &str,
    field_count: usize,
    underlying_go_type: Option<&str>,
    method_name: &str,
) -> String {
    let receiver = receiver_name(name);
    let go_type_name = go_name::escape_keyword(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    if field_count == 0 {
        return format!(
            "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn \"{name}\"\n}}"
        );
    }
    if let Some(underlying) = underlying_go_type {
        if underlying.starts_with('*') {
            return String::new();
        }
        return format!(
            "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn fmt.Sprintf(\"{name}(%v)\", {underlying}({receiver}))\n}}"
        );
    }
    let placeholders: Vec<&str> = (0..field_count).map(|_| "%v").collect();
    let args: Vec<String> = (0..field_count)
        .map(|i| format!("{receiver}.F{i}"))
        .collect();
    format!(
        "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn fmt.Sprintf(\"{name}({})\", {})\n}}",
        placeholders.join(", "),
        args.join(", ")
    )
}
