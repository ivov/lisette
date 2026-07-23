use crate::_harness::infer;
use syntax::ast::{BinaryOperator, Expression};
use syntax::lex::Lexer;
use syntax::parse::Parser;
use syntax::program::{BindingMutation, MutationInfo};
use syntax::types::{FunctionParameter, SubstitutionMap, Type, substitute};

fn walk(expression: &Expression, visit: &mut impl FnMut(&Expression)) {
    visit(expression);
    for child in expression.children() {
        walk(child, visit);
    }
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

    assert_eq!(mutations.mutation(7), BindingMutation::ThroughAlias);
    assert_eq!(mutations.mutation(8), BindingMutation::Unchanged);
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
    assert_eq!(checked.resolved_types(), Some([Type::int()].as_slice()));
}
