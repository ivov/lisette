use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;

use semantics::facts::{DeferredChecks, GenericBoundOrigin};
use semantics::generics::bound_display_name;
use semantics::store::Store;
use syntax::types::{CompoundKind, TypeVarId};

pub(crate) fn run(store: &Store, checks: &DeferredChecks, sink: &LocalSink) {
    let mut reported_vars: HashSet<(String, TypeVarId)> = HashSet::default();
    let mut collected = Vec::new();
    let mut report_vars = |ty: &syntax::types::Type, package_id: &str| {
        collected.clear();
        ty.collect_unbound_variables(&mut collected);
        reported_vars.extend(collected.iter().map(|v| (package_id.to_string(), *v)));
    };
    for check in &checks.generic_calls {
        if check.ty.has_unbound_variables() {
            sink.push(diagnostics::infer::cannot_infer_type_argument(check.span));
            report_vars(&check.ty, &check.package_id);
        }
    }
    for obligation in &checks.generic_bounds {
        if obligation.argument.has_unbound_variables() {
            let required_name = bound_display_name(store, &obligation.required);
            let diagnostic = match &obligation.origin {
                GenericBoundOrigin::FunctionReference { name } => {
                    diagnostics::infer::cannot_infer_bounded_function_reference(
                        name,
                        &obligation.param_name,
                        &required_name,
                        obligation.span,
                    )
                }
                GenericBoundOrigin::Construction { name, .. } => {
                    diagnostics::infer::cannot_infer_struct_type_argument(
                        name,
                        &obligation.param_name,
                        &required_name,
                        obligation.span,
                    )
                }
            };
            sink.push(diagnostic);
            report_vars(&obligation.argument, &obligation.package_id);
        }
    }
    for check in &checks.empty_collections {
        if check.ty.has_unbound_variables() {
            sink.push(diagnostics::infer::uninferred_binding(
                &check.name,
                check.span,
            ));
            report_vars(&check.ty, &check.package_id);
        }
    }
    let mut reported_literal_spans = HashSet::default();
    for check in &checks.empty_literals {
        if !check.ty.has_unbound_variables() {
            continue;
        }
        let mut literal_vars = Vec::new();
        check.ty.collect_unbound_variables(&mut literal_vars);
        if literal_vars
            .iter()
            .any(|v| reported_vars.contains(&(check.package_id.clone(), *v)))
        {
            continue;
        }
        if reported_literal_spans.insert(check.span) {
            sink.push(diagnostics::infer::empty_slice_no_element_type(check.span));
        }
    }
    for check in &checks.slice_makes {
        let slice_ty = store.peel_alias(&check.ty);
        let Some((CompoundKind::Slice, args)) = slice_ty.as_compound() else {
            continue;
        };
        let Some(element_ty) = args.first() else {
            continue;
        };
        if element_ty.is_error() || element_ty.is_variable() {
            continue;
        }
        if let Err(no_zero) = semantics::zero::has_zero(store, element_ty, &check.package_id) {
            sink.push(diagnostics::infer::slice_make_no_zero(
                &no_zero.leaf_ty.stringify(),
                no_zero.hidden_go_state(),
                check.span,
            ));
        }
    }
    for check in &checks.statement_tails {
        if !check.expected_ty.is_unit()
            && !check.expected_ty.is_variable()
            && !check.expected_ty.is_ignored()
            && !check.expected_ty.is_error()
        {
            sink.push(diagnostics::infer::statement_as_tail(
                check.span,
                &check.expected_ty,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semantics::facts::{EmptyLiteralCheck, GenericCallCheck};
    use syntax::ast::Span;
    use syntax::types::{CompoundKind, Type};

    fn unbound_slice(id: u32) -> Type {
        Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![Type::Var {
                id: TypeVarId::new(id),
                hint: Some("T".into()),
            }],
        }
    }

    fn run_checks(call_package: &str, literal_package: &str) -> Vec<String> {
        let mut checks = DeferredChecks::default();
        checks.generic_calls.push(GenericCallCheck {
            ty: unbound_slice(5),
            span: Span::new(0, 0, 10),
            package_id: call_package.to_string(),
        });
        checks.empty_literals.push(EmptyLiteralCheck {
            ty: unbound_slice(5),
            span: Span::new(1, 4, 2),
            package_id: literal_package.to_string(),
        });
        let sink = LocalSink::new();
        run(&Store::new(), &checks, &sink);
        sink.into_diagnostics()
            .iter()
            .filter_map(|d| d.code_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn same_package_shared_var_suppresses_literal() {
        assert_eq!(run_checks("a", "a"), vec!["infer.missing_type_argument"]);
    }

    #[test]
    fn same_var_id_across_packages_does_not_suppress() {
        assert_eq!(
            run_checks("a", "b"),
            vec![
                "infer.missing_type_argument",
                "infer.empty_slice_no_element_type"
            ]
        );
    }
}
