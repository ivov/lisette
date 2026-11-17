use crate::Planner;
use crate::control_flow::propagation::plain_return;
use crate::definitions::structs::SynthesizedMethod;
use crate::names::go_name;
use crate::plan::bodies::{ElseArm, IfPlan, LoweredStatement, SwitchKind, SwitchStatementPlan};
use crate::plan::bodies::{LoweredBlock, SwitchCasePlan};
use crate::plan::declarations::GoDeclaration;
use crate::plan::go_expression::BinaryOp;
use crate::plan::go_expression::{CompositeLayout, GoParameter};
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use crate::utils::ReceiverGenerics;
use crate::utils::{synthesized_local_name, synthesized_receiver_name};
use syntax::ast::{Attribute, Generic};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

impl Planner<'_> {
    pub(crate) fn emit_enum(
        &mut self,
        name: &str,
        generics: &[Generic],
        attributes: &[Attribute],
        doc_comment: &str,
    ) -> Option<Vec<GoDeclaration>> {
        let enum_id = self.facts.qualified_current(name);

        let layout = self.facts.enum_layout(&enum_id)?;

        let receiver_generics = self.receiver_generics(generics);
        let generics = self.plan_generics(
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
        let has_iterate = attributes.iter().any(|a| a.name == "iterate");

        let mut result = layout.emit_definition(self, generics, doc_comment);
        for method in self.enum_synthesized_methods(name, attributes) {
            match method {
                SynthesizedMethod::Format(format) => {
                    result.push(layout.format_declaration(self, &receiver_generics, format))
                }
                SynthesizedMethod::Json => {
                    result.extend(layout.json_declarations(self, &receiver_generics))
                }
                SynthesizedMethod::ToString => result.push(self.emit_to_string_method(
                    name,
                    &receiver_generics,
                    &self.to_string_method_go_name(),
                )),
                SynthesizedMethod::Equals => {
                    result.extend(self.enum_equals_method(name, &enum_id, &receiver_generics))
                }
                SynthesizedMethod::StringerShadow => {
                    unreachable!("only structs embed a stringer to shadow")
                }
            }
        }
        if has_iterate {
            let is_public = self
                .facts
                .definition(enum_id.as_str())
                .is_some_and(|definition| definition.visibility.is_public());
            let fn_name = go_name::associated_function_go_name(name, "variants", is_public);
            result.push(layout.variants_declaration(&fn_name));
        }

        Some(result)
    }

    fn enum_equals_method(
        &mut self,
        name: &str,
        enum_id: &str,
        receiver_generics: &ReceiverGenerics,
    ) -> Option<GoDeclaration> {
        let Some(Definition {
            body:
                DefinitionBody::Enum {
                    generics: sem_generics,
                    variants: sem_variants,
                    ..
                },
            ..
        }) = self.facts.definition(enum_id)
        else {
            return None;
        };
        let sem_generics = sem_generics.clone();
        let sem_variants = sem_variants.clone();
        let layout = self
            .facts
            .enum_layout(enum_id)
            .expect("enum layout should exist");

        let receiver = synthesized_receiver_name(name, receiver_generics);
        let other = synthesized_local_name("other", &receiver, receiver_generics);
        let receiver = self.scope.new_local_identifier(receiver);
        let other = self.scope.new_local_identifier(other);
        let go_type_name = go_name::escape_type_name(name);
        let receiver_type = receiver_generics.instantiate(&go_type_name);
        let go_method = self.equals_method_go_name();

        let mut cases: Vec<SwitchCasePlan> = Vec::new();
        for (sem_variant, layout_variant) in sem_variants.iter().zip(layout.variants.iter()) {
            if sem_variant.fields.is_empty() {
                continue;
            }
            let comparisons: Vec<GoExpression> = sem_variant
                .fields
                .iter()
                .zip(layout_variant.fields.iter())
                .map(|(sem_field, layout_field)| {
                    let field = |base: &GoIdentifier| {
                        let access = GoExpression::selector(
                            GoExpression::identifier(base.clone()),
                            layout_field.go_name.clone(),
                        );
                        if layout_field.is_recursive() {
                            GoExpression::dereference(access)
                        } else {
                            access
                        }
                    };

                    self.equality_expression(
                        field(&receiver),
                        field(&other),
                        &sem_field.ty,
                        &sem_generics,
                    )
                })
                .collect();
            let value = comparisons
                .into_iter()
                .reduce(|left, right| GoExpression::binary(left, BinaryOp::And, right))
                .unwrap_or_else(|| GoExpression::literal("true".to_string()));
            cases.push(SwitchCasePlan {
                labels: vec![GoExpression::external_name(
                    layout_variant.tag_constant.clone(),
                )],
                body: LoweredBlock {
                    statements: vec![plain_return(value)],
                },
                line: None,
            });
        }

        let left = GoExpression::selector(
            GoExpression::identifier(receiver.clone()),
            "Tag".to_string(),
        );
        let right =
            GoExpression::selector(GoExpression::identifier(other.clone()), "Tag".to_string());
        let statements = if cases.is_empty() {
            vec![plain_return(GoExpression::binary(
                left,
                BinaryOp::Eq,
                right,
            ))]
        } else {
            vec![
                LoweredStatement::If(IfPlan::plain(
                    GoExpression::binary(left.clone(), BinaryOp::Ne, right),
                    LoweredBlock {
                        statements: vec![plain_return(GoExpression::literal("false".to_string()))],
                    },
                    ElseArm::None,
                ))
                .into(),
                LoweredStatement::Switch(SwitchStatementPlan {
                    kind: SwitchKind::Value { subject: left },
                    cases,
                    default: Some(LoweredBlock {
                        statements: vec![plain_return(GoExpression::literal("true".to_string()))],
                    }),
                    default_line: None,
                })
                .into(),
            ]
        };
        let declaration = GoDeclaration::function(
            Some(GoParameter::new(receiver, receiver_type.clone())),
            go_method,
            Vec::new(),
            vec![GoParameter::new(other, receiver_type)],
            GoType::new("bool"),
            LoweredBlock { statements },
        );
        Some(declaration)
    }

    pub(crate) fn create_make_function_code(
        &mut self,
        enum_id: &str,
        variant_name: &str,
    ) -> GoDeclaration {
        let layout = self.facts.enum_layout(enum_id).expect("enum layout exists");
        let variant = layout
            .get_variant(variant_name)
            .expect("variant exists in layout");
        let mut fields = vec![(
            "Tag".to_string(),
            GoExpression::external_name(variant.tag_constant.clone()),
        )];
        let mut parameters = Vec::new();
        for (index, field) in variant.fields.iter().enumerate() {
            let name = self.scope.new_local_identifier(format!("arg{index}"));
            parameters.push(GoParameter::new(name.clone(), self.go_type(&field.ty)));
            let value = GoExpression::identifier(name);
            fields.push((
                field.go_name.clone(),
                if field.is_recursive() {
                    GoExpression::address_of(value)
                } else {
                    value
                },
            ));
        }
        let result = self.go_type(&Type::Nominal {
            id: Symbol::from_raw(layout.enum_name.clone()),
            writable: false,
            params: layout
                .generics
                .iter()
                .map(|generic| Type::Parameter(generic.name.clone()))
                .collect(),
        });
        GoDeclaration::function(
            None,
            go_name::enum_make_function(&layout.enum_name, &variant.name),
            self.plan_generics(
                &layout
                    .generics
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
            ),
            parameters,
            result.clone(),
            LoweredBlock {
                statements: vec![plain_return(GoExpression::struct_literal(
                    result,
                    fields,
                    CompositeLayout::Inline { padded: true },
                ))],
            },
        )
    }
}
