//! Generic-parameter shape checks.
//!
//! - **Unused type parameters** — a generic declared on a function that never
//!   appears in its signature is likely a typo; record as a fact for the
//!   lint layer.
//! - **Type parameters only in a bound** — a generic that appears only inside
//!   `T: SomeTrait` but never in a parameter or the return type cannot be
//!   inferred from a call site; record as a fact.
//! - **Unconstrained bounded generics at call sites** — at a call, any bound
//!   whose generic position is still an unbound `Type::Var` after freeze
//!   means the caller failed to constrain it; emit a hard error.
//! - **Missing bounds on generic return types** — a function returning a
//!   nominal type whose methods require bounds on a type parameter must
//!   declare those bounds on its own generic; emit a hard error.

use diagnostics::LocalSink;
use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::ast::{Annotation, Binding, Expression, Generic, Span};
use syntax::types::{Bound, Symbol, Type};

use crate::facts::Facts;
use crate::store::Store;

pub(super) fn run(
    typed_ast: &[Expression],
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
    sink: &LocalSink,
) {
    for item in typed_ast {
        visit_expression(item, None, module_id, store, facts, sink);
    }
}

fn visit_expression(
    expression: &Expression,
    enclosing_impl_generics: Option<&[Generic]>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
    sink: &LocalSink,
) {
    match expression {
        Expression::ImplBlock {
            methods, generics, ..
        } => {
            for method in methods {
                visit_expression(method, Some(generics), module_id, store, facts, sink);
            }
            return;
        }
        Expression::Function {
            name,
            generics,
            params,
            return_annotation,
            return_type,
            ..
        } => {
            check_unused_type_parameters(generics, params, return_type, facts);
            check_type_params_only_in_bound(generics, params, return_type, facts);
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
        visit_expression(
            child,
            enclosing_impl_generics,
            module_id,
            store,
            facts,
            sink,
        );
    }
}

fn check_unused_type_parameters(
    generics: &[Generic],
    params: &[Binding],
    return_type: &Type,
    facts: &mut Facts,
) {
    if generics.is_empty() {
        return;
    }

    let mut remaining: HashSet<EcoString> = generics.iter().map(|g| g.name.clone()).collect();
    for param in params {
        param.ty.remove_found_type_names(&mut remaining);
    }
    return_type.remove_found_type_names(&mut remaining);
    for generic in generics {
        for bound in &generic.bounds {
            annotation_remove_names(bound, &mut remaining);
        }
    }

    for generic in generics {
        if generic.name.starts_with('_') {
            continue;
        }

        if remaining.contains(&generic.name) {
            facts.add_unused_type_param(generic.name.to_string(), generic.span);
        }
    }
}

fn check_type_params_only_in_bound(
    generics: &[Generic],
    params: &[Binding],
    return_type: &Type,
    facts: &mut Facts,
) {
    if generics.is_empty() {
        return;
    }
    if generics.iter().all(|g| g.bounds.is_empty()) {
        return;
    }

    let only_in_bound = collect_type_params_only_in_bound(generics, params, return_type);
    if only_in_bound.is_empty() {
        return;
    }

    for generic in generics {
        if generic.name.starts_with('_') || !only_in_bound.contains(&generic.name) {
            continue;
        }
        facts.add_type_param_only_in_bound(generic.name.to_string(), generic.span);
    }
}

fn collect_type_params_only_in_bound(
    generics: &[Generic],
    params: &[Binding],
    return_type: &Type,
) -> HashSet<EcoString> {
    let mut unseen_outside_bound_rhs: HashSet<EcoString> =
        generics.iter().map(|g| g.name.clone()).collect();
    for param in params {
        param
            .ty
            .remove_found_type_names(&mut unseen_outside_bound_rhs);
    }
    return_type.remove_found_type_names(&mut unseen_outside_bound_rhs);

    let mut unseen_anywhere = unseen_outside_bound_rhs.clone();
    for generic in generics {
        for bound in &generic.bounds {
            annotation_remove_names(bound, &mut unseen_anywhere);
        }
    }

    unseen_outside_bound_rhs
        .into_iter()
        .filter(|name| !unseen_anywhere.contains(name))
        .collect()
}

fn annotation_remove_names(annotation: &Annotation, names: &mut HashSet<EcoString>) {
    match annotation {
        Annotation::Constructor { name, params, .. } => {
            names.remove(name.as_str());
            for p in params {
                annotation_remove_names(p, names);
            }
        }
        Annotation::Function {
            params,
            return_type,
            ..
        } => {
            for p in params {
                annotation_remove_names(p, names);
            }
            annotation_remove_names(return_type, names);
        }
        Annotation::Tuple { elements, .. } => {
            for e in elements {
                annotation_remove_names(e, names);
            }
        }
        Annotation::Unknown | Annotation::Opaque { .. } => {}
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
