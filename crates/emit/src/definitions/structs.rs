use crate::EmitFacts;
use crate::Planner;
use crate::control_flow::propagation::plain_return;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::expressions::display_form::{DisplayForm, UNIT_DISPLAY};
use crate::expressions::top_items::item_doc_text;
use crate::ir::bodies::LoweredBlock;
use crate::ir::declarations::GoDeclaration;
use crate::ir::declarations::{DeclarationKind, FieldDeclaration, TypeParameter};
use crate::ir::go_expression::BinaryOp;
use crate::ir::go_expression::GoParameter;
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::generics::generic_bounds;
use crate::names::go_name::{self, GeneratedPackage};
use crate::names::receiver::ReceiverGenerics;
use crate::names::receiver::{synthesized_local_name, synthesized_receiver_name};
use crate::types::go_type::GoType;
use rustc_hash::FxHashSet;
use syntax::ast::{Attribute, Generic, StructFieldDefinition, StructFields};
use syntax::go_names;
use syntax::program::MethodOrigin;
use syntax::program::{Definition, DefinitionBody, interface_requirements};
use syntax::types::Type;

const DEBUG_STRING_METHOD: &str = "DebugString";
const STRINGER_METHODS: [&str; 2] = ["string", ENUM_STRINGER_METHOD];

impl Planner<'_> {
    pub(crate) fn emit_struct_definition(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &StructFields,
        attributes: &[Attribute],
    ) -> Vec<GoDeclaration> {
        let parameters = self.plan_generics(
            &generics.iter().map(generic_bounds).collect::<Vec<_>>(),
            None,
        );
        let StructFields::Record(fields) = fields else {
            let StructFields::Tuple(fields) = fields else {
                unreachable!();
            };
            return self.emit_tuple_struct(name, parameters, fields, generics, attributes);
        };
        let qualified = self.facts.qualified_current(name);
        let (fields_plan, stringer_fields): (Vec<_>, Vec<_>) = fields
            .iter()
            .map(|field| self.emit_struct_field(&qualified, field, attributes))
            .unzip();
        let receiver_generics = self.receiver_generics(generics);
        let mut result = vec![GoDeclaration::new(DeclarationKind::Struct {
            name: go_name::escape_type_name(name).into_owned(),
            parameters,
            fields: fields_plan,
        })];
        for method in self.struct_synthesized_methods(name, attributes, StructShape::Record) {
            result.push(match method {
                SynthesizedMethod::Format(format) => self.emit_struct_format_method(
                    name,
                    &receiver_generics,
                    &stringer_fields,
                    format,
                ),
                SynthesizedMethod::StringerShadow => self.emit_struct_shadow_stringer_method(
                    name,
                    &receiver_generics,
                    &stringer_fields,
                ),
                SynthesizedMethod::ToString => self.emit_to_string_method(
                    name,
                    &receiver_generics,
                    &self.facts.to_string_method_go_name(),
                ),
                SynthesizedMethod::Equals => self.struct_equals_method(name, generics, fields),
                SynthesizedMethod::Json => unreachable!("only enums synthesize JSON methods"),
            });
        }
        result
    }

    fn synthesizes_embedded_stringer_shadow(&self, name: &str) -> bool {
        let id = self.facts.qualified_current(name);
        let Some(definition) = self.facts.definition(&id) else {
            return false;
        };
        !definition.is_display()
            && self.stringer_kind_of(&id, &mut FxHashSet::default())
                == Some(StringerKind::Synthesized)
    }

    fn stringer_kind(&self, ty: &Type, visited: &mut FxHashSet<String>) -> Option<StringerKind> {
        let id = &self.facts.receiver_nominal_id(ty)?;
        if !visited.insert(id.to_string()) {
            return None;
        }
        let kind = self.stringer_kind_of(id.as_str(), visited);
        visited.remove(id.as_str());
        kind
    }

    fn stringer_kind_of(&self, id: &str, visited: &mut FxHashSet<String>) -> Option<StringerKind> {
        let definition = self.facts.definition(id)?;
        if self.facts.declares_string_selector(id) {
            return Some(StringerKind::Foreign);
        }
        if matches!(definition.body, DefinitionBody::Interface { .. }) {
            return None;
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

    fn emit_tuple_struct(
        &mut self,
        name: &str,
        parameters: Vec<TypeParameter>,
        fields: &[StructFieldDefinition],
        generics: &[Generic],
        attributes: &[Attribute],
    ) -> Vec<GoDeclaration> {
        let underlying = self
            .facts
            .is_newtype_definition(name)
            .then(|| self.go_type(&fields[0].ty));
        let go_name = go_name::escape_type_name(name).into_owned();
        let definition = match &underlying {
            Some(underlying) => DeclarationKind::Type {
                name: go_name,
                parameters,
                underlying: underlying.clone(),
            },
            None => DeclarationKind::Struct {
                name: go_name,
                parameters,
                fields: fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| FieldDeclaration {
                        documentation: Default::default(),
                        name: Some(go_name::tuple_struct_field(index)),
                        ty: self.go_type(&field.ty),
                        tag: None,
                    })
                    .collect(),
            },
        };
        let mut result = vec![GoDeclaration::new(definition)];
        let receiver_generics = self.receiver_generics(generics);
        let formats = fields
            .iter()
            .map(|field| DisplayForm::of(&field.ty, |id| self.facts.definition(id)))
            .collect::<Vec<_>>();
        for method in self.struct_synthesized_methods(name, attributes, StructShape::Tuple) {
            result.push(match method {
                SynthesizedMethod::Format(format) => self.emit_tuple_struct_format_method(
                    name,
                    &receiver_generics,
                    &formats,
                    underlying.as_ref(),
                    format,
                ),
                SynthesizedMethod::ToString => self.emit_to_string_method(
                    name,
                    &receiver_generics,
                    &self.facts.to_string_method_go_name(),
                ),
                _ => unreachable!("tuple structs synthesize only formatting methods"),
            });
        }
        result
    }

    fn emit_struct_field(
        &self,
        struct_id: &str,
        field: &StructFieldDefinition,
        attributes: &[Attribute],
    ) -> (FieldDeclaration, StringerField) {
        let name = self.facts.declared_field_go_name(struct_id, field);
        let tag = if field.is_embedded() {
            None
        } else {
            let configs = interpret_field_attributes(field, attributes);
            format_tag_string(&field.name, &configs, self.facts.is_option(&field.ty))
        };
        let declaration = FieldDeclaration {
            documentation: item_doc_text(&field.doc, &field.name, &name).into(),
            name: (!field.is_embedded()).then(|| name.clone()),
            ty: self.go_type(&field.ty),
            tag,
        };
        (
            declaration,
            StringerField {
                source_name: field.name.to_string(),
                go_name: name,
                format: DisplayForm::of(&field.ty, |id| self.facts.definition(id)),
            },
        )
    }

    fn struct_equals_method(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
    ) -> GoDeclaration {
        let receiver_generics = self.receiver_generics(generics);
        let receiver = synthesized_receiver_name(name, &receiver_generics);
        let other = synthesized_local_name("other", &receiver, &receiver_generics);
        let receiver = self.scope.new_local_identifier(receiver);
        let other = self.scope.new_local_identifier(other);
        let qualified = self.facts.qualified_current(name);
        let mut recipes = self
            .facts
            .synthesized_field_equalities(qualified.as_str())
            .iter();
        let comparisons: Vec<GoExpression> = fields
            .iter()
            .map(|f| {
                let recipe = recipes
                    .next()
                    .expect("internal error: the checker records the equality of every field");
                let go_field = self.facts.declared_field_go_name(&qualified, f);
                let field = |base: &GoIdentifier| {
                    GoExpression::selector(GoExpression::identifier(base.clone()), go_field.clone())
                };
                self.equality_expression(field(&receiver), field(&other), recipe)
            })
            .collect();
        let body = comparisons
            .into_iter()
            .reduce(|left, right| GoExpression::binary(left, BinaryOp::And, right))
            .unwrap_or_else(|| GoExpression::literal("true".to_string()));
        let receiver_type = receiver_generics.receiver_type(name);
        GoDeclaration::function(
            Some(GoParameter::new(receiver, receiver_type.clone())),
            self.facts.equals_method_go_name(),
            Vec::new(),
            vec![GoParameter::new(other, receiver_type)],
            GoType::new("bool"),
            LoweredBlock {
                statements: vec![plain_return(body)],
            },
        )
    }

    pub(crate) fn struct_synthesized_methods(
        &self,
        name: &str,
        attributes: &[Attribute],
        shape: StructShape,
    ) -> Vec<SynthesizedMethod> {
        if shape == StructShape::Tuple && self.facts.is_pointer_backed_newtype(name) {
            return Vec::new();
        }
        let mut methods = Vec::new();
        if let Some(method) = self.facts.stringer_method_name(name, attributes) {
            methods.push(SynthesizedMethod::Format(StringFormat::Display {
                method,
                qualified: false,
            }));
        }
        if self.facts.synthesizes_debug_string(name) {
            methods.push(SynthesizedMethod::Format(StringFormat::Debug));
        }
        if self.facts.should_synthesize_to_string(name) {
            methods.push(SynthesizedMethod::ToString);
        }
        if shape == StructShape::Record {
            if self.facts.should_synthesize_equals(name) {
                methods.push(SynthesizedMethod::Equals);
            }
            if self.synthesizes_embedded_stringer_shadow(name) {
                methods.push(SynthesizedMethod::StringerShadow);
            }
        }
        methods
    }

    pub(crate) fn emit_to_string_method(
        &mut self,
        name: &str,
        generics: &ReceiverGenerics,
        method: &str,
    ) -> GoDeclaration {
        let receiver = self
            .scope
            .new_local_identifier(synthesized_receiver_name(name, generics));
        let value = GoExpression::call(
            GoExpression::selector(
                GoExpression::identifier(receiver.clone()),
                ENUM_STRINGER_METHOD.to_string(),
            ),
            vec![],
        );
        string_method(name, generics, receiver, method, value)
    }

    fn emit_struct_format_method(
        &mut self,
        name: &str,
        generics: &ReceiverGenerics,
        fields: &[StringerField],
        format: StringFormat<'_>,
    ) -> GoDeclaration {
        let receiver = self
            .scope
            .new_local_identifier(synthesized_receiver_name(name, generics));
        let parts = fields
            .iter()
            .map(|field| format!("{}: {}", field.source_name, format.verb(field.format)))
            .collect::<Vec<_>>();
        let arguments = fields
            .iter()
            .filter_map(|field| {
                format.format_argument(
                    GoExpression::selector(
                        GoExpression::identifier(receiver.clone()),
                        field.go_name.clone(),
                    ),
                    field.format,
                )
            })
            .collect();
        let text = if fields.is_empty() {
            name.to_string()
        } else {
            format!("{name} {{ {} }}", parts.join(", "))
        };
        string_method(
            name,
            generics,
            receiver,
            format.method(),
            format_value(&text, arguments),
        )
    }

    fn emit_struct_shadow_stringer_method(
        &mut self,
        name: &str,
        generics: &ReceiverGenerics,
        fields: &[StringerField],
    ) -> GoDeclaration {
        let receiver = self
            .scope
            .new_local_identifier(synthesized_receiver_name(name, generics));
        let text = format!("{{{}}}", vec!["%v"; fields.len()].join(" "));
        let arguments = fields
            .iter()
            .map(|field| {
                GoExpression::selector(
                    GoExpression::identifier(receiver.clone()),
                    field.go_name.clone(),
                )
            })
            .collect();
        string_method(
            name,
            generics,
            receiver,
            ENUM_STRINGER_METHOD,
            format_value(&text, arguments),
        )
    }

    fn emit_tuple_struct_format_method(
        &mut self,
        name: &str,
        generics: &ReceiverGenerics,
        fields: &[DisplayForm],
        underlying: Option<&GoType>,
        format: StringFormat<'_>,
    ) -> GoDeclaration {
        let receiver = self
            .scope
            .new_local_identifier(synthesized_receiver_name(name, generics));
        let arguments = fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                let receiver = GoExpression::identifier(receiver.clone());
                let value = match underlying {
                    Some(ty) => GoExpression::conversion(ty.clone(), receiver),
                    None => GoExpression::selector(receiver, go_name::tuple_struct_field(index)),
                };
                format.format_argument(value, *field)
            })
            .collect();
        let text = if fields.is_empty() {
            name.to_string()
        } else {
            format!(
                "{name}({})",
                fields
                    .iter()
                    .map(|field| format.verb(*field))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        string_method(
            name,
            generics,
            receiver,
            format.method(),
            format_value(&text, arguments),
        )
    }
}

fn should_synthesize_stringer(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "display")
}

struct StringerField {
    source_name: String,
    go_name: String,
    format: DisplayForm,
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

#[derive(Clone, Copy)]
pub(crate) enum SynthesizedMethod {
    Format(StringFormat<'static>),
    StringerShadow,
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

    pub(crate) fn verb(self, field: DisplayForm) -> &'static str {
        match (self, field) {
            (_, DisplayForm::Unit) => UNIT_DISPLAY,
            (_, DisplayForm::Function) => "%p",
            (StringFormat::Display { .. }, DisplayForm::Char) => "%c",
            (StringFormat::Display { .. }, _) => "%v",
            (StringFormat::Debug { .. }, _) => "%s",
        }
    }

    pub(crate) fn format_argument(
        self,
        value: GoExpression,
        field: DisplayForm,
    ) -> Option<GoExpression> {
        match (self, field) {
            (_, DisplayForm::Unit) => None,
            (StringFormat::Display { .. }, _) | (StringFormat::Debug, DisplayForm::Function) => {
                Some(value)
            }
            (StringFormat::Debug, _) => Some(GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                vec![value],
            )),
        }
    }
}

fn string_method(
    name: &str,
    generics: &ReceiverGenerics,
    receiver: GoIdentifier,
    method: &str,
    value: GoExpression,
) -> GoDeclaration {
    GoDeclaration::function(
        Some(GoParameter::new(receiver, generics.receiver_type(name))),
        method.to_string(),
        Vec::new(),
        Vec::new(),
        GoType::new("string"),
        LoweredBlock {
            statements: vec![plain_return(value)],
        },
    )
}
pub(crate) fn format_value(text: &str, mut arguments: Vec<GoExpression>) -> GoExpression {
    let text = GoExpression::literal(format!("{text:?}"));
    if arguments.is_empty() {
        return text;
    }
    arguments.insert(0, text);
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Fmt, "Sprintf"),
        arguments,
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

impl EmitFacts<'_> {
    pub(crate) fn declared_field_go_name(
        &self,
        struct_id: &str,
        field: &StructFieldDefinition,
    ) -> String {
        let forces_export = self
            .definition(struct_id)
            .is_some_and(|definition| definition.is_serialized());
        go_names::struct_field_go_name(field, forces_export).into_owned()
    }

    fn declares_string_selector(&self, id: &str) -> bool {
        let Some(definition) = self.definition(id) else {
            return false;
        };
        if matches!(definition.body, DefinitionBody::Interface { .. }) {
            return self.interface_has_string_selector(id);
        }
        STRINGER_METHODS
            .iter()
            .any(|method| self.emits_go_method(id, method))
    }

    fn interface_has_string_selector(&self, id: &str) -> bool {
        let interface_ty = Type::Nominal {
            id: id.into(),
            params: vec![],
            writable: false,
        };
        interface_requirements(&interface_ty, |id| self.definition(id))
            .iter()
            .any(|requirement| STRINGER_METHODS.contains(&requirement.name.as_str()))
    }

    fn should_synthesize_to_string(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.method(&qualified, "to_string")
            .is_some_and(|method| method.origin == MethodOrigin::Synthesized)
    }

    fn should_synthesize_equals(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.synthesizes_equals(qualified.as_str())
    }

    pub(crate) fn is_newtype_definition(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.definition(qualified.as_str())
            .is_some_and(Definition::is_newtype)
    }

    fn is_pointer_backed_newtype(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.definition(qualified.as_str())
            .is_some_and(|definition| {
                definition.is_pointer_backed_newtype(|id| self.definition(id))
            })
    }

    fn stringer_overrides(&self, name: &str) -> (bool, bool) {
        let qualified = self.qualified_current(name);
        let declares = |methods: &[&str]| {
            methods
                .iter()
                .any(|method| self.has_stringer_signature(&qualified, method))
        };
        (
            declares(&STRINGER_METHODS),
            declares(&["goString", ENUM_GO_STRINGER_METHOD]),
        )
    }

    fn debug_string_override(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        (self.method_needs_export("debug_string")
            && self.has_stringer_signature(&qualified, "debug_string"))
            || self.has_stringer_signature(&qualified, DEBUG_STRING_METHOD)
    }

    fn synthesizes_debug_string(&self, name: &str) -> bool {
        self.tests_enabled() && !self.debug_string_override(name)
    }

    pub(crate) fn to_string_method_go_name(&self) -> String {
        self.method_go_name("to_string", false)
    }

    pub(crate) fn equals_method_go_name(&self) -> String {
        self.method_go_name("equals", false)
    }

    fn stringer_method_name(&self, name: &str, attributes: &[Attribute]) -> Option<&'static str> {
        if !should_synthesize_stringer(attributes) {
            return None;
        }
        match self.stringer_overrides(name) {
            (true, true) => None,
            (true, false) => Some(ENUM_GO_STRINGER_METHOD),
            _ => Some(ENUM_STRINGER_METHOD),
        }
    }

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
}
