use crate::Planner;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::expressions::top_items::emit_doc;
use crate::names::go_name::{self, GeneratedPackage, prelude_qualifier};
use crate::names::packages::PackageRequirements;
use crate::plan::values::GoExpression;
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

        let mut result = definition;
        for method in self.struct_synthesized_methods(name, struct_attrs, StructShape::Record) {
            let (code, requirements) = match method {
                SynthesizedMethod::Format(format) => {
                    emit_struct_format_method(name, &receiver_generics, &stringer_fields, format)
                }
                SynthesizedMethod::StringerShadow => {
                    emit_struct_shadow_stringer_method(name, &receiver_generics, &stringer_fields)
                }
                SynthesizedMethod::ToString => (
                    self.to_string_method(name, &receiver_generics),
                    PackageRequirements::default(),
                ),
                SynthesizedMethod::Equals => (
                    self.struct_equals_method(name, generics, fields, struct_attrs),
                    PackageRequirements::default(),
                ),
                SynthesizedMethod::Json => unreachable!("only enums synthesize JSON methods"),
            };
            self.require_packages(&requirements);
            result.push_str("\n\n");
            result.push_str(&code);
        }
        result
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
        let mut result = self.emit_tuple_struct_definition(name, generics_string, fields);
        let methods = self.struct_synthesized_methods(name, struct_attrs, StructShape::Tuple);
        if methods.is_empty() {
            return result;
        }
        let receiver_generics = self.receiver_generics_string(generics);
        let is_type_alias = fields.len() == 1 && generics_string.is_empty();
        let underlying_go_type = is_type_alias.then(|| self.use_go_type(&fields[0].ty));
        let field_is_function: Vec<bool> =
            fields.iter().map(|f| is_raw_function_type(&f.ty)).collect();
        for method in methods {
            let code = match method {
                SynthesizedMethod::Format(format) => {
                    let (code, requirements) = emit_tuple_struct_format_method(
                        name,
                        &receiver_generics,
                        &field_is_function,
                        underlying_go_type.as_deref(),
                        format,
                    );
                    self.require_packages(&requirements);
                    code
                }
                SynthesizedMethod::ToString => self.to_string_method(name, &receiver_generics),
                SynthesizedMethod::StringerShadow
                | SynthesizedMethod::Equals
                | SynthesizedMethod::Json => {
                    unreachable!("a tuple struct synthesizes only formatting methods")
                }
            };
            result.push_str("\n\n");
            result.push_str(&code);
        }
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

    pub(crate) fn should_synthesize_to_string(&self, name: &str) -> bool {
        let qualified = self.facts.qualified_current(name);
        self.facts
            .method(&qualified, "to_string")
            .is_some_and(|method| method.origin == MethodOrigin::Synthesized)
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
        self.method_go_name("to_string", false)
    }

    pub(crate) fn equals_method_go_name(&self) -> String {
        self.method_go_name("equals", false)
    }

    pub(crate) fn to_string_method(&self, name: &str, receiver_generics: &str) -> String {
        emit_to_string_method(name, receiver_generics, &self.to_string_method_go_name())
    }

    fn struct_equals_method(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
        attributes: &[Attribute],
    ) -> String {
        let receiver_generics = self.receiver_generics_string(generics);
        let receiver = synthesized_receiver_name(name, &receiver_generics);
        let other = synthesized_local_name("other", &receiver, &receiver_generics);
        let comparisons: Vec<String> = fields
            .iter()
            .map(|f| {
                let go_field = struct_field_go_name(f, attributes);
                let field = |base: &str| {
                    GoExpression::selector(GoExpression::name(base.to_string()), go_field.clone())
                };
                let comparison =
                    self.equality_expression(field(&receiver), field(&other), &f.ty, generics);
                self.render_expression(&comparison)
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
        format!(
            "func ({receiver} {receiver_type}) {go_method}({other} {receiver_type}) bool {{\nreturn {body}\n}}"
        )
    }

    /// The Go methods emit adds to the struct `name`, in emission order.
    pub(crate) fn struct_synthesized_methods(
        &self,
        name: &str,
        attributes: &[Attribute],
        shape: StructShape,
    ) -> Vec<SynthesizedMethod> {
        if shape == StructShape::Tuple && self.is_pointer_backed_newtype(name) {
            return Vec::new();
        }
        let mut methods = Vec::new();
        if let Some(method) = self.stringer_method_name(name, attributes) {
            methods.push(SynthesizedMethod::Format(StringFormat::Display {
                method,
                qualified: false,
            }));
        }
        if self.synthesizes_debug_string(name) {
            methods.push(SynthesizedMethod::Format(StringFormat::Debug));
        }
        if self.should_synthesize_to_string(name) {
            methods.push(SynthesizedMethod::ToString);
        }
        if shape == StructShape::Record {
            if self.should_synthesize_equals(name) {
                methods.push(SynthesizedMethod::Equals);
            }
            if self.synthesizes_embedded_stringer_shadow(name) {
                methods.push(SynthesizedMethod::StringerShadow);
            }
        }
        methods
    }

    /// The Go methods emit adds to the enum `name`, in emission order.
    pub(crate) fn enum_synthesized_methods(
        &self,
        name: &str,
        attributes: &[Attribute],
    ) -> Vec<SynthesizedMethod> {
        let mut methods = Vec::new();
        if should_synthesize_stringer(attributes) {
            let (has_user_string, has_user_go_string) = self.stringer_overrides(name);
            if !has_user_string {
                methods.push(SynthesizedMethod::Format(StringFormat::Display {
                    method: ENUM_STRINGER_METHOD,
                    qualified: false,
                }));
            }
            if !has_user_go_string {
                methods.push(SynthesizedMethod::Format(StringFormat::Display {
                    method: ENUM_GO_STRINGER_METHOD,
                    qualified: true,
                }));
            }
        }
        if attributes.iter().any(|attribute| attribute.name == "json") {
            methods.push(SynthesizedMethod::Json);
        }
        if self.synthesizes_debug_string(name) {
            methods.push(SynthesizedMethod::Format(StringFormat::Debug));
        }
        if self.should_synthesize_to_string(name) {
            methods.push(SynthesizedMethod::ToString);
        }
        if self.should_synthesize_equals(name) {
            methods.push(SynthesizedMethod::Equals);
        }
        methods
    }

    pub(crate) fn synthesized_method_go_names(&self, method: SynthesizedMethod) -> Vec<String> {
        match method {
            SynthesizedMethod::Format(format) => vec![format.method().to_string()],
            SynthesizedMethod::StringerShadow => vec![ENUM_STRINGER_METHOD.to_string()],
            SynthesizedMethod::Json => vec!["MarshalJSON".to_string(), "UnmarshalJSON".to_string()],
            SynthesizedMethod::ToString => vec![self.to_string_method_go_name()],
            SynthesizedMethod::Equals => vec![self.equals_method_go_name()],
        }
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructShape {
    Record,
    Tuple,
}

/// A Go method that emit adds to a Lisette struct or enum.
#[derive(Clone, Copy)]
pub(crate) enum SynthesizedMethod {
    Format(StringFormat<'static>),
    /// A struct `String` that hides the one an embedded field synthesizes.
    StringerShadow,
    /// An enum's `MarshalJSON` and `UnmarshalJSON`.
    Json,
    ToString,
    Equals,
}

#[derive(Clone, Copy)]
pub(crate) enum StringFormat<'a> {
    Display { method: &'a str, qualified: bool },
    Debug,
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

    pub(crate) fn argument(
        self,
        value: String,
        is_function: bool,
        requirements: &mut PackageRequirements,
    ) -> String {
        match (self, is_function) {
            (StringFormat::Display { .. }, _) | (StringFormat::Debug, true) => value,
            (StringFormat::Debug, false) => {
                requirements.require_generated(GeneratedPackage::Prelude);
                format!("{}.Debug({value})", prelude_qualifier())
            }
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
) -> (String, PackageRequirements) {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_type_name(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    let method = format.method();
    let mut requirements = PackageRequirements::default();
    if fields.is_empty() {
        let code = format!(
            "func ({receiver} {receiver_type}) {method}() string {{\nreturn \"{name}\"\n}}"
        );
        return (code, requirements);
    }
    requirements.require_generated(GeneratedPackage::Fmt);
    let format_parts: Vec<String> = fields
        .iter()
        .map(|f| format!("{}: {}", f.source_name, format.verb(f.is_function)))
        .collect();
    let args: Vec<String> = fields
        .iter()
        .map(|f| {
            format.argument(
                format!("{receiver}.{}", f.go_name),
                f.is_function,
                &mut requirements,
            )
        })
        .collect();
    let code = format!(
        "func ({receiver} {receiver_type}) {method}() string {{\nreturn fmt.Sprintf(\"{name} {{ {} }}\", {})\n}}",
        format_parts.join(", "),
        args.join(", ")
    );
    (code, requirements)
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
) -> (String, PackageRequirements) {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_type_name(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    let mut requirements = PackageRequirements::default();
    if fields.is_empty() {
        let code =
            format!("func ({receiver} {receiver_type}) String() string {{\nreturn \"{{}}\"\n}}");
        return (code, requirements);
    }
    requirements.require_generated(GeneratedPackage::Fmt);
    let placeholders: Vec<&str> = fields.iter().map(|_| "%v").collect();
    let args: Vec<String> = fields
        .iter()
        .map(|f| format!("{receiver}.{}", f.go_name))
        .collect();
    let code = format!(
        "func ({receiver} {receiver_type}) String() string {{\nreturn fmt.Sprintf(\"{{{}}}\", {})\n}}",
        placeholders.join(" "),
        args.join(", ")
    );
    (code, requirements)
}

fn emit_tuple_struct_format_method(
    name: &str,
    receiver_generics: &str,
    field_is_function: &[bool],
    underlying_go_type: Option<&str>,
    format: StringFormat<'_>,
) -> (String, PackageRequirements) {
    let receiver = synthesized_receiver_name(name, receiver_generics);
    let go_type_name = go_name::escape_type_name(name);
    let receiver_type = format!("{go_type_name}{receiver_generics}");
    let method = format.method();
    let mut requirements = PackageRequirements::default();
    if field_is_function.is_empty() {
        let code = format!(
            "func ({receiver} {receiver_type}) {method}() string {{\nreturn \"{name}\"\n}}"
        );
        return (code, requirements);
    }
    requirements.require_generated(GeneratedPackage::Fmt);
    if let Some(underlying) = underlying_go_type {
        let is_function = field_is_function[0];
        let value = format.argument(
            render_conversion(underlying, &receiver),
            is_function,
            &mut requirements,
        );
        let code = format!(
            "func ({receiver} {receiver_type}) {method}() string {{\nreturn fmt.Sprintf(\"{name}({})\", {value})\n}}",
            format.verb(is_function)
        );
        return (code, requirements);
    }
    let placeholders: Vec<&str> = field_is_function
        .iter()
        .map(|is_function| format.verb(*is_function))
        .collect();
    let args: Vec<String> = field_is_function
        .iter()
        .enumerate()
        .map(|(i, is_function)| {
            format.argument(format!("{receiver}.F{i}"), *is_function, &mut requirements)
        })
        .collect();
    let code = format!(
        "func ({receiver} {receiver_type}) {method}() string {{\nreturn fmt.Sprintf(\"{name}({})\", {})\n}}",
        placeholders.join(", "),
        args.join(", ")
    );
    (code, requirements)
}
