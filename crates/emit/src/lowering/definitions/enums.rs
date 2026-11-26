use crate::Planner;
use crate::lookup::facts::synthesized_methods::SynthesizedMethod;
use crate::lowering::control_flow::propagation::plain_return;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, SwitchCasePlan, SwitchKind,
    SwitchStatementPlan,
};
use crate::target::ir::declarations::Documentation;
use crate::target::ir::declarations::GoDeclaration;
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::go_expression::{CompositeLayout, GoParameter};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::generics::generic_bounds;
use crate::target::names::go_name::{self, GoExport};
use crate::target::names::receiver::ReceiverGenerics;
use crate::target::names::receiver::{synthesized_local_name, synthesized_receiver_name};
use crate::target::types::enum_layout::{EnumLayout, FieldLayout};
use crate::target::types::go_type::GoType;
use crate::target::types::go_type::receiver_type;
use syntax::ast::{Attribute, Generic};
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::program::{Definition, DefinitionBody, EqualityRecipe};
use syntax::types::{Symbol, Type};

impl Planner<'_> {
    pub(in crate::lowering) fn emit_enum(
        &mut self,
        name: &str,
        generics: &[Generic],
        attributes: &[Attribute],
        documentation: Documentation,
    ) -> Vec<GoDeclaration> {
        let enum_id = self.facts.qualified_current(name);

        let layout = self.facts.enum_layout(&enum_id);

        let receiver_generics = self.names.receiver_generics(generics);
        let generics = self
            .queries()
            .plan_generics(&generics.iter().map(generic_bounds).collect::<Vec<_>>());
        let has_iterate = attributes.iter().any(|a| a.name == "iterate");

        let mut result = layout.emit_definition(self, generics, documentation);
        for method in self.facts.enum_synthesized_methods(name, attributes) {
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
                    &self.facts.to_string_method_go_name(),
                )),
                SynthesizedMethod::Equals => result.extend(self.enum_equals_method(
                    name,
                    &enum_id,
                    layout,
                    &receiver_generics,
                )),
                SynthesizedMethod::StringerShadow => {
                    unreachable!("only structs embed a stringer to shadow")
                }
            }
        }
        if has_iterate {
            let definition = self.facts.definition(enum_id.as_str());
            let export = GoExport::from_visibility(definition.visibility);

            let fn_name = go_name::associated_function_go_name(name, "variants", export);
            result.push(layout.variants_declaration(&fn_name));
        }

        result
    }

    fn enum_equals_method(
        &mut self,
        name: &str,
        enum_id: &str,
        layout: &EnumLayout,
        receiver_generics: &ReceiverGenerics,
    ) -> Option<GoDeclaration> {
        let Definition {
            body:
                DefinitionBody::Enum {
                    variants: source_variants,
                    ..
                },
            ..
        } = self.facts.definition(enum_id)
        else {
            panic!("internal error: `{enum_id}` is not an enum");
        };

        let source_variants = source_variants.clone();
        let receiver = synthesized_receiver_name(name, receiver_generics);
        let other = synthesized_local_name("other", &receiver, receiver_generics);
        let receiver = self.scope.new_local_identifier(receiver);
        let other = self.scope.new_local_identifier(other);
        let receiver_type = receiver_type(receiver_generics, name);
        let go_method = self.facts.equals_method_go_name();

        let mut recipes = self.facts.synthesized_field_equalities(enum_id).iter();
        let mut cases: Vec<SwitchCasePlan> = Vec::new();
        for (source_variant, layout_variant) in source_variants.iter().zip(layout.variants.iter()) {
            if source_variant.fields.is_empty() {
                continue;
            }
            let fields = layout_variant
                .fields
                .iter()
                .take(source_variant.fields.len());
            let value = self.fields_equality(fields, [&receiver, &other], &mut recipes);
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

        let statements = lower_enum_equals_body(&receiver, &other, cases);
        let declaration = GoDeclaration::function(
            Some(GoParameter::new(receiver, receiver_type.clone())),
            go_method,
            Vec::new(),
            vec![GoParameter::new(other, receiver_type)],
            Some(GoType::new("bool")),
            LoweredBlock { statements },
        );

        Some(declaration)
    }

    fn fields_equality<'r>(
        &mut self,
        fields: impl Iterator<Item = &'r FieldLayout>,
        [receiver, other]: [&GoIdentifier; 2],
        recipes: &mut impl Iterator<Item = &'r EqualityRecipe>,
    ) -> GoExpression {
        let comparisons: Vec<GoExpression> = fields
            .map(|layout_field| {
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

                let recipe = recipes
                    .next()
                    .expect("internal error: the checker records the equality of every field");
                self.equality_test(
                    field(receiver),
                    field(other),
                    recipe,
                    GoBinaryOperator::Equal,
                )
            })
            .collect();

        comparisons
            .into_iter()
            .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::And, right))
            .unwrap_or_else(|| GoExpression::boolean(true))
    }

    pub(in crate::lowering) fn create_make_function_code(
        &mut self,
        enum_id: &str,
        variant_name: &str,
    ) -> GoDeclaration {
        let layout = self.facts.enum_layout(enum_id);
        let variant = layout.variant(variant_name);
        let mut fields = vec![(
            ENUM_TAG_FIELD.to_string(),
            GoExpression::external_name(variant.tag_constant.clone()),
        )];
        let mut parameters = Vec::new();
        for (index, field) in variant.fields.iter().enumerate() {
            let name = self.scope.new_local_identifier(format!("arg{index}"));
            parameters.push(GoParameter::new(
                name.clone(),
                self.queries().go_type(&field.ty),
            ));
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
        let result = self.queries().go_type(&Type::Nominal {
            id: Symbol::from_raw(enum_id),
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
            self.queries().plan_generics(
                &layout
                    .generics
                    .iter()
                    .map(generic_bounds)
                    .collect::<Vec<_>>(),
            ),
            parameters,
            Some(result.clone()),
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

fn lower_enum_equals_body(
    receiver: &GoIdentifier,
    other: &GoIdentifier,
    cases: Vec<SwitchCasePlan>,
) -> Vec<Statement> {
    let left = GoExpression::selector(
        GoExpression::identifier(receiver.clone()),
        ENUM_TAG_FIELD.to_string(),
    );
    let right = GoExpression::selector(
        GoExpression::identifier(other.clone()),
        ENUM_TAG_FIELD.to_string(),
    );
    if cases.is_empty() {
        vec![plain_return(GoExpression::binary(
            left,
            GoBinaryOperator::Equal,
            right,
        ))]
    } else {
        vec![
            LoweredStatement::If(IfPlan::plain(
                GoExpression::binary(left.clone(), GoBinaryOperator::NotEqual, right),
                LoweredBlock {
                    statements: vec![plain_return(GoExpression::boolean(false))],
                },
                ElseArm::None,
            ))
            .into(),
            LoweredStatement::Switch(SwitchStatementPlan {
                kind: SwitchKind::Value { subject: left },
                cases,
                default: Some(LoweredBlock {
                    statements: vec![plain_return(GoExpression::boolean(true))],
                }),
                default_line: None,
            })
            .into(),
        ]
    }
}
