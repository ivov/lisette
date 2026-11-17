use crate::Planner;
use crate::abi::transition::multi_value_return;
use crate::control_flow::propagation::plain_return;
use crate::definitions::structs::{StringFormat, format_value};
use crate::expressions::display_form::DisplayForm;
use crate::names::go_name::{self, GeneratedPackage};
use crate::names::packages::PackageUse;
use crate::plan::bodies::{
    Definition as GoDefinition, ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock,
    LoweredStatement, Statement, SwitchCasePlan, SwitchKind, SwitchStatementPlan, assign,
};
use crate::plan::declarations::{
    ConstantDeclaration, DeclarationKind, FunctionSignature, GoDeclaration, TypeParameter,
};
use crate::plan::go_expression::{BinaryOp, CompositeLayout, GoParameter};
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use crate::utils::ReceiverGenerics;
use crate::utils::{synthesized_local_name, synthesized_receiver_name};
use syntax::ast::{EnumVariant, Generic};
use syntax::containment::enum_payload_pointer_wrapped;
use syntax::program::Definition;
use syntax::types::Type;

use syntax::go_names;
use syntax::go_names::EnumFieldShape;
pub(crate) use syntax::go_names::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD, ENUM_TAG_FIELD};

#[derive(Debug, Clone)]
pub(crate) struct EnumLayout {
    pub(crate) enum_name: String,
    pub(crate) variants: Vec<VariantLayout>,
    pub(crate) generics: Vec<Generic>,
    /// Index of the `#[default]` variant, which takes tag `0`.
    default_variant: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct VariantLayout {
    pub(crate) name: String,
    pub(crate) tag_constant: String,
    is_struct_variant: bool,
    pub(crate) fields: Vec<FieldLayout>,
    doc: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldLayout {
    pub(crate) source_name: String,
    pub(crate) go_name: String,
    /// Declared payload type, rendered by each declaring file.
    pub(crate) ty: Type,
    kind: FieldKind,
}

#[derive(Debug, Clone, Copy)]
enum FieldKind {
    Plain(DisplayForm),
    Recursive,
}

impl FieldLayout {
    fn format(&self) -> DisplayForm {
        match self.kind {
            FieldKind::Plain(format) => format,
            FieldKind::Recursive => DisplayForm::Value,
        }
    }

    pub(crate) fn is_recursive(&self) -> bool {
        matches!(self.kind, FieldKind::Recursive)
    }
}

impl EnumLayout {
    pub(crate) fn new<'d>(
        enum_id: &str,
        generics: &[Generic],
        variants: &[EnumVariant],
        default_variant: Option<usize>,
        definition: impl Fn(&str) -> Option<&'d Definition> + Copy,
    ) -> Self {
        let enum_name = go_name::unqualified_name(enum_id).to_string();

        let slots = go_names::enum_field_slots(&enum_name, variants);
        let variants: Vec<_> = variants
            .iter()
            .enumerate()
            .map(|(vi, v)| Self::compute_variant_layout(vi, v, enum_id, &slots[vi], definition))
            .collect();
        Self {
            enum_name,
            variants,
            generics: generics.to_vec(),
            default_variant,
        }
    }

    fn tag_go_type(&self) -> &'static str {
        if self.variants.len() <= 256 {
            "uint8"
        } else {
            "uint16"
        }
    }

    /// Declaration order, except that the marked variant takes `0`.
    fn tag_values(&self) -> Vec<usize> {
        let Some(default_index) = self.default_variant.filter(|index| *index != 0) else {
            return (0..self.variants.len()).collect();
        };
        let mut next = 1;
        (0..self.variants.len())
            .map(|index| {
                if index == default_index {
                    return 0;
                }
                let value = next;
                next += 1;
                value
            })
            .collect()
    }

    fn compute_variant_layout<'d>(
        variant_index: usize,
        variant: &EnumVariant,
        enum_id: &str,
        slots: &[String],
        definition: impl Fn(&str) -> Option<&'d Definition>,
    ) -> VariantLayout {
        let enum_name = go_name::unqualified_name(enum_id);
        let tag_constant = go_name::enum_tag_constant(enum_name, &variant.name);

        let field_shape =
            go_names::enum_field_shape(&variant.fields).unwrap_or(EnumFieldShape::TupleMultiple);

        let fields: Vec<_> = variant
            .fields
            .iter()
            .enumerate()
            .map(|(fi, field)| {
                let source_name = if field_shape == EnumFieldShape::Struct {
                    field.name.to_string()
                } else {
                    fi.to_string()
                };

                let go_name = slots[fi].clone();

                let kind = if enum_payload_pointer_wrapped(
                    enum_id,
                    variant_index,
                    fi,
                    &field.ty,
                    &definition,
                ) {
                    FieldKind::Recursive
                } else {
                    FieldKind::Plain(DisplayForm::of(&field.ty, &definition))
                };

                FieldLayout {
                    source_name,
                    go_name,
                    ty: field.ty.clone(),
                    kind,
                }
            })
            .collect();
        VariantLayout {
            name: variant.name.to_string(),
            tag_constant,
            is_struct_variant: field_shape == EnumFieldShape::Struct,
            fields,
            doc: variant.doc.clone(),
        }
    }

    pub(crate) fn get_variant(&self, name: &str) -> Option<&VariantLayout> {
        self.variants.iter().find(|variant| variant.name == name)
    }

    pub(crate) fn struct_field_name(&self, variant_name: &str, field_name: &str) -> Option<String> {
        let variant = self.get_variant(variant_name)?;
        variant
            .fields
            .iter()
            .find(|field| field.source_name == field_name)
            .map(|field| field.go_name.clone())
    }

    pub(crate) fn tuple_field_name(&self, variant_name: &str, index: usize) -> Option<String> {
        let variant = self.get_variant(variant_name)?;
        variant.fields.get(index).map(|f| f.go_name.clone())
    }

    pub(crate) fn emit_definition(
        &self,
        planner: &mut Planner,
        generics: Vec<TypeParameter>,
        doc: &str,
    ) -> Vec<GoDeclaration> {
        let tag = GoType::new(format!("{}Tag", self.enum_name));
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
        let mut fields = vec![("Tag".to_string(), tag)];
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

    pub(crate) fn format_declaration(
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
                member(&receiver, "Tag"),
            ],
        ))];
        let subject = member(&receiver, "Tag");
        GoDeclaration::function(
            Some(GoParameter::new(
                receiver,
                generics.instantiate(&go_name::escape_type_name(&self.enum_name)),
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

    pub(crate) fn variants_declaration(&self, name: &str) -> GoDeclaration {
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
                            "Tag".to_string(),
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

    pub(crate) fn json_declarations(
        &self,
        planner: &mut Planner,
        generics: &ReceiverGenerics,
    ) -> Vec<GoDeclaration> {
        let receiver_name = synthesized_receiver_name(&self.enum_name, generics);
        let receiver = planner.scope.new_local_identifier(&receiver_name);
        let receiver_type = generics.instantiate(&go_name::escape_type_name(&self.enum_name));
        let cases = self
            .variants
            .iter()
            .map(|variant| {
                let payload = if variant.fields.is_empty() {
                    string(&variant.name)
                } else {
                    let inner = if variant.is_struct_variant {
                        map(variant
                            .fields
                            .iter()
                            .map(|field| {
                                (
                                    field.source_name.to_string(),
                                    member(&receiver, &field.go_name),
                                )
                            })
                            .collect())
                    } else if variant.fields.len() == 1 {
                        member(&receiver, &variant.fields[0].go_name)
                    } else {
                        GoExpression::composite(
                            Some(GoType::slice_of(&GoType::new("any"))),
                            variant
                                .fields
                                .iter()
                                .map(|field| (None, member(&receiver, &field.go_name)))
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
                    member(&receiver, "Tag"),
                ],
            ),
        ])];
        let marshal = GoDeclaration::function(
            Some(GoParameter::new(receiver.clone(), receiver_type.clone())),
            "MarshalJSON",
            Vec::new(),
            Vec::new(),
            GoType::result_list(&[GoType::slice_of(&GoType::new("byte")), GoType::new("error")]),
            LoweredBlock {
                statements: vec![switch(member(&receiver, "Tag"), cases, default)],
            },
        );
        let fresh = |planner: &mut Planner, base: &str| {
            planner.scope.new_local_identifier(synthesized_local_name(
                base,
                &receiver_name,
                generics,
            ))
        };
        let data = fresh(planner, "data");
        let name = fresh(planner, "name");
        let obj = fresh(planner, "obj");
        let key = fresh(planner, "key");
        let val = fresh(planner, "val");
        let inner = fresh(planner, "inner");
        let v = fresh(planner, "v");
        let arr = fresh(planner, "arr");
        let raw = GoType::named(
            "RawMessage",
            &[],
            Some(PackageUse::generated(GeneratedPackage::Json)),
        );
        let (empty, payload): (Vec<_>, Vec<_>) = self
            .variants
            .iter()
            .partition(|variant| variant.fields.is_empty());
        let unknown = |value| {
            plain_return(GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                vec![
                    string(&format!("unknown {} variant: %s", self.enum_name)),
                    value,
                ],
            ))
        };
        let invalid = |message: String| {
            plain_return(GoExpression::call(
                GoExpression::generated(GeneratedPackage::Errors, "New"),
                vec![string(&message)],
            ))
        };
        let mut body = Vec::new();
        if !empty.is_empty() {
            body.push(variable(&name, GoType::new("string")));
            let cases = empty
                .iter()
                .map(|variant| {
                    case(
                        string(&variant.name),
                        vec![
                            assign(
                                member(&receiver, "Tag"),
                                GoExpression::external_name(variant.tag_constant.clone()),
                            ),
                            plain_return(GoExpression::nil()),
                        ],
                    )
                })
                .collect();
            let choice = switch(
                GoExpression::identifier(name.clone()),
                cases,
                vec![unknown(GoExpression::identifier(name.clone()))],
            );
            if payload.is_empty() {
                body.push(json_guard(
                    planner,
                    GoExpression::identifier(data.clone()),
                    GoExpression::identifier(name.clone()),
                    Some(vec![invalid(format!(
                        "invalid {} JSON: expected string",
                        self.enum_name
                    ))]),
                    false,
                ));
                body.push(choice);
            } else {
                body.push(json_guard(
                    planner,
                    GoExpression::identifier(data.clone()),
                    GoExpression::identifier(name.clone()),
                    Some(vec![choice]),
                    true,
                ));
            }
        }
        if !payload.is_empty() {
            body.push(variable(&obj, GoType::map_of(&GoType::new("string"), &raw)));
            body.push(json_guard(
                planner,
                GoExpression::identifier(data.clone()),
                GoExpression::identifier(obj.clone()),
                Some(vec![invalid(format!("invalid {} JSON", self.enum_name))]),
                false,
            ));
            let cases = payload
                .iter()
                .map(|variant| {
                    let mut body = vec![assign(
                        member(&receiver, "Tag"),
                        GoExpression::external_name(variant.tag_constant.clone()),
                    )];
                    if variant.is_struct_variant {
                        body.push(variable(
                            &inner,
                            GoType::map_of(&GoType::new("string"), &raw),
                        ));
                        body.push(json_guard(
                            planner,
                            GoExpression::identifier(val.clone()),
                            GoExpression::identifier(inner.clone()),
                            None,
                            false,
                        ));
                        for field in &variant.fields {
                            let ok = planner.scope.new_local_identifier("ok");
                            let mut plan = IfPlan::plain(
                                GoExpression::identifier(ok.clone()),
                                LoweredBlock {
                                    statements: vec![json_guard(
                                        planner,
                                        GoExpression::identifier(v.clone()),
                                        member(&receiver, &field.go_name),
                                        None,
                                        false,
                                    )],
                                },
                                ElseArm::None,
                            );
                            plan.initializer = Some(GoDefinition {
                                names: vec![v.clone(), ok],
                                value: GoExpression::index(
                                    GoExpression::identifier(inner.clone()),
                                    string(&field.source_name),
                                ),
                            });
                            body.push(LoweredStatement::If(plan).into());
                        }
                        body.push(plain_return(GoExpression::nil()));
                    } else if variant.fields.len() == 1 {
                        body.push(plain_return(unmarshal(
                            GoExpression::identifier(val.clone()),
                            member(&receiver, &variant.fields[0].go_name),
                        )));
                    } else {
                        body.push(variable(&arr, GoType::slice_of(&raw)));
                        body.push(json_guard(
                            planner,
                            GoExpression::identifier(val.clone()),
                            GoExpression::identifier(arr.clone()),
                            None,
                            false,
                        ));
                        let len = GoExpression::call(
                            GoExpression::external_name("len".to_string()),
                            vec![GoExpression::identifier(arr.clone())],
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
                        body.extend(unmarshal_tuple_fields(planner, variant, &arr, &receiver));
                    }
                    case(string(&variant.name), body)
                })
                .collect();
            let choice = switch(
                GoExpression::identifier(key.clone()),
                cases,
                vec![unknown(GoExpression::identifier(key.clone()))],
            );
            body.push(
                LoweredStatement::Loop(LoopPlan {
                    kind: LoopKind::Generated { label: None },
                    header: LoopHeader::Range {
                        key: Some(key),
                        value: Some(val),
                        iterable: GoExpression::identifier(obj.clone()),
                    },
                    body: LoweredBlock {
                        statements: vec![choice],
                    },
                })
                .into(),
            );
            body.push(invalid(format!("empty {} JSON object", self.enum_name)));
        }
        if self.variants.is_empty() {
            body.push(invalid(format!("invalid {} JSON", self.enum_name)));
        }
        let unmarshal = GoDeclaration::function(
            Some(GoParameter::new(
                receiver,
                GoType::pointer_to(&receiver_type),
            )),
            "UnmarshalJSON",
            Vec::new(),
            vec![GoParameter::new(
                data,
                GoType::slice_of(&GoType::new("byte")),
            )],
            GoType::new("error"),
            LoweredBlock { statements: body },
        );
        vec![marshal, unmarshal]
    }
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
fn variable(name: &GoIdentifier, go_type: GoType) -> Statement {
    LoweredStatement::VarDecl {
        name: name.clone(),
        go_type,
        value: None,
    }
    .into()
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
fn json_guard(
    planner: &mut Planner,
    value: GoExpression,
    target: GoExpression,
    body: Option<Vec<Statement>>,
    success: bool,
) -> Statement {
    let error = planner.scope.new_local_identifier("err");
    let body = body.unwrap_or_else(|| vec![plain_return(GoExpression::identifier(error.clone()))]);
    let condition = GoExpression::binary(
        GoExpression::identifier(error.clone()),
        if success { BinaryOp::Eq } else { BinaryOp::Ne },
        GoExpression::nil(),
    );
    let mut plan = IfPlan::plain(condition, LoweredBlock { statements: body }, ElseArm::None);
    plan.initializer = Some(GoDefinition {
        names: vec![error],
        value: unmarshal(value, target),
    });
    LoweredStatement::If(plan).into()
}

fn comment_line(line: &str) -> String {
    if line.is_empty() {
        "//".to_string()
    } else {
        format!("// {line}")
    }
}

fn unmarshal_tuple_fields(
    planner: &mut Planner,
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
                json_guard(planner, value, target, None, false)
            }
        })
        .collect()
}
