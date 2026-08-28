use crate::Planner;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::expressions::top_items::emit_doc;
use crate::names::go_name::{self, prelude_qualifier};
use crate::types::go_type::render_conversion;
use crate::utils::{synthesized_local_name, synthesized_receiver_name};
use rustc_hash::FxHashSet;
use syntax::ast::{Attribute, Generic, StructFieldDefinition, StructFields};
use syntax::attributes::struct_attribute_forces_field_export;
use syntax::go_names;
use syntax::program::MethodOrigin;
use syntax::program::{Definition, DefinitionBody, Methods, interface_requirements};
use syntax::types::Type;

pub(crate) const DEBUG_STRING_METHOD: &str = "DebugString";

impl Planner<'_> {
    pub(crate) fn emit_struct_definition(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &StructFields,
        struct_attrs: &[Attribute],
    ) -> String {
        let generics_string = self.generics_to_string(generics);

        let StructFields::Record(fields) = fields else {
            let StructFields::Tuple(fields) = fields else {
                unreachable!();
            };
            return self.emit_tuple_struct(name, &generics_string, fields, generics, struct_attrs);
        };

        let mut field_strings: Vec<String> = Vec::with_capacity(fields.len());
        let mut stringer_fields: Vec<StringerField> = Vec::with_capacity(fields.len());
        for f in fields {
            let (field_string, stringer_field) = self.emit_struct_field(f, struct_attrs);
            field_strings.push(field_string);
            stringer_fields.push(stringer_field);
        }

        let receiver_generics = self.receiver_generics_string(generics);
        let go_type_name = go_name::escape_type_name(name);

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

        let mut result = if let Some(stringer_name) = self.stringer_method_name(name, struct_attrs)
        {
            let string_method = emit_struct_format_method(
                name,
                &receiver_generics,
                &stringer_fields,
                StringFormat::Display {
                    method: stringer_name,
                    qualified: false,
                },
            );
            if !stringer_fields.is_empty() {
                self.require_fmt();
            }
            format!("{definition}\n\n{string_method}")
        } else {
            definition
        };
        self.append_struct_debug_method(&mut result, name, &receiver_generics, &stringer_fields);
        self.append_to_string_method(&mut result, name, &receiver_generics, struct_attrs);
        self.append_equals_method(&mut result, name, generics, fields, struct_attrs);
        self.append_embedded_stringer_shadow(
            &mut result,
            name,
            &receiver_generics,
            &stringer_fields,
        );
        result
    }

    fn append_embedded_stringer_shadow(
        &mut self,
        out: &mut String,
        name: &str,
        receiver_generics: &str,
        stringer_fields: &[StringerField],
    ) {
        if !self.synthesizes_embedded_stringer_shadow(name) {
            return;
        }
        self.require_fmt();
        out.push_str("\n\n");
        out.push_str(&emit_struct_shadow_stringer_method(
            name,
            receiver_generics,
            stringer_fields,
        ));
    }

    pub(crate) fn synthesizes_embedded_stringer_shadow(&self, name: &str) -> bool {
        let id = self.facts.qualified_current(name);
        let Some(definition) = self.facts.definition(&id) else {
            return false;
        };
        !definition.is_display()
            && self.stringer_kind_of(&id, &mut FxHashSet::default())
                == Some(StringerKind::Synthesized)
    }

    fn stringer_kind(&self, ty: &Type, visited: &mut FxHashSet<String>) -> Option<StringerKind> {
        let Type::Nominal { id, .. } = &self.facts.resolve_embed_target(ty) else {
            return None;
        };
        if !visited.insert(id.to_string()) {
            return None;
        }
        let kind = self.stringer_kind_of(id.as_str(), visited);
        visited.remove(id.as_str());
        kind
    }

    fn stringer_kind_of(&self, id: &str, visited: &mut FxHashSet<String>) -> Option<StringerKind> {
        let definition = self.facts.definition(id)?;
        if definition_declares_string(definition, |m| self.facts.is_ufcs_method(id, m)) {
            return Some(StringerKind::Foreign);
        }
        if matches!(definition.body, DefinitionBody::Interface { .. }) {
            return self
                .interface_has_string_selector(id)
                .then_some(StringerKind::Foreign);
        }
        if definition_emits_go_string_field(definition) {
            return Some(StringerKind::Foreign);
        }
        if definition.is_display() {
            return Some(StringerKind::Synthesized);
        }
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return None;
        };
        self.promoted_stringer_kind(fields, visited)
    }

    fn promoted_stringer_kind(
        &self,
        fields: &[StructFieldDefinition],
        visited: &mut FxHashSet<String>,
    ) -> Option<StringerKind> {
        let mut kinds = fields
            .iter()
            .filter(|f| f.is_embedded())
            .filter_map(|f| self.stringer_kind(&f.ty, visited));
        let first = kinds.next()?;
        if kinds.next().is_some() {
            return None;
        }
        matches!(first, StringerKind::Synthesized).then_some(StringerKind::Synthesized)
    }

    fn interface_has_string_selector(&self, id: &str) -> bool {
        let interface_ty = Type::Nominal {
            id: id.into(),
            params: vec![],
            writable: false,
        };
        interface_requirements(&interface_ty, |id| self.facts.definition(id))
            .iter()
            .any(|requirement| {
                requirement.name == "string" || requirement.name == ENUM_STRINGER_METHOD
            })
    }

    /// Emit a tuple struct and its optional Stringer.
    fn emit_tuple_struct(
        &mut self,
        name: &str,
        generics_string: &str,
        fields: &[StructFieldDefinition],
        generics: &[Generic],
        struct_attrs: &[Attribute],
    ) -> String {
        let definition = self.emit_tuple_struct_definition(name, generics_string, fields);
        let receiver_generics = self.receiver_generics_string(generics);
        let mut result = definition;
        if self.is_pointer_backed_newtype(name) {
            return result;
        }
        let is_type_alias = fields.len() == 1 && generics_string.is_empty();
        let underlying_go_type = is_type_alias.then(|| self.use_go_type(&fields[0].ty));
        let field_is_function: Vec<bool> =
            fields.iter().map(|f| is_raw_function_type(&f.ty)).collect();
        if let Some(stringer_name) = self.stringer_method_name(name, struct_attrs) {
            let string_method = emit_tuple_struct_format_method(
                name,
                &receiver_generics,
                &field_is_function,
                underlying_go_type.as_deref(),
                StringFormat::Display {
                    method: stringer_name,
                    qualified: false,
                },
            );
            if !string_method.is_empty() {
                if string_method.contains("fmt.") {
                    self.require_fmt();
                }
                result.push_str("\n\n");
                result.push_str(&string_method);
            }
        }
        self.append_tuple_struct_debug_method(
            &mut result,
            name,
            &receiver_generics,
            &field_is_function,
            underlying_go_type.as_deref(),
        );
        self.append_to_string_method(&mut result, name, &receiver_generics, struct_attrs);
        result
    }

    /// Emit one Go struct field with its stringer metadata.
    fn emit_struct_field(
        &mut self,
        f: &StructFieldDefinition,
        struct_attrs: &[Attribute],
    ) -> (String, StringerField) {
        if f.is_embedded() {
            let field_with_doc = format!("{}{}", emit_doc(&f.doc), self.use_go_type(&f.ty));
            let stringer_field = StringerField {
                source_name: f.name.to_string(),
                go_name: struct_field_go_name(f, struct_attrs),
                is_function: is_raw_function_type(&f.ty),
            };
            return (field_with_doc, stringer_field);
        }

        let tag_configs = interpret_field_attributes(f, struct_attrs);
        let is_option = self.facts.peel_alias(&f.ty).is_option();
        let tag_string = format_tag_string(&f.name, &tag_configs, is_option);

        let field_name = struct_field_go_name(f, struct_attrs);

        let field_definition = if let Some(tags) = tag_string {
            format!("{} {} {}", field_name, self.use_go_type(&f.ty), tags)
        } else {
            format!("{} {}", field_name, self.use_go_type(&f.ty))
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
    ) -> String {
        let go_type_name = go_name::escape_type_name(name);

        if fields.is_empty() {
            return format!("type {}{} struct{{}}", go_type_name, generics_string);
        }

        if fields.len() == 1 && generics_string.is_empty() {
            let underlying = self.use_go_type(&fields[0].ty);
            return format!("type {} {}", go_type_name, underlying);
        }

        let field_strings: Vec<String> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| format!("F{} {}", i, self.use_go_type(&f.ty)))
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
            .and_then(type_methods);

        let is_user_stringer = |method_name: &str| {
            methods.is_some_and(|methods| {
                methods
                    .get(method_name)
                    .is_some_and(|method| method.ty.is_stringer_signature())
            }) && !self.facts.is_ufcs_method(&qualified, method_name)
        };

        let has_stringer = is_user_stringer("string") || is_user_stringer(ENUM_STRINGER_METHOD);
        let has_go_stringer =
            is_user_stringer("goString") || is_user_stringer(ENUM_GO_STRINGER_METHOD);
        (has_stringer, has_go_stringer)
    }

    pub(crate) fn debug_string_override(&self, name: &str) -> bool {
        let qualified = self.facts.qualified_current(name);
        let methods = self
            .facts
            .definition(qualified.as_str())
            .and_then(type_methods);
        let has_signature = |method_name: &str| {
            methods.is_some_and(|methods| {
                methods
                    .get(method_name)
                    .is_some_and(|method| method.ty.is_stringer_signature())
            }) && !self.facts.is_ufcs_method(&qualified, method_name)
        };
        (self.method_needs_export("debug_string") && has_signature("debug_string"))
            || has_signature(DEBUG_STRING_METHOD)
    }

    pub(crate) fn synthesizes_debug_string(&self, name: &str) -> bool {
        self.facts.emit_tests_enabled() && !self.debug_string_override(name)
    }

    fn append_struct_debug_method(
        &mut self,
        out: &mut String,
        name: &str,
        receiver_generics: &str,
        stringer_fields: &[StringerField],
    ) {
        if !self.synthesizes_debug_string(name) {
            return;
        }
        if !stringer_fields.is_empty() {
            self.require_fmt();
            if stringer_fields.iter().any(|f| !f.is_function) {
                self.require_stdlib();
            }
        }
        out.push_str("\n\n");
        out.push_str(&emit_struct_format_method(
            name,
            receiver_generics,
            stringer_fields,
            StringFormat::Debug {
                prelude: prelude_qualifier(),
            },
        ));
    }

    fn append_tuple_struct_debug_method(
        &mut self,
        out: &mut String,
        name: &str,
        receiver_generics: &str,
        field_is_function: &[bool],
        underlying: Option<&str>,
    ) {
        if !self.synthesizes_debug_string(name) {
            return;
        }
        let uses_prelude = field_is_function.iter().any(|is_function| !is_function);
        if !field_is_function.is_empty() {
            self.require_fmt();
        }
        if uses_prelude {
            self.require_stdlib();
        }
        out.push_str("\n\n");
        out.push_str(&emit_tuple_struct_format_method(
            name,
            receiver_generics,
            field_is_function,
            underlying,
            StringFormat::Debug {
                prelude: prelude_qualifier(),
            },
        ));
    }

    pub(crate) fn should_synthesize_to_string(&self, name: &str, attributes: &[Attribute]) -> bool {
        if !attributes.iter().any(|a| a.name == "display") {
            return false;
        }
        let qualified = self.facts.qualified_current(name);
        let has_user_method = self
            .facts
            .method(&qualified, "to_string")
            .is_some_and(|method| {
                matches!(method.origin, MethodOrigin::Declared) && method.ty.is_stringer_signature()
            })
            && !self.facts.is_ufcs_method(&qualified, "to_string");
        !has_user_method
    }

    pub(crate) fn should_synthesize_equals(&self, name: &str) -> bool {
        let qualified = self.facts.qualified_current(name);
        self.facts.synthesizes_equals(qualified.as_str())
    }

    fn is_pointer_backed_newtype(&self, name: &str) -> bool {
        let qualified = self.facts.qualified_current(name);
        self.facts
            .definition(qualified.as_str())
            .is_some_and(|definition| {
                definition.is_pointer_backed_newtype(|id| self.facts.definition(id))
            })
    }

    pub(crate) fn to_string_method_go_name(&self) -> String {
        self.method_go_name("to_string")
    }

    pub(crate) fn equals_method_go_name(&self) -> String {
        self.method_go_name("equals")
    }

    fn method_go_name(&self, method: &str) -> String {
        if self.method_needs_export(method) {
            go_name::snake_to_camel(method)
        } else {
            go_name::unexported_method_go_name(method)
        }
    }

    pub(crate) fn append_to_string_method(
        &self,
        out: &mut String,
        name: &str,
        receiver_generics: &str,
        attributes: &[Attribute],
    ) {
        if self.should_synthesize_to_string(name, attributes) {
            let go_method = self.to_string_method_go_name();
            out.push_str("\n\n");
            out.push_str(&emit_to_string_method(name, receiver_generics, &go_method));
        }
    }

    fn append_equals_method(
        &mut self,
        out: &mut String,
        name: &str,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
        attributes: &[Attribute],
    ) {
        if !self.should_synthesize_equals(name) {
            return;
        }
        let receiver_generics = self.receiver_generics_string(generics);
        let receiver = synthesized_receiver_name(name, &receiver_generics);
        let other = synthesized_local_name("other", &receiver, &receiver_generics);
        let comparisons: Vec<String> = fields
            .iter()
            .map(|f| {
                let go_field = struct_field_go_name(f, attributes);
                let lhs = format!("{receiver}.{go_field}");
                let rhs = format!("{other}.{go_field}");
                self.render_equality(&lhs, &rhs, &f.ty, generics)
            })
            .collect();
        let body = if comparisons.is_empty() {
            "true".to_string()
        } else {
            comparisons.join(" && ")
        };
        let go_method = self.equals_method_go_name();
        let go_type_name = go_name::escape_type_name(name);
        let receiver_type = format!("{go_type_name}{receiver_generics}");
        out.push_str("\n\n");
        out.push_str(&format!(
            "func ({receiver} {receiver_type}) {go_method}({other} {receiver_type}) bool {{\nreturn {body}\n}}"
        ));
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

pub(crate) fn should_synthesize_stringer(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "display")
}

pub(crate) fn struct_field_go_name(
    field: &StructFieldDefinition,
    struct_attrs: &[Attribute],
) -> String {
    let struct_forces_export = struct_attrs
        .iter()
        .any(struct_attribute_forces_field_export);
    go_names::struct_field_go_name(field, struct_forces_export).into_owned()
}

struct StringerField {
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

#[derive(Clone, Copy)]
pub(crate) enum StringFormat<'a> {
    Display { method: &'a str, qualified: bool },
    Debug { prelude: &'a str },
}

impl<'a> StringFormat<'a> {
    pub(crate) fn method(self) -> &'a str {
        match self {
            StringFormat::Display { method, .. } => method,
            StringFormat::Debug { .. } => DEBUG_STRING_METHOD,
        }
    }

    pub(crate) fn prefix(self, type_name: &str) -> String {
        match self {
            StringFormat::Display {
                qualified: true, ..
            } => format!("{type_name}."),
            _ => String::new(),
        }
    }

    pub(crate) fn verb(self, is_function: bool) -> &'static str {
        match (self, is_function) {
            (_, true) => "%p",
            (StringFormat::Display { .. }, false) => "%v",
            (StringFormat::Debug { .. }, false) => "%s",
        }
    }

    pub(crate) fn argument(self, value: String, is_function: bool) -> String {
        match (self, is_function) {
            (StringFormat::Display { .. }, _) | (StringFormat::Debug { .. }, true) => value,
            (StringFormat::Debug { prelude }, false) => format!("{prelude}.Debug({value})"),
        }
    }
}

fn emit_to_string_method(name: &str, receiver_generics: &str, method_name: &str) -> String {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_type_name(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    format!(
        "func ({receiver} {receiver_type}) {method_name}() string {{\nreturn {receiver}.String()\n}}"
    )
}

fn emit_struct_format_method(
    name: &str,
    receiver_generics: &str,
    fields: &[StringerField],
    format: StringFormat<'_>,
) -> String {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_type_name(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    let method = format.method();
    if fields.is_empty() {
        return format!(
            "func ({receiver} {receiver_type}) {method}() string {{\nreturn \"{name}\"\n}}"
        );
    }
    let format_parts: Vec<String> = fields
        .iter()
        .map(|f| format!("{}: {}", f.source_name, format.verb(f.is_function)))
        .collect();
    let args: Vec<String> = fields
        .iter()
        .map(|f| format.argument(format!("{receiver}.{}", f.go_name), f.is_function))
        .collect();
    format!(
        "func ({receiver} {receiver_type}) {method}() string {{\nreturn fmt.Sprintf(\"{name} {{ {} }}\", {})\n}}",
        format_parts.join(", "),
        args.join(", ")
    )
}

#[derive(Clone, Copy, PartialEq)]
enum StringerKind {
    Synthesized,
    Foreign,
}

fn definition_emits_go_string_field(definition: &Definition) -> bool {
    let DefinitionBody::Struct { fields, .. } = &definition.body else {
        return false;
    };
    let forces_export = definition.is_serialized();
    fields
        .iter()
        .any(|field| go_names::struct_field_go_name(field, forces_export) == ENUM_STRINGER_METHOD)
}

fn type_methods(definition: &Definition) -> Option<&Methods> {
    match &definition.body {
        DefinitionBody::Struct { methods, .. }
        | DefinitionBody::Enum { methods, .. }
        | DefinitionBody::TypeAlias { methods, .. } => Some(methods),
        _ => None,
    }
}

fn definition_declares_string(definition: &Definition, is_ufcs: impl Fn(&str) -> bool) -> bool {
    let Some(methods) = type_methods(definition) else {
        return false;
    };
    ["string", ENUM_STRINGER_METHOD]
        .iter()
        .any(|method| methods.contains_key(*method) && !is_ufcs(method))
}

fn emit_struct_shadow_stringer_method(
    name: &str,
    receiver_generics: &str,
    fields: &[StringerField],
) -> String {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_keyword(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    if fields.is_empty() {
        return format!(
            "func ({receiver} {receiver_type}) String() string {{\nreturn \"{{}}\"\n}}"
        );
    }
    let placeholders: Vec<&str> = fields.iter().map(|_| "%v").collect();
    let args: Vec<String> = fields
        .iter()
        .map(|f| format!("{receiver}.{}", f.go_name))
        .collect();
    format!(
        "func ({receiver} {receiver_type}) String() string {{\nreturn fmt.Sprintf(\"{{{}}}\", {})\n}}",
        placeholders.join(" "),
        args.join(", ")
    )
}

fn emit_tuple_struct_format_method(
    name: &str,
    receiver_generics: &str,
    field_is_function: &[bool],
    underlying_go_type: Option<&str>,
    format: StringFormat<'_>,
) -> String {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_type_name(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    let method = format.method();
    if field_is_function.is_empty() {
        return format!(
            "func ({receiver} {receiver_type}) {method}() string {{\nreturn \"{name}\"\n}}"
        );
    }
    if let Some(underlying) = underlying_go_type {
        let is_function = field_is_function[0];
        let value = format.argument(render_conversion(underlying, &receiver), is_function);
        return format!(
            "func ({receiver} {receiver_type}) {method}() string {{\nreturn fmt.Sprintf(\"{name}({})\", {value})\n}}",
            format.verb(is_function)
        );
    }
    let placeholders: Vec<&str> = field_is_function
        .iter()
        .map(|is_function| format.verb(*is_function))
        .collect();
    let args: Vec<String> = field_is_function
        .iter()
        .enumerate()
        .map(|(i, is_function)| format.argument(format!("{receiver}.F{i}"), *is_function))
        .collect();
    format!(
        "func ({receiver} {receiver_type}) {method}() string {{\nreturn fmt.Sprintf(\"{name}({})\", {})\n}}",
        placeholders.join(", "),
        args.join(", ")
    )
}
