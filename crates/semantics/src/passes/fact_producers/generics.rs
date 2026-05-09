//! Generic-parameter fact producers for the lint layer.
//!
//! Records facts on `Facts`; rendering happens later in `lints::from_facts`.

use ecow::EcoString;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Annotation, Binding, Expression, Generic};
use syntax::types::Type;

use crate::facts::Facts;

pub(crate) fn run(typed_ast: &[Expression], facts: &mut Facts) {
    for item in typed_ast {
        visit_expression(item, facts);
    }
}

fn visit_expression(expression: &Expression, facts: &mut Facts) {
    match expression {
        Expression::ImplBlock { methods, .. } => {
            for method in methods {
                visit_expression(method, facts);
            }
            return;
        }
        Expression::Function {
            generics,
            params,
            return_type,
            ..
        } => {
            check_unused_type_parameters(generics, params, return_type, facts);
            check_type_params_only_in_bound(generics, params, return_type, facts);
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, facts);
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
