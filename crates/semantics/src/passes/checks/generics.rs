//! Hard errors over generic-parameter shapes.
//!
//! Lint-style fact production for unused/bound-only type params lives in
//! `passes::fact_producers::generics`.

use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::{Annotation, Expression, Generic, Span};
use syntax::types::{Bound, Symbol, Type};

use crate::store::Store;

pub(crate) fn run(typed_ast: &[Expression], module_id: &str, store: &Store, sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, None, module_id, store, sink);
    }
}

fn visit_expression(
    expression: &Expression,
    enclosing_impl_generics: Option<&[Generic]>,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    match expression {
        Expression::ImplBlock {
            methods, generics, ..
        } => {
            for method in methods {
                visit_expression(method, Some(generics), module_id, store, sink);
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
                enclosing_impl_generics,
                return_annotation,
                name,
                module_id,
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
        visit_expression(child, enclosing_impl_generics, module_id, store, sink);
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
    enclosing_impl_generics: Option<&[Generic]>,
    return_annotation: &Annotation,
    fn_name: &str,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    let Type::Nominal { id, params, .. } = return_ty else {
        return;
    };

    if params.is_empty() {
        return;
    }

    let qualified_id =
        lookup_qualified_name(id, module_id, store).unwrap_or_else(|| id.to_string());
    let Some(methods) = store.get_own_methods(&qualified_id) else {
        return;
    };

    let mut required_bounds: HashMap<String, Vec<Type>> = HashMap::default();
    for method_ty in methods.values() {
        let fn_ty = match method_ty {
            Type::Forall { body, .. } => body.as_ref(),
            other => other,
        };
        if let Type::Function { bounds, .. } = fn_ty {
            for bound in bounds {
                if let Type::Parameter(param_name) = &bound.generic {
                    let entry = required_bounds.entry(param_name.to_string()).or_default();
                    if !entry.contains(&bound.ty) {
                        entry.push(bound.ty.clone());
                    }
                }
            }
        }
    }

    let span = return_annotation.get_span();
    for return_param in params.iter() {
        if let Type::Parameter(param_name) = return_param
            && let Some(method_bounds) = required_bounds.get(param_name.as_ref())
        {
            let fn_generic = generics
                .iter()
                .find(|g| g.name.as_ref() == param_name.as_ref());

            if let Some(fn_gen) = fn_generic {
                let fn_bounds: Vec<Type> = fn_gen
                    .bounds
                    .iter()
                    .filter_map(|b| annotation_to_type(b, module_id, store))
                    .collect();

                for method_bound in method_bounds {
                    if !fn_bounds.iter().any(|fb| fb == method_bound) {
                        sink.push(
                            diagnostics::infer::missing_constraint_on_generic_return_type(
                                fn_name,
                                param_name.as_ref(),
                                method_bound,
                                span,
                            ),
                        );
                    }
                }
            } else if let Some(impl_generics) = enclosing_impl_generics {
                let impl_bounds: Vec<Type> = impl_generics
                    .iter()
                    .filter(|g| g.name.as_ref() == param_name.as_ref())
                    .flat_map(|g| g.bounds.iter())
                    .filter_map(|b| annotation_to_type(b, module_id, store))
                    .collect();
                let all_covered = method_bounds.iter().all(|mb| impl_bounds.contains(mb));
                if !all_covered {
                    let bound_str = method_bounds
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(" + ");
                    sink.push(
                        diagnostics::infer::missing_constraint_on_generic_return_type(
                            fn_name,
                            param_name.as_ref(),
                            &Type::Parameter(bound_str.into()),
                            span,
                        ),
                    );
                }
            } else {
                let bound_str = method_bounds
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join(" + ");
                sink.push(
                    diagnostics::infer::missing_constraint_on_generic_return_type(
                        fn_name,
                        param_name.as_ref(),
                        &Type::Parameter(bound_str.into()),
                        span,
                    ),
                );
            }
        }
    }
}

fn lookup_qualified_name(id: &str, module_id: &str, store: &Store) -> Option<String> {
    if id.contains('.') {
        return Some(id.to_string());
    }

    let candidate = Symbol::from_parts(module_id, id);
    if store
        .get_module(module_id)
        .is_some_and(|m| m.definitions.contains_key(candidate.as_str()))
    {
        return Some(candidate.to_string());
    }
    None
}

fn annotation_to_type(annotation: &Annotation, module_id: &str, store: &Store) -> Option<Type> {
    let Annotation::Constructor { name, params, .. } = annotation else {
        return None;
    };
    let qualified = lookup_qualified_name(name, module_id, store)?;
    let ty = store.get_type(&qualified)?.clone();
    let instantiated = match &ty {
        Type::Forall { body, .. } if params.is_empty() => (**body).clone(),
        _ => ty,
    };
    Some(instantiated)
}
