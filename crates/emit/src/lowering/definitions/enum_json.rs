use crate::Planner;
use crate::lookup::facts::synthesized_methods::StringFormat;
use crate::lowering::abi::go_returns::multi_value_return;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::definitions::structs::format_value;
use crate::target::ir::bodies::{
    Definition as GoDefinition, ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock,
    LoweredStatement, Statement, SwitchCasePlan, SwitchKind, SwitchStatementPlan, assign, declare,
};
use crate::target::ir::declarations::{
    ConstantDeclaration, DeclarationKind, FunctionSignature, GoDeclaration, TypeParameter,
};
use crate::target::ir::go_expression::{BinaryOp, CompositeLayout, GoParameter};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::{self, GeneratedPackage};
use crate::target::names::packages::PackageUse;
use crate::target::names::receiver::ReceiverGenerics;
use crate::target::names::receiver::{synthesized_local_name, synthesized_receiver_name};
use crate::target::types::enum_layout::{EnumLayout, JsonPayload, VariantLayout};
use crate::target::types::go_type::GoType;
use crate::target::types::go_type::receiver_type;

use syntax::go_names::ENUM_TAG_FIELD;

impl EnumLayout {
    pub(in crate::lowering) fn emit_definition(
        &self,
        planner: &mut Planner,
        generics: Vec<TypeParameter>,
        doc: &str,
    ) -> Vec<GoDeclaration> {
        let tag = GoType::new(go_name::enum_tag_type(&self.enum_name));
        let mut declarations = vec![GoDeclaration::new(DeclarationKind::Type {
            name: tag.code.clone(),
            parameters: Vec::new(),
            underlying: GoType::new(self.tag_go_type()),
        })];
        if !self.variants.is_empty() {
            let renumbered = self.default_variant.is_some_and(|i| i != 0);
            let values = self.tag_values();
            declarations.push(GoDeclaration::new(DeclarationKind::Constants(
                self.variants
                    .iter()
                    .enumerate()
                    .map(|(i, variant)| ConstantDeclaration {
                        documentation: variant
                            .doc
                            .as_ref()
                            .map(|doc| doc.lines().map(comment_line).collect::<Vec<_>>().join("\n"))
                            .unwrap_or_default()
                            .into(),
                        name: variant.tag_constant.clone(),
                        ty: (renumbered || i == 0).then(|| tag.clone()),
                        value: if renumbered {
                            Some(GoExpression::literal(values[i].to_string()))
                        } else {
                            (i == 0).then(|| GoExpression::external_name("iota".to_string()))
                        },
                    })
                    .collect(),
            )));
        }
        let mut fields = vec![(ENUM_TAG_FIELD.to_string(), tag)];
        for variant in &self.variants {
            for field in &variant.fields {
                let ty = planner.go_type(&field.ty);
                let ty = if field.is_recursive() {
                    GoType::pointer_to(&ty)
                } else {
                    ty
                };
                if let Some((_, existing)) = fields.iter().find(|(name, _)| name == &field.go_name)
                {
                    assert!(existing == &ty, "shared enum field has incompatible types");
                } else {
                    fields.push((field.go_name.clone(), ty));
                }
            }
        }
        let mut declaration = GoDeclaration::new(DeclarationKind::Type {
            name: go_name::escape_type_name(&self.enum_name).into_owned(),
            parameters: generics,
            underlying: GoType::structure(fields),
        });
        declaration.documentation = doc.to_string().into();
        declarations.push(declaration);
        declarations
    }

    pub(in crate::lowering) fn format_declaration(
        &self,
        planner: &mut Planner,
        generics: &ReceiverGenerics,
        format: StringFormat<'_>,
    ) -> GoDeclaration {
        let receiver = planner
            .scope
            .new_local_identifier(synthesized_receiver_name(&self.enum_name, generics));
        let cases = self
            .variants
            .iter()
            .map(|variant| {
                let prefix = format.prefix(&self.enum_name);
                let (text, arguments) = if variant.fields.is_empty() {
                    (format!("{prefix}{}", variant.name), Vec::new())
                } else {
                    let arguments = variant
                        .fields
                        .iter()
                        .filter_map(|field| {
                            format
                                .format_argument(member(&receiver, &field.go_name), field.format())
                        })
                        .collect();

                    let (open, close, parts) = if variant.is_struct_variant {
                        (
                            " { ",
                            " }",
                            variant
                                .fields
                                .iter()
                                .map(|field| {
                                    format!(
                                        "{}: {}",
                                        field.source_name,
                                        format.verb(field.format())
                                    )
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        (
                            "(",
                            ")",
                            variant
                                .fields
                                .iter()
                                .map(|field| format.verb(field.format()).to_string())
                                .collect(),
                        )
                    };

                    (
                        format!("{prefix}{}{open}{}{close}", variant.name, parts.join(", ")),
                        arguments,
                    )
                };

                let value = format_value(&text, arguments);
                case(
                    GoExpression::external_name(variant.tag_constant.clone()),
                    vec![plain_return(value)],
                )
            })
            .collect();

        let default = vec![plain_return(GoExpression::call(
            GoExpression::generated(GeneratedPackage::Fmt, "Sprintf"),
            vec![
                string(&format!("{}(%d)", self.enum_name)),
                member(&receiver, ENUM_TAG_FIELD),
            ],
        ))];

        let subject = member(&receiver, ENUM_TAG_FIELD);
        GoDeclaration::function(
            Some(GoParameter::new(
                receiver,
                receiver_type(generics, &self.enum_name),
            )),
            format.method(),
            Vec::new(),
            Vec::new(),
            GoType::new("string"),
            LoweredBlock {
                statements: vec![switch(subject, cases, default)],
            },
        )
    }

    pub(in crate::lowering) fn variants_declaration(&self, name: &str) -> GoDeclaration {
        let ty = GoType::new(go_name::escape_type_name(&self.enum_name).into_owned());
        let values = self
            .variants
            .iter()
            .map(|variant| {
                (
                    None,
                    GoExpression::struct_literal(
                        ty.clone(),
                        vec![(
                            ENUM_TAG_FIELD.to_string(),
                            GoExpression::external_name(variant.tag_constant.clone()),
                        )],
                        CompositeLayout::Inline { padded: false },
                    )
                    .elide_composite_type(&ty),
                )
            })
            .collect();

        GoDeclaration::new(DeclarationKind::Function {
            signature: FunctionSignature {
                receiver: None,
                name: name.to_string(),
                generics: Vec::new(),
                parameters: Vec::new(),
                result: GoType::slice_of(&ty),
            },
            body: LoweredBlock {
                statements: vec![plain_return(GoExpression::composite(
                    Some(GoType::slice_of(&ty)),
                    values,
                    CompositeLayout::MultiLine { indented: true },
                ))],
            },
        })
    }

    pub(in crate::lowering) fn json_declarations(
        &self,
        planner: &mut Planner,
        generics: &ReceiverGenerics,
    ) -> Vec<GoDeclaration> {
        let receiver_name = synthesized_receiver_name(&self.enum_name, generics);
        let receiver = planner.scope.new_local_identifier(&receiver_name);
        let receiver_type = receiver_type(generics, &self.enum_name);
        let marshal = self.json_marshal_declaration(&receiver, &receiver_type);
        let mut fresh = |base: &str| {
            planner.scope.new_local_identifier(synthesized_local_name(
                base,
                &receiver_name,
                generics,
            ))
        };

        let names = JsonNames {
            data: fresh("data"),
            name: fresh("name"),
            obj: fresh("obj"),
            key: fresh("key"),
            val: fresh("val"),
            inner: fresh("inner"),
            v: fresh("v"),
            arr: fresh("arr"),
            receiver,
        };

        let unmarshal = self.json_unmarshal_declaration(planner, names, &receiver_type);
        vec![marshal, unmarshal]
    }

    fn json_marshal_declaration(
        &self,
        receiver: &GoIdentifier,
        receiver_type: &GoType,
    ) -> GoDeclaration {
        let cases = self
            .variants
            .iter()
            .map(|variant| {
                let payload = if variant.fields.is_empty() {
                    string(&variant.name)
                } else {
                    let inner = if variant.json_payload() == JsonPayload::Object {
                        map(variant
                            .fields
                            .iter()
                            .map(|field| {
                                (field.source_name.clone(), member(receiver, &field.go_name))
                            })
                            .collect())
                    } else if variant.json_payload() == JsonPayload::Value {
                        member(receiver, &variant.fields[0].go_name)
                    } else {
                        GoExpression::composite(
                            Some(GoType::slice_of(&GoType::new("any"))),
                            variant
                                .fields
                                .iter()
                                .map(|field| (None, member(receiver, &field.go_name)))
                                .collect(),
                            CompositeLayout::Inline { padded: false },
                        )
                    };

                    map(vec![(variant.name.clone(), inner)])
                };

                case(
                    GoExpression::external_name(variant.tag_constant.clone()),
                    vec![plain_return(GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Json, "Marshal"),
                        vec![payload],
                    ))],
                )
            })
            .collect();

        let default = vec![multi_value_return(vec![
            GoExpression::nil(),
            GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                vec![
                    string(&format!("unknown {} tag: %d", self.enum_name)),
                    member(receiver, ENUM_TAG_FIELD),
                ],
            ),
        ])];

        GoDeclaration::function(
            Some(GoParameter::new(receiver.clone(), receiver_type.clone())),
            "MarshalJSON",
            Vec::new(),
            Vec::new(),
            GoType::result_list(&[GoType::slice_of(&GoType::new("byte")), GoType::new("error")]),
            LoweredBlock {
                statements: vec![switch(member(receiver, ENUM_TAG_FIELD), cases, default)],
            },
        )
    }

    fn json_unmarshal_declaration(
        &self,
        planner: &mut Planner,
        names: JsonNames,
        receiver_type: &GoType,
    ) -> GoDeclaration {
        let (empty, payload): (Vec<_>, Vec<_>) = self
            .variants
            .iter()
            .partition(|variant| variant.fields.is_empty());
        let mut body = Vec::new();
        if !empty.is_empty() {
            self.json_unit_variant_statements(
                planner,
                &names,
                &empty,
                payload.is_empty(),
                &mut body,
            );
        }
        if !payload.is_empty() {
            self.json_payload_variant_statements(planner, &names, &payload, &mut body);
        }
        if self.variants.is_empty() {
            body.push(json_error(format!("invalid {} JSON", self.enum_name)));
        }
        GoDeclaration::function(
            Some(GoParameter::new(
                names.receiver,
                GoType::pointer_to(receiver_type),
            )),
            "UnmarshalJSON",
            Vec::new(),
            vec![GoParameter::new(
                names.data,
                GoType::slice_of(&GoType::new("byte")),
            )],
            GoType::new("error"),
            LoweredBlock { statements: body },
        )
    }

    fn json_unit_variant_statements(
        &self,
        planner: &mut Planner,
        names: &JsonNames,
        empty: &[&VariantLayout],
        only_unit_variants: bool,
        body: &mut Vec<Statement>,
    ) {
        body.push(declare(names.name.clone(), GoType::new("string")));
        let cases = empty
            .iter()
            .map(|variant| {
                case(
                    string(&variant.name),
                    vec![
                        assign(
                            member(&names.receiver, ENUM_TAG_FIELD),
                            GoExpression::external_name(variant.tag_constant.clone()),
                        ),
                        plain_return(GoExpression::nil()),
                    ],
                )
            })
            .collect();

        let choice = switch(
            GoExpression::identifier(names.name.clone()),
            cases,
            vec![self.json_unknown_variant(GoExpression::identifier(names.name.clone()))],
        );
        if only_unit_variants {
            body.push(planner.json_guard(
                GoExpression::identifier(names.data.clone()),
                GoExpression::identifier(names.name.clone()),
                Some(vec![json_error(format!(
                    "invalid {} JSON: expected string",
                    self.enum_name
                ))]),
                Polarity::Negated,
            ));
            body.push(choice);
        } else {
            body.push(planner.json_guard(
                GoExpression::identifier(names.data.clone()),
                GoExpression::identifier(names.name.clone()),
                Some(vec![choice]),
                Polarity::Positive,
            ));
        }
    }

    fn json_payload_variant_statements(
        &self,
        planner: &mut Planner,
        names: &JsonNames,
        payload: &[&VariantLayout],
        body: &mut Vec<Statement>,
    ) {
        body.push(declare(
            names.obj.clone(),
            GoType::map_of(&GoType::new("string"), &json_raw_message()),
        ));
        body.push(planner.json_guard(
            GoExpression::identifier(names.data.clone()),
            GoExpression::identifier(names.obj.clone()),
            Some(vec![json_error(format!("invalid {} JSON", self.enum_name))]),
            Polarity::Negated,
        ));
        let cases = payload
            .iter()
            .map(|variant| self.json_payload_variant_case(planner, names, variant))
            .collect();
        let choice = switch(
            GoExpression::identifier(names.key.clone()),
            cases,
            vec![self.json_unknown_variant(GoExpression::identifier(names.key.clone()))],
        );
        body.push(
            LoweredStatement::Loop(LoopPlan {
                kind: LoopKind::Generated { label: None },
                header: LoopHeader::Range {
                    key: Some(names.key.clone()),
                    value: Some(names.val.clone()),
                    iterable: GoExpression::identifier(names.obj.clone()),
                },
                body: LoweredBlock {
                    statements: vec![choice],
                },
            })
            .into(),
        );
        body.push(json_error(format!("empty {} JSON object", self.enum_name)));
    }

    fn json_payload_variant_case(
        &self,
        planner: &mut Planner,
        names: &JsonNames,
        variant: &VariantLayout,
    ) -> SwitchCasePlan {
        let mut body = vec![assign(
            member(&names.receiver, ENUM_TAG_FIELD),
            GoExpression::external_name(variant.tag_constant.clone()),
        )];
        if variant.json_payload() == JsonPayload::Object {
            body.push(declare(
                names.inner.clone(),
                GoType::map_of(&GoType::new("string"), &json_raw_message()),
            ));
            body.push(planner.json_guard(
                GoExpression::identifier(names.val.clone()),
                GoExpression::identifier(names.inner.clone()),
                None,
                Polarity::Negated,
            ));
            for field in &variant.fields {
                let ok = planner.scope.new_local_identifier("ok");
                let mut plan = IfPlan::plain(
                    GoExpression::identifier(ok.clone()),
                    LoweredBlock {
                        statements: vec![planner.json_guard(
                            GoExpression::identifier(names.v.clone()),
                            member(&names.receiver, &field.go_name),
                            None,
                            Polarity::Negated,
                        )],
                    },
                    ElseArm::None,
                );

                plan.initializer = Some(GoDefinition {
                    names: vec![names.v.clone(), ok],
                    value: GoExpression::index(
                        GoExpression::identifier(names.inner.clone()),
                        string(&field.source_name),
                    ),
                });
                body.push(LoweredStatement::If(plan).into());
            }
            body.push(plain_return(GoExpression::nil()));
        } else if variant.json_payload() == JsonPayload::Value {
            body.push(plain_return(unmarshal(
                GoExpression::identifier(names.val.clone()),
                member(&names.receiver, &variant.fields[0].go_name),
            )));
        } else {
            body.push(declare(
                names.arr.clone(),
                GoType::slice_of(&json_raw_message()),
            ));
            body.push(planner.json_guard(
                GoExpression::identifier(names.val.clone()),
                GoExpression::identifier(names.arr.clone()),
                None,
                Polarity::Negated,
            ));
            let len = GoExpression::call(
                GoExpression::external_name("len".to_string()),
                vec![GoExpression::identifier(names.arr.clone())],
            );
            body.push(
                LoweredStatement::If(IfPlan::plain(
                    GoExpression::binary(
                        len.clone(),
                        BinaryOp::Ne,
                        GoExpression::literal(variant.fields.len().to_string()),
                    ),
                    LoweredBlock {
                        statements: vec![plain_return(GoExpression::call(
                            GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                            vec![
                                string(&format!(
                                    "{} expects {} fields, got %d",
                                    variant.name,
                                    variant.fields.len()
                                )),
                                len,
                            ],
                        ))],
                    },
                    ElseArm::None,
                ))
                .into(),
            );
            body.extend(planner.unmarshal_tuple_fields(variant, &names.arr, &names.receiver));
        }
        case(string(&variant.name), body)
    }

    fn json_unknown_variant(&self, value: GoExpression) -> Statement {
        plain_return(GoExpression::call(
            GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
            vec![
                string(&format!("unknown {} variant: %s", self.enum_name)),
                value,
            ],
        ))
    }
}

struct JsonNames {
    receiver: GoIdentifier,
    data: GoIdentifier,
    name: GoIdentifier,
    obj: GoIdentifier,
    key: GoIdentifier,
    val: GoIdentifier,
    inner: GoIdentifier,
    v: GoIdentifier,
    arr: GoIdentifier,
}

fn json_raw_message() -> GoType {
    GoType::named(
        "RawMessage",
        &[],
        Some(PackageUse::generated(GeneratedPackage::Json)),
    )
}

fn json_error(message: String) -> Statement {
    plain_return(GoExpression::call(
        GoExpression::generated(GeneratedPackage::Errors, "New"),
        vec![string(&message)],
    ))
}

fn member(name: &GoIdentifier, field: &str) -> GoExpression {
    GoExpression::selector(GoExpression::identifier(name.clone()), field.to_string())
}

fn string(value: &str) -> GoExpression {
    GoExpression::literal(format!("{value:?}"))
}

fn map(fields: Vec<(String, GoExpression)>) -> GoExpression {
    GoExpression::composite(
        Some(GoType::map_of(&GoType::new("string"), &GoType::new("any"))),
        fields
            .into_iter()
            .map(|(key, value)| (Some(string(&key)), value))
            .collect(),
        CompositeLayout::Inline { padded: false },
    )
}

fn case(label: GoExpression, body: Vec<Statement>) -> SwitchCasePlan {
    SwitchCasePlan {
        labels: vec![label],
        body: LoweredBlock { statements: body },
        line: None,
    }
}

fn switch(subject: GoExpression, cases: Vec<SwitchCasePlan>, default: Vec<Statement>) -> Statement {
    LoweredStatement::Switch(SwitchStatementPlan {
        kind: SwitchKind::Value { subject },
        cases,
        default: Some(LoweredBlock {
            statements: default,
        }),
        default_line: None,
    })
    .into()
}

fn unmarshal(value: GoExpression, target: GoExpression) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Json, "Unmarshal"),
        vec![value, GoExpression::address_of(target)],
    )
}

fn comment_line(line: &str) -> String {
    if line.is_empty() {
        "//".to_string()
    } else {
        format!("// {line}")
    }
}

impl Planner<'_> {
    fn json_guard(
        &mut self,
        value: GoExpression,
        target: GoExpression,
        body: Option<Vec<Statement>>,
        polarity: Polarity,
    ) -> Statement {
        let error = self.scope.new_local_identifier("err");
        let body =
            body.unwrap_or_else(|| vec![plain_return(GoExpression::identifier(error.clone()))]);
        let condition = GoExpression::binary(
            GoExpression::identifier(error.clone()),
            match polarity {
                Polarity::Positive => BinaryOp::Eq,
                Polarity::Negated => BinaryOp::Ne,
            },
            GoExpression::nil(),
        );

        let mut plan = IfPlan::plain(condition, LoweredBlock { statements: body }, ElseArm::None);
        plan.initializer = Some(GoDefinition {
            names: vec![error],
            value: unmarshal(value, target),
        });
        LoweredStatement::If(plan).into()
    }

    fn unmarshal_tuple_fields(
        &mut self,
        variant: &VariantLayout,
        array: &GoIdentifier,
        receiver: &GoIdentifier,
    ) -> Vec<Statement> {
        variant
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let value = GoExpression::index(
                    GoExpression::identifier(array.clone()),
                    GoExpression::literal(index.to_string()),
                );
                let target = member(receiver, &field.go_name);
                if index + 1 == variant.fields.len() {
                    plain_return(unmarshal(value, target))
                } else {
                    self.json_guard(value, target, None, Polarity::Negated)
                }
            })
            .collect()
    }
}
