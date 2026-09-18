//! Hard errors over generic-parameter shapes.

use diagnostics::LocalSink;
use syntax::ast::{Expression, Generic, Span};
use syntax::types::Type;

use semantics::generics::{
    bound_implied, bound_requires_evidence, nested_type_obligations, type_obligations,
};
use semantics::store::Store;

#[derive(Clone, Copy)]
struct GenericContext<'a> {
    generics: &'a [Generic],
    receiver: Option<&'a Type>,
}

pub(crate) fn run(typed_ast: &[Expression], store: &Store, sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, None, store, sink);
    }
}

fn visit_expression(
    expression: &Expression,
    enclosing: Option<GenericContext<'_>>,
    store: &Store,
    sink: &LocalSink,
) {
    match expression {
        Expression::ImplBlock {
            methods,
            generics,
            ty,
            ..
        } => {
            let context = GenericContext {
                generics,
                receiver: Some(ty),
            };
            for method in methods {
                visit_expression(method, Some(context), store, sink);
            }
            return;
        }
        Expression::Interface {
            method_signatures,
            generics,
            ..
        } => {
            let context = GenericContext {
                generics,
                receiver: None,
            };
            for method in method_signatures {
                visit_expression(method, Some(context), store, sink);
            }
            return;
        }
        Expression::Function { .. } => {
            check_constrained_return_type(expression, enclosing, store, sink);
        }
        Expression::Call {
            expression: callee,
            span,
            ..
        } => {
            let callee_ty = callee.get_type();
            if !callee_ty.get_bounds().is_empty() {
                check_unconstrained_bounded(&callee_ty, callee_name(callee).as_deref(), span, sink);
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, enclosing, store, sink);
    }
}

/// The callee as written, so the suggested type argument pastes back.
fn callee_name(callee: &Expression) -> Option<String> {
    match callee.unwrap_parens() {
        Expression::Identifier { value, .. } => Some(value.to_string()),
        Expression::DotAccess {
            expression, member, ..
        } => Some(match expression.get_var_name() {
            Some(base) => format!("{base}.{member}"),
            None => member.to_string(),
        }),
        _ => None,
    }
}

fn check_unconstrained_bounded(
    callee_ty: &Type,
    callee: Option<&str>,
    span: &Span,
    sink: &LocalSink,
) {
    let signature = callee_ty.as_function_type();
    let example_args = match signature {
        Some(f) if f.params.is_empty() => "()",
        _ => "(...)",
    };
    let example = format!("{}<SomeType>{example_args}", callee.unwrap_or("f"));
    for bound in callee_ty.get_bounds() {
        if !matches!(&bound.generic, Type::Var { .. }) {
            continue;
        }
        // Absent from the signature, no call could ever pin the parameter down.
        let in_signature = signature.is_some_and(|f| {
            f.params.iter().any(|p| p.ty.contains_type(&bound.generic))
                || f.return_type.contains_type(&bound.generic)
        });
        sink.push(diagnostics::infer::unconstrained_type_param(
            &bound.param_name,
            &example,
            in_signature,
            *span,
        ));
    }
}

fn check_constrained_return_type(
    function: &Expression,
    enclosing: Option<GenericContext<'_>>,
    store: &Store,
    sink: &LocalSink,
) {
    let Expression::Function {
        name: fn_name,
        generics,
        return_annotation,
        return_type: return_ty,
        ..
    } = function
    else {
        return;
    };
    let span = return_annotation.get_span();
    let mut seen = rustc_hash::FxHashSet::default();
    for applied in nested_type_obligations(store, return_ty) {
        let Type::Parameter(param_name) = &applied.argument else {
            continue;
        };
        if !bound_requires_evidence(store, &applied.required)
            || !seen.insert((param_name.clone(), applied.required.to_string()))
        {
            continue;
        }
        let available = generics
            .iter()
            .find(|generic| generic.name == *param_name)
            .map(|generic| {
                generic
                    .resolved_bounds()
                    .expect("generic bounds must be resolved before checks")
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(|| enclosing_parameter_bounds(store, enclosing, param_name));
        if !bound_implied(store, &available, &applied.required) {
            sink.push(
                diagnostics::infer::missing_constraint_on_generic_return_type(
                    fn_name,
                    param_name,
                    &applied.required,
                    span,
                ),
            );
        }
    }
}

fn enclosing_parameter_bounds(
    store: &Store,
    context: Option<GenericContext<'_>>,
    parameter: &str,
) -> Vec<Type> {
    let Some(context) = context else {
        return Vec::new();
    };
    let mut available = context
        .generics
        .iter()
        .find(|generic| generic.name == parameter)
        .map_or_else(Vec::new, |generic| {
            generic
                .resolved_bounds()
                .expect("generic bounds must be resolved before checks")
                .cloned()
                .collect()
        });
    if let Some(receiver) = context.receiver {
        available.extend(
            type_obligations(store, receiver)
                .into_iter()
                .filter_map(|obligation| {
                    (obligation.argument == Type::Parameter(parameter.into()))
                        .then_some(obligation.required)
                }),
        );
    }
    available
}
