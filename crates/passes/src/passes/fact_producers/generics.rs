//! Generic-parameter fact producers for the lint layer.
//!
//! Records facts via `LocalFacts`; rendering happens later in `lints::from_facts`.

use ecow::EcoString;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Annotation, Binding, Expression, Generic};
use syntax::types::Type;

use semantics::facts::LocalFacts;

pub(crate) fn run(typed_ast: &[Expression], local: &mut LocalFacts) {
    for item in typed_ast {
        visit_expression(item, local);
    }
}

fn visit_expression(expression: &Expression, local: &mut LocalFacts) {
    match expression {
        Expression::ImplBlock { methods, .. } => {
            for method in methods {
                visit_expression(method, local);
            }
            return;
        }
        Expression::Function {
            generics,
            params,
            return_type,
            body,
            ..
        } => {
            if !generics.is_empty() {
                let mut still_missing: HashSet<EcoString> =
                    generics.iter().map(|g| g.name.clone()).collect();
                body_remove_found_type_names(body, &mut still_missing);
                let found_in_body: HashSet<EcoString> = generics
                    .iter()
                    .map(|g| g.name.clone())
                    .filter(|name| !still_missing.contains(name))
                    .collect();
                check_unused_type_parameters(generics, params, return_type, &found_in_body, local);
                check_type_params_only_in_bound(
                    generics,
                    params,
                    return_type,
                    &found_in_body,
                    local,
                );
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, local);
    }
}

fn check_unused_type_parameters(
    generics: &[Generic],
    params: &[Binding],
    return_type: &Type,
    found_in_body: &HashSet<EcoString>,
    local: &mut LocalFacts,
) {
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
    remaining.retain(|name| !found_in_body.contains(name));

    for generic in generics {
        if generic.name.starts_with('_') {
            continue;
        }

        if remaining.contains(&generic.name) {
            local.add_unused_type_param(generic.name.to_string(), generic.span);
        }
    }
}

fn check_type_params_only_in_bound(
    generics: &[Generic],
    params: &[Binding],
    return_type: &Type,
    found_in_body: &HashSet<EcoString>,
    local: &mut LocalFacts,
) {
    if generics.iter().all(|g| g.bounds.is_empty()) {
        return;
    }

    let only_in_bound =
        collect_type_params_only_in_bound(generics, params, return_type, found_in_body);
    if only_in_bound.is_empty() {
        return;
    }

    for generic in generics {
        if generic.name.starts_with('_') || !only_in_bound.contains(&generic.name) {
            continue;
        }
        local.add_type_param_only_in_bound(generic.name.to_string(), generic.span);
    }
}

fn collect_type_params_only_in_bound(
    generics: &[Generic],
    params: &[Binding],
    return_type: &Type,
    found_in_body: &HashSet<EcoString>,
) -> HashSet<EcoString> {
    let mut unseen_outside_bound_rhs: HashSet<EcoString> =
        generics.iter().map(|g| g.name.clone()).collect();
    for param in params {
        param
            .ty
            .remove_found_type_names(&mut unseen_outside_bound_rhs);
    }
    return_type.remove_found_type_names(&mut unseen_outside_bound_rhs);
    unseen_outside_bound_rhs.retain(|name| !found_in_body.contains(name));

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

/// Stops at nested function/lambda boundaries, which may shadow the name.
fn body_remove_found_type_names(expression: &Expression, names: &mut HashSet<EcoString>) {
    if names.is_empty() {
        return;
    }
    match expression {
        Expression::Function { .. } | Expression::Lambda { .. } => return,
        Expression::Let { binding, .. } => binding.ty.remove_found_type_names(names),
        Expression::Call {
            resolved_type_args, ..
        } => {
            for ty in resolved_type_args {
                ty.remove_found_type_names(names);
            }
        }
        Expression::Cast { ty, .. } => ty.remove_found_type_names(names),
        _ => {}
    }
    for child in expression.children() {
        body_remove_found_type_names(child, names);
    }
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
        Annotation::Unknown | Annotation::Opaque { .. } | Annotation::Constant { .. } => {}
    }
}
