use crate::_harness::infer;
use syntax::ast::{
    Annotation, BinaryOperator, CallTypeArguments, ConstructorPatternResolution, Expression,
    IfLetAlternative, Pattern, SelectArm, Span, StructFieldKind, StructFields,
};
use syntax::lex::Lexer;
use syntax::parse::Parser;
use syntax::program::{
    BindingMutation, Definition, DefinitionBody, EqualityIndex, File, Module, MutationInfo,
    ValueKind, Visibility,
};
use syntax::types::{FunctionParameter, SubstitutionMap, Type, TypeVarId, substitute};

fn walk(expression: &Expression, visit: &mut impl FnMut(&Expression)) {
    visit(expression);
    for child in expression.children() {
        walk(child, visit);
    }
}

#[test]
fn zero_width_source_spans_are_not_dummy_spans() {
    let source_position = Span::new(0, 12, 0);

    assert_eq!(
        (source_position.is_dummy(), Span::dummy().is_dummy()),
        (false, true)
    );
}

#[test]
fn span_merge_is_order_independent() {
    let earlier = Span::new(3, 10, 4);
    let later = Span::new(3, 20, 2);
    let merged = Span::new(3, 10, 12);

    assert_eq!(earlier.merge(later), merged);
    assert_eq!(later.merge(earlier), merged);
}

#[test]
fn function_parameter_metadata_survives_type_substitution() {
    let function = Type::function(
        vec![FunctionParameter::named(
            Type::Parameter("T".into()),
            Some("value".into()),
            true,
        )],
        vec![],
        Box::new(Type::Parameter("T".into())),
    );
    let mut substitutions = SubstitutionMap::default();
    substitutions.insert("T".into(), Type::int());

    let Type::Function(substituted) = substitute(&function, &substitutions) else {
        panic!("expected a function type");
    };
    let [parameter] = substituted.params.as_slice() else {
        panic!("expected one parameter");
    };

    assert_eq!(parameter.ty, Type::int());
    assert_eq!(parameter.name.as_deref(), Some("value"));
    assert!(parameter.mutable);
    assert_eq!(*substituted.return_type, Type::int());
}

#[test]
fn alias_mutation_is_not_downgraded_by_a_direct_mark() {
    let mut mutations = MutationInfo::default();
    mutations.record(7, BindingMutation::ThroughAlias);
    mutations.record(7, BindingMutation::Direct);

    assert_eq!(mutations.mutation(7), Some(BindingMutation::ThroughAlias));
    assert_eq!(mutations.mutation(8), None);
}

#[test]
fn equality_index_has_one_visibility_rule_for_all_kinds() {
    let mut index = EqualityIndex::default();
    index.insert_declared_method("public".into(), None);
    index.insert_synthesized_method("private".into(), Some("module".into()));
    index.insert_ufcs_lowered("ufcs".into(), Some("module".into()));

    assert!(index.usable_from("public", "other"));
    assert!(index.usable_from("private", "module"));
    assert!(!index.usable_from("private", "other"));
    assert!(index.is_ufcs_lowered_from("ufcs", "module"));
    assert!(!index.is_ufcs_lowered_from("ufcs", "other"));
}

#[test]
fn nested_type_closers_do_not_consume_shift_operators() {
    let source = r#"
fn test(value: int) {
  let nested = build<Slice<Option<int>>>();
  let shifted = value >> 1;
}
"#;
    let lexed = Lexer::new(source, 0).lex();
    assert!(lexed.errors.is_empty(), "{:?}", lexed.errors);
    let parsed = Parser::new(lexed.tokens, source).parse();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);

    let mut saw_unresolved_type_arguments = false;
    let mut saw_shift = false;
    for item in &parsed.ast {
        walk(item, &mut |expression| match expression {
            Expression::Call { type_arguments, .. } if !type_arguments.is_empty() => {
                saw_unresolved_type_arguments = true;
                assert_eq!(type_arguments.annotations().len(), 1);
                assert!(type_arguments.resolved_types().is_none());
            }
            Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                ..
            } => saw_shift = true,
            _ => {}
        });
    }

    assert!(saw_unresolved_type_arguments);
    assert!(saw_shift);
}

#[test]
fn struct_shape_owns_its_field_collection() {
    let source = "struct Record { value: int }\nstruct Tuple(int)";
    let lexed = Lexer::new(source, 0).lex();
    assert!(lexed.errors.is_empty(), "{:?}", lexed.errors);
    let parsed = Parser::new(lexed.tokens, source).parse();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);

    let [
        Expression::Struct {
            fields: StructFields::Record(record_fields),
            ..
        },
        Expression::Struct {
            fields: StructFields::Tuple(tuple_fields),
            ..
        },
    ] = parsed.ast.as_slice()
    else {
        panic!("expected record and tuple struct definitions");
    };

    assert_eq!(record_fields.len(), 1);
    assert_eq!(tuple_fields.len(), 1);
}

#[test]
fn struct_field_kind_owns_only_valid_metadata() {
    let result = syntax::build_ast(
        r#"
struct Inner {}
struct Outer {
  #[json(omitempty)]
  value: int,
  embed Inner,
}
"#,
        0,
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let Expression::Struct {
        fields: StructFields::Record(fields),
        ..
    } = &result.ast[1]
    else {
        panic!("expected a record struct");
    };
    let [named, embedded] = fields.as_slice() else {
        panic!("expected named and embedded fields");
    };

    assert!(matches!(
        &named.kind,
        StructFieldKind::Named { attributes } if attributes.len() == 1
    ));
    assert!(matches!(&embedded.kind, StructFieldKind::Embedded));
    assert!(embedded.attributes().is_empty());
}

#[test]
fn select_arms_are_their_pattern_variants() {
    let result = syntax::build_ast(
        "fn test(ch: Receiver<int>) { select { let value = ch => value, _ => 0 } }",
        0,
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let mut kinds = None;
    walk(&result.ast[0], &mut |expression| {
        if let Expression::Select { arms, .. } = expression {
            kinds = Some((
                matches!(arms.first(), Some(SelectArm::Receive { .. })),
                matches!(arms.get(1), Some(SelectArm::WildCard { .. })),
            ));
        }
    });

    assert_eq!(kinds, Some((true, true)));
}

#[test]
fn conditional_alternatives_encode_presence_directly() {
    let result = syntax::build_ast(
        r#"
fn main() {
  let plain = if true { 1 }
  let with_else = if false { 1 } else { 2 }
  let plain_let = if let Some(value) = Some(1) { value }
  let let_with_else = if let Some(value) = Some(1) { value } else { 0 }
}
"#,
        0,
    );
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let mut plain_if = false;
    let mut if_with_else = false;
    let mut plain_if_let = false;
    let mut if_let_with_else = false;
    for item in &result.ast {
        walk(item, &mut |expression| match expression {
            Expression::If { alternative, .. } => {
                if alternative.is_none() {
                    plain_if = true;
                } else {
                    if_with_else = true;
                }
            }
            Expression::IfLet { alternative, .. } => match alternative {
                IfLetAlternative::Absent => plain_if_let = true,
                IfLetAlternative::Present { else_span, .. } => {
                    assert_eq!(else_span.byte_length, 4);
                    if_let_with_else = true;
                }
            },
            _ => {}
        });
    }

    assert!(plain_if);
    assert!(if_with_else);
    assert!(plain_if_let);
    assert!(if_let_with_else);
}

#[test]
fn inference_transitions_explicit_type_arguments_to_resolved() {
    let result = infer(
        r#"
fn identity<T>(value: T) -> T { value }
fn main() -> int { identity<int>(1) }
"#,
    )
    .assert_no_errors();

    let mut checked = None;
    for item in &result.ast {
        walk(item, &mut |expression| {
            if let Expression::Call { type_arguments, .. } = expression
                && !type_arguments.is_empty()
            {
                checked = Some(type_arguments.clone());
            }
        });
    }

    let checked = checked.expect("expected an explicitly instantiated call");
    assert_eq!(checked.annotations().len(), 1);
    let resolved = checked
        .resolved_types()
        .expect("checked call must have resolved type arguments");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0], Type::int());
}

#[test]
fn checked_call_type_arguments_cannot_have_misaligned_types() {
    let resolved = CallTypeArguments::resolved([(Annotation::Unknown, Type::int())]);
    assert_eq!(resolved.annotations().len(), 1);
    let types = resolved.resolved_types().expect("call is checked");
    assert_eq!(types.len(), 1);
    assert_eq!(types[0], Type::int());

    let const_like = CallTypeArguments::checked_without_types(vec![Annotation::Unknown]);
    assert_eq!(const_like.annotations().len(), 1);
    assert!(
        const_like
            .resolved_types()
            .expect("call is checked")
            .is_empty()
    );
}

#[test]
fn placeholder_types_are_not_reserved_type_variable_ids() {
    let variable = Type::Var {
        id: TypeVarId::new(u32::MAX),
        hint: None,
    };

    assert!(variable.is_variable());
    assert_eq!(variable, variable.clone());
    assert!(!Type::uninferred().is_variable());
    assert!(!Type::ignored().is_variable());
    assert!(Type::uninferred().is_uninferred());
    assert!(Type::ignored().is_ignored());
}

#[test]
fn statement_only_try_tail_has_unit_success_type() {
    let result = infer(
        r#"
fn test() -> Result<(), string> {
  let mut value = 0
  try {
    let _ = Ok(1)?
    value = 1
  }
}
"#,
    )
    .assert_no_errors();

    let mut success_type = None;
    for item in &result.ast {
        walk(item, &mut |expression| {
            if let Expression::TryBlock { ty, .. } = expression {
                success_type = Some(ty.ok_type());
            }
        });
    }

    assert_eq!(success_type, Some(Type::unit()));
}

#[test]
fn generic_bounds_have_an_explicit_resolution_transition() {
    let source = "fn constrained<T: Comparable>(value: T) -> T { value }";
    let parsed = syntax::build_ast(source, 0);
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let [Expression::Function { generics, .. }] = parsed.ast.as_slice() else {
        panic!("expected one function");
    };
    let [generic] = generics.as_slice() else {
        panic!("expected one generic");
    };
    assert_eq!(generic.bound_count(), 1);
    assert!(!generic.bounds_are_resolved());
    assert!(generic.resolved_bounds().is_none());

    let inferred = infer(source).assert_no_errors();
    let generic = inferred
        .ast
        .iter()
        .find_map(|item| match item {
            Expression::Function { name, generics, .. } if name == "constrained" => {
                generics.first()
            }
            _ => None,
        })
        .expect("expected constrained function");
    assert!(generic.bounds_are_resolved());
    assert_eq!(generic.resolved_bounds().unwrap().count(), 1);
}

#[test]
fn inference_enriches_the_canonical_pattern_tree() {
    let source = r#"
enum Item { Value(int) }

fn unwrap(item: Item) -> int {
  match item {
    Item.Value(value) => value,
  }
}
"#;

    let parsed = syntax::build_ast(source, 0);
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let mut parsed_pattern = None;
    for item in &parsed.ast {
        walk(item, &mut |expression| {
            if let Expression::Match { arms, .. } = expression {
                parsed_pattern = arms.first().map(|arm| arm.pattern.clone());
            }
        });
    }
    assert!(matches!(
        parsed_pattern,
        Some(Pattern::EnumVariant {
            resolution: ConstructorPatternResolution::Unresolved,
            ..
        })
    ));

    let inferred = infer(source).assert_no_errors();
    let mut checked_pattern = None;
    for item in &inferred.ast {
        walk(item, &mut |expression| {
            if let Expression::Match { arms, .. } = expression {
                checked_pattern = arms.first().map(|arm| arm.pattern.clone());
            }
        });
    }
    let Some(Pattern::EnumVariant {
        fields,
        ty,
        resolution:
            ConstructorPatternResolution::EnumVariant {
                enum_name,
                variant_name,
            },
        ..
    }) = checked_pattern
    else {
        panic!("expected a resolved constructor pattern");
    };
    assert_eq!(variant_name, "Item.Value");
    assert_eq!(ty.get_qualified_id(), Some(enum_name.as_str()));
    assert!(
        matches!(fields.as_slice(), [Pattern::Identifier { identifier, .. }] if identifier == "value")
    );
}

#[test]
fn inferred_signatures_retain_transparent_alias_identity() {
    let result = infer(
        r#"
type UserId = int
fn identity(value: UserId) -> UserId { value }
"#,
    )
    .assert_no_errors();

    let function_ty = result
        .ast
        .iter()
        .find_map(|expression| match expression {
            Expression::Function { name, ty, .. } if name == "identity" => ty.as_function_type(),
            _ => None,
        })
        .expect("expected identity's function type");
    let [parameter] = function_ty.params.as_slice() else {
        panic!("expected one parameter");
    };

    for ty in [&parameter.ty, function_ty.return_type.as_ref()] {
        assert!(
            matches!(ty, Type::Nominal { id, params } if id.last_segment() == "UserId" && params.is_empty()),
            "transparent alias identity was lost: {ty:?}"
        );
    }
}

fn value_definition(kind: ValueKind) -> Definition {
    Definition {
        visibility: Visibility::Private,
        ty: Type::int(),
        name: None,
        name_span: None,
        doc: None,
        body: DefinitionBody::Value {
            kind,
            allowed_lints: vec![],
            go_hints: vec![],
            go_name: None,
            go_type_param_recipe: None,
        },
    }
}

#[test]
fn nonliteral_constants_remain_distinguishable_from_runtime_values() {
    let constant = value_definition(ValueKind::ConstantDeclaration);
    let runtime = value_definition(ValueKind::Runtime);

    assert!(constant.is_const());
    assert!(constant.const_value().is_none());
    assert!(!runtime.is_const());
}

#[test]
fn module_derives_file_classification_from_each_file() {
    let mut module = Module::new("example");
    let source = File::new_cached("example", "main.lis", "main.lis", "", 1);
    let typedef = File::new_cached("example", "native.d.lis", "native.d.lis", "", 2);
    module.files.insert(source.id, source);
    module.files.insert(typedef.id, typedef);

    assert_eq!(module.source_files().count(), 1);
    assert_eq!(module.typedef_files().count(), 1);
    assert_eq!(module.file_ids().collect::<Vec<_>>(), vec![1]);
    assert!(module.get_file(2).is_some());
    assert!(module.is_typedef(2));
}

#[test]
fn recursion_depth_is_scoped_to_each_top_level_item() {
    let source = (0..80)
        .map(|index| format!("fn item_{index}() -> int {{ 1 }}\n"))
        .collect::<String>();
    let parsed = syntax::build_ast(&source, 0);

    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    assert_eq!(parsed.ast.len(), 80);
}
