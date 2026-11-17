use crate::Planner;
use crate::control_flow::propagation::plain_return;
use crate::definitions::enum_layout::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use crate::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::expressions::display_form::{DisplayForm, UNIT_DISPLAY};
use crate::expressions::top_items::emit_item_doc;
use crate::names::go_name::{self, GeneratedPackage};
use crate::plan::bodies::LoweredBlock;
use crate::plan::declarations::GoDeclaration;
use crate::plan::declarations::{DeclarationKind, FieldDeclaration, TypeParameter};
use crate::plan::go_expression::BinaryOp;
use crate::plan::go_expression::GoParameter;
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use crate::utils::ReceiverGenerics;
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
        attributes: &[Attribute],
    ) -> Vec<GoDeclaration> {
        let parameters = self.plan_generics(
            &generics
                .iter()
                .map(|generic| {
                    (
                        generic.name.clone(),
                        generic
                            .resolved_bounds()
                            .expect("generic bounds are resolved before emission")
                            .cloned()
                            .collect(),
                    )
                })
                .collect::<Vec<_>>(),
        );
        let StructFields::Record(fields) = fields else {
            let StructFields::Tuple(fields) = fields else {
                unreachable!();
            };
            return self.emit_tuple_struct(name, parameters, fields, generics, attributes);
        };
        let (fields_plan, stringer_fields): (Vec<_>, Vec<_>) = fields
            .iter()
            .map(|field| self.emit_struct_field(field, attributes))
            .unzip();
        let receiver_generics = self.receiver_generics(generics);
        let mut result = vec![GoDeclaration::new(DeclarationKind::Struct {
            name: go_name::escape_type_name(name).into_owned(),
            parameters,
            fields: fields_plan,
        })];
        for method in self.struct_synthesized_methods(name, attributes, StructShape::Record) {
            result.push(match method {
                SynthesizedMethod::Format(format) => emit_struct_format_method(
                    self,
                    name,
                    &receiver_generics,
                    &stringer_fields,
                    format,
                ),
                SynthesizedMethod::StringerShadow => emit_struct_shadow_stringer_method(
                    self,
                    name,
                    &receiver_generics,
                    &stringer_fields,
                ),
                SynthesizedMethod::ToString => self.emit_to_string_method(
                    name,
                    &receiver_generics,
                    &self.to_string_method_go_name(),
                ),
                SynthesizedMethod::Equals => {
                    self.struct_equals_method(name, generics, fields, attributes)
                }
                SynthesizedMethod::Json => unreachable!("only enums synthesize JSON methods"),
            });
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
        let Type::Nominal { id, .. } = &self.emit_shape_ty(ty) else {
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

    fn emit_tuple_struct(
        &mut self,
        name: &str,
        parameters: Vec<TypeParameter>,
        fields: &[StructFieldDefinition],
        generics: &[Generic],
        attributes: &[Attribute],
    ) -> Vec<GoDeclaration> {
        let underlying =
            (fields.len() == 1 && parameters.is_empty()).then(|| self.go_type(&fields[0].ty));
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
                        name: Some(format!("F{index}")),
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
                SynthesizedMethod::Format(format) => emit_tuple_struct_format_method(
                    self,
                    name,
                    &receiver_generics,
                    &formats,
                    underlying.as_ref(),
                    format,
                ),
                SynthesizedMethod::ToString => self.emit_to_string_method(
                    name,
                    &receiver_generics,
                    &self.to_string_method_go_name(),
                ),
                _ => unreachable!("tuple structs synthesize only formatting methods"),
            });
        }
        result
    }

    fn emit_struct_field(
        &self,
        field: &StructFieldDefinition,
        attributes: &[Attribute],
    ) -> (FieldDeclaration, StringerField) {
        let name = struct_field_go_name(field, attributes);
        let tag = if field.is_embedded() {
            None
        } else {
            let configs = interpret_field_attributes(field, attributes);
            format_tag_string(
                &field.name,
                &configs,
                self.facts.peel_alias(&field.ty).is_option(),
            )
        };
        let declaration = FieldDeclaration {
            documentation: emit_item_doc(&field.doc, &field.name, &name).into(),
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

    /// Whether the user already supplies `(String, GoString)` via real receiver
    /// methods (UFCS-emitted free functions do not satisfy Go interfaces, so
    /// they don't count). Drives which stringers the compiler synthesizes.
    pub(crate) fn stringer_overrides(&self, name: &str) -> (bool, bool) {
        let is_user_stringer = |method_name| self.has_stringer_signature(name, method_name);
        let has_stringer = is_user_stringer("string") || is_user_stringer(ENUM_STRINGER_METHOD);
        let has_go_stringer =
            is_user_stringer("goString") || is_user_stringer(ENUM_GO_STRINGER_METHOD);
        (has_stringer, has_go_stringer)
    }

    pub(crate) fn debug_string_override(&self, name: &str) -> bool {
        (self.method_needs_export("debug_string")
            && self.has_stringer_signature(name, "debug_string"))
            || self.has_stringer_signature(name, DEBUG_STRING_METHOD)
    }

    fn has_stringer_signature(&self, name: &str, method_name: &str) -> bool {
        let qualified = self.facts.qualified_current(name);
        self.facts
            .definition(qualified.as_str())
            .and_then(type_methods)
            .and_then(|methods| methods.get(method_name))
            .is_some_and(|method| {
                method
                    .ty
                    .is_stringer_signature(|ty| self.facts.peel_alias(ty))
            })
            && !self.facts.is_ufcs_method(&qualified, method_name)
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

    fn struct_equals_method(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &[StructFieldDefinition],
        attributes: &[Attribute],
    ) -> GoDeclaration {
        let receiver_generics = self.receiver_generics(generics);
        let receiver = synthesized_receiver_name(name, &receiver_generics);
        let other = synthesized_local_name("other", &receiver, &receiver_generics);
        let receiver = self.scope.new_local_identifier(receiver);
        let other = self.scope.new_local_identifier(other);
        let comparisons: Vec<GoExpression> = fields
            .iter()
            .map(|f| {
                let go_field = struct_field_go_name(f, attributes);
                let field = |base: &GoIdentifier| {
                    GoExpression::selector(GoExpression::identifier(base.clone()), go_field.clone())
                };

                self.equality_expression(field(&receiver), field(&other), &f.ty, generics)
            })
            .collect();
        let body = comparisons
            .into_iter()
            .reduce(|left, right| GoExpression::binary(left, BinaryOp::And, right))
            .unwrap_or_else(|| GoExpression::literal("true".to_string()));
        let receiver_type = receiver_generics.instantiate(&go_name::escape_type_name(name));
        GoDeclaration::function(
            Some(GoParameter::new(receiver, receiver_type.clone())),
            self.equals_method_go_name(),
            Vec::new(),
            vec![GoParameter::new(other, receiver_type)],
            GoType::new("bool"),
            LoweredBlock {
                statements: vec![plain_return(body)],
            },
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
        Some(GoParameter::new(
            receiver,
            generics.instantiate(&go_name::escape_type_name(name)),
        )),
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
impl Planner<'_> {
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
                "String".to_string(),
            ),
            vec![],
        );
        string_method(name, generics, receiver, method, value)
    }
}
fn emit_struct_format_method(
    planner: &mut Planner,
    name: &str,
    generics: &ReceiverGenerics,
    fields: &[StringerField],
    format: StringFormat<'_>,
) -> GoDeclaration {
    let receiver = planner
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
    planner: &mut Planner,
    name: &str,
    generics: &ReceiverGenerics,
    fields: &[StringerField],
) -> GoDeclaration {
    let receiver = planner
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
        "String",
        format_value(&text, arguments),
    )
}
fn emit_tuple_struct_format_method(
    planner: &mut Planner,
    name: &str,
    generics: &ReceiverGenerics,
    fields: &[DisplayForm],
    underlying: Option<&GoType>,
    format: StringFormat<'_>,
) -> GoDeclaration {
    let receiver = planner
        .scope
        .new_local_identifier(synthesized_receiver_name(name, generics));
    let arguments = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let receiver = GoExpression::identifier(receiver.clone());
            let value = match underlying {
                Some(ty) => GoExpression::conversion(ty.clone(), receiver),
                None => GoExpression::selector(receiver, format!("F{index}")),
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
