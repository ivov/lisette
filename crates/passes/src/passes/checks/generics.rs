//! Hard errors over generic-parameter shapes.
//!
//! Lint-style fact production for unused/bound-only type params lives in
//! `passes::fact_producers::generics`.

use diagnostics::LocalSink;
use syntax::ast::{Annotation, Expression, Generic, Span};
use syntax::types::{Bound, Type};

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
        Expression::Function {
            name,
            generics,
            return_annotation,
            return_type,
            ..
        } => {
            check_constrained_return_type(
                return_type,
                generics,
                enclosing,
                return_annotation,
                name,
                store,
                sink,
            );
        }
        Expression::Call {
            expression: callee,
            span,
            ..
        } => {
            let callee_ty = callee.get_type();
            let bounds = callee_ty.get_bounds();
            if !bounds.is_empty() {
                check_unconstrained_bounded(bounds, span, sink);
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, enclosing, store, sink);
    }
}

fn check_unconstrained_bounded(bounds: &[Bound], span: &Span, sink: &LocalSink) {
    for bound in bounds {
        if matches!(&bound.generic, Type::Var { .. }) {
            sink.push(diagnostics::infer::unconstrained_type_param(
                &bound.param_name,
                *span,
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_constrained_return_type(
    return_ty: &Type,
    generics: &[Generic],
    enclosing: Option<GenericContext<'_>>,
    return_annotation: &Annotation,
    fn_name: &str,
    store: &Store,
    sink: &LocalSink,
) {
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
            .map(|generic| generic.resolved_bounds.clone())
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
        .map_or_else(Vec::new, |generic| generic.resolved_bounds.clone());
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
