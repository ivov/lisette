use std::ptr;

use diagnostics::LocalSink;
use syntax::ast::{Expression, Generic, Span};
use syntax::program::{GoTypeTerm, GoTypeTerms, resolved_callee, resolved_instantiation};
use syntax::types::{CompoundKind, SimpleKind, SubstitutionMap, Type, unqualified_name};

use semantics::store::Store;

const ORDERED: [SimpleKind; 14] = [
    SimpleKind::Int,
    SimpleKind::Int8,
    SimpleKind::Int16,
    SimpleKind::Int32,
    SimpleKind::Int64,
    SimpleKind::Uint,
    SimpleKind::Uint8,
    SimpleKind::Uint16,
    SimpleKind::Uint32,
    SimpleKind::Uint64,
    SimpleKind::Uintptr,
    SimpleKind::Float32,
    SimpleKind::Float64,
    SimpleKind::String,
];

pub(crate) fn run(items: &[Expression], store: &Store, sink: &LocalSink) {
    let mut scope = Vec::new();
    for item in items {
        visit(item, &mut scope, None, store, sink);
    }
}

fn visit<'a>(
    expression: &'a Expression,
    scope: &mut Vec<&'a Generic>,
    call_span: Option<Span>,
    store: &Store,
    sink: &LocalSink,
) {
    let depth = scope.len();
    match expression {
        Expression::Function { generics, .. } | Expression::ImplBlock { generics, .. } => {
            scope.extend(generics);
        }
        Expression::Identifier { .. } | Expression::DotAccess { .. } => {
            if let Some(instantiation) =
                resolved_instantiation(expression).filter(|instantiation| !instantiation.is_empty())
                && let Some(name) = resolved_callee(expression)
            {
                let name = name.qualified_name();
                let reference = Some((name.as_str(), instantiation));
                check_reference(expression, reference, scope, store, sink, call_span);
            }
        }
        _ => {}
    }
    let callee = match expression {
        Expression::Call {
            expression: callee,
            span,
            ..
        } => Some((callee.as_ref(), *span)),
        _ => None,
    };
    expression.for_each_child(|child| {
        let call_span = callee
            .filter(|(callee, _)| ptr::eq(*callee, child))
            .map(|(_, span)| span);
        visit(child, scope, call_span, store, sink);
    });
    scope.truncate(depth);
}

fn check_reference(
    expression: &Expression,
    reference: Option<(&str, &SubstitutionMap)>,
    scope: &[&Generic],
    store: &Store,
    sink: &LocalSink,
    call_span: Option<Span>,
) {
    let Some((name, instantiation)) = reference else {
        return;
    };
    let Some(definition) = store.get_definition(name) else {
        return;
    };
    let span = call_span.unwrap_or_else(|| expression.get_span());
    for constraint in definition.go_type_terms() {
        let Some(argument) = instantiation.get(&constraint.parameter) else {
            continue;
        };
        let function = expression
            .as_dotted_path()
            .unwrap_or_else(|| unqualified_name(name).to_string());
        if call_span.is_none() && store.peel_alias(argument).has_unbound_variables() {
            sink.push(diagnostics::infer::cannot_infer_bounded_function_reference(
                &function,
                &constraint.parameter,
                &constraint.display_terms(),
                span,
            ));
            continue;
        }
        if satisfies(store, argument, constraint, scope) {
            continue;
        }
        sink.push(diagnostics::infer::type_outside_constraint(
            &argument.without_permissions().to_string(),
            matches!(store.peel_alias(argument), Type::Parameter(_)),
            &constraint.parameter,
            &function,
            &constraint.display_terms(),
            span,
        ));
    }
}

fn satisfies(store: &Store, argument: &Type, constraint: &GoTypeTerms, scope: &[&Generic]) -> bool {
    let argument = store.peel_alias(argument);
    if argument.is_variable() || argument.contains_error() {
        return true;
    }
    let Type::Parameter(name) = &argument else {
        return constraint
            .terms
            .iter()
            .any(|term| includes(store, term, &argument));
    };
    let is_ordered = scope
        .iter()
        .rev()
        .find(|generic| &generic.name == name)
        .and_then(|generic| generic.resolved_bounds())
        .is_some_and(|mut bounds| {
            bounds.any(|bound| {
                matches!(
                    bound.get_qualified_id(),
                    Some("go:cmp.Ordered" | "prelude.Ordered")
                )
            })
        });
    is_ordered
        && ORDERED.iter().all(|kind| {
            constraint.terms.iter().any(|term| {
                term.tilde && matches!(term.ty, Type::Simple(term) if go_kind(term) == *kind)
            })
        })
}

fn includes(store: &Store, term: &GoTypeTerm, argument: &Type) -> bool {
    let argument = if term.tilde {
        store.peel_underlying(argument)
    } else {
        argument.clone()
    };
    identical(store, &argument, &term.ty)
}

fn identical(store: &Store, left: &Type, right: &Type) -> bool {
    match (store.peel_alias(left), store.peel_alias(right)) {
        (Type::Simple(left), Type::Simple(right)) => go_kind(left) == go_kind(right),
        (
            Type::Compound {
                kind: left_kind,
                args: left_args,
                ..
            },
            Type::Compound {
                kind: right_kind,
                args: right_args,
                ..
            },
        ) => {
            left_kind == right_kind
                && !matches!(left_kind, CompoundKind::VarArgs)
                && left_args.len() == right_args.len()
                && left_args
                    .iter()
                    .zip(&right_args)
                    .all(|(left, right)| identical(store, left, right))
        }
        (
            Type::Array {
                length: left_length,
                element: left_element,
            },
            Type::Array {
                length: right_length,
                element: right_element,
            },
        ) => left_length == right_length && identical(store, &left_element, &right_element),
        (left @ Type::Nominal { .. }, right @ Type::Nominal { .. }) => {
            left.without_permissions() == right.without_permissions()
        }
        _ => false,
    }
}

fn go_kind(kind: SimpleKind) -> SimpleKind {
    match kind {
        SimpleKind::Byte => SimpleKind::Uint8,
        SimpleKind::Rune => SimpleKind::Int32,
        other => other,
    }
}
