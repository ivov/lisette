use crate::Planner;
use crate::lookup::facts::synthesized_methods::{StringFormat, StructShape, SynthesizedMethod};
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::definitions::tags::{format_tag_string, interpret_field_attributes};
use crate::lowering::expressions::top_items::item_doc_text;
use crate::target::ir::bodies::LoweredBlock;
use crate::target::ir::declarations::GoDeclaration;
use crate::target::ir::declarations::{DeclarationKind, FieldDeclaration, TypeParameter};
use crate::target::ir::go_expression::BinaryOp;
use crate::target::ir::go_expression::GoParameter;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::generics::generic_bounds;
use crate::target::names::go_name::{self, GeneratedPackage};
use crate::target::names::receiver::ReceiverGenerics;
use crate::target::names::receiver::{synthesized_local_name, synthesized_receiver_name};
use crate::target::types::display_form::DisplayForm;
use crate::target::types::go_type::GoType;
use syntax::ast::{Attribute, Generic, StructFieldDefinition, StructFields};
use syntax::go_names::ENUM_STRINGER_METHOD;

impl Planner<'_> {
    pub(in crate::lowering) fn emit_struct_definition(
        &mut self,
        name: &str,
        generics: &[Generic],
        fields: &StructFields,
        attributes: &[Attribute],
    ) -> Vec<GoDeclaration> {
        let parameters =
            self.plan_generics(&generics.iter().map(generic_bounds).collect::<Vec<_>>());
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
        for method in self
            .facts
            .struct_synthesized_methods(name, attributes, StructShape::Record)
        {
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
        for method in self
            .facts
            .struct_synthesized_methods(name, attributes, StructShape::Tuple)
        {
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

    pub(in crate::lowering) fn emit_to_string_method(
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

struct StringerField {
    source_name: String,
    go_name: String,
    format: DisplayForm,
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
pub(in crate::lowering) fn format_value(
    text: &str,
    mut arguments: Vec<GoExpression>,
) -> GoExpression {
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
