use crate::Planner;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate};
use crate::patterns::decision_tree::{
    Check, PatternBinding, PatternInfo, SubjectRoot, render_condition,
};
use crate::plan::bodies::{LoweredStatement, assign, define, define_many};
use crate::plan::values::GoExpression;
use crate::state::bindings::InlineExpr;
use std::borrow::Cow;
use syntax::ast::Expression;

/// Hoist a root type assertion as `asserted := subject.(T)` for irrefutable
/// destructure paths (the pattern compiler has already verified the type).
pub(crate) fn apply_root_assertion<'s>(
    planner: &mut Planner,
    statements: &mut Vec<LoweredStatement>,
    info: &PatternInfo,
    subject: &'s str,
) -> Cow<'s, str> {
    let Some(assertion) = info.root_assertion.as_ref() else {
        return Cow::Borrowed(subject);
    };
    if !info.requires_asserted_subject() {
        return Cow::Borrowed(subject);
    }
    let [go_type] = assertion.go_types.as_slice() else {
        unreachable!("multi-type root assertions only reach match destructure paths")
    };
    planner.scope.record_go_use(subject);
    let expression =
        GoExpression::type_assertion(GoExpression::name(subject.to_string()), go_type.clone());
    let var = planner.hoist_tmp_value_statement(statements, "asserted", expression);
    Cow::Owned(var)
}

/// Hoist a root type assertion as comma-ok for refutable contexts (while-let,
/// select arms, or-pattern let-else). Returns `(effective_subject, ok_test)`.
pub(crate) fn apply_refutable_root_assertion<'s>(
    planner: &mut Planner,
    statements: &mut Vec<LoweredStatement>,
    info: &PatternInfo,
    subject: &'s str,
) -> (Cow<'s, str>, Option<GoExpression>) {
    let Some(assertion) = info.root_assertion.as_ref() else {
        return (Cow::Borrowed(subject), None);
    };
    planner.scope.record_go_use(subject);
    let needs_asserted = info.requires_asserted_subject();
    let assertion_of = |go_type: &String| {
        GoExpression::type_assertion(GoExpression::name(subject.to_string()), go_type.clone())
    };
    match assertion.go_types.as_slice() {
        [go_type] => {
            let asserted_lhs = if needs_asserted {
                let v = planner.fresh_var(Some("asserted"));
                planner.declare(&v);
                v
            } else {
                "_".to_string()
            };
            let ok = planner.fresh_var(Some("ok"));
            planner.declare(&ok);
            statements.push(define_many(
                vec![asserted_lhs.clone(), ok.clone()],
                assertion_of(go_type),
            ));
            let effective = if needs_asserted {
                Cow::Owned(asserted_lhs)
            } else {
                Cow::Borrowed(subject)
            };
            (effective, Some(GoExpression::name(ok)))
        }
        multiple => {
            // No-binding interface or-pattern (`A | B`): no single asserted
            // form is possible across types.
            let oks = multiple
                .iter()
                .map(|t| {
                    let ok = planner.fresh_var(Some("ok"));
                    planner.declare(&ok);
                    statements.push(define_many(
                        vec!["_".to_string(), ok.clone()],
                        assertion_of(t),
                    ));
                    GoExpression::name(ok)
                })
                .reduce(|left, right| GoExpression::binary(left, "||", right))
                .expect("a multi-type assertion names at least one type");
            (
                Cow::Borrowed(subject),
                Some(GoExpression::parenthesized(oks)),
            )
        }
    }
}

/// Combine an optional `ok` test with the rendered checks into a guard
/// condition; `true` when both are absent.
pub(crate) fn compose_refutable_condition(
    ok_test: Option<&GoExpression>,
    checks: &[Check],
    effective_subject: &str,
) -> GoExpression {
    let condition = render_condition(checks, SubjectRoot::Var(effective_subject));
    match ok_test {
        None => condition,
        Some(ok) if checks.is_empty() => ok.clone(),
        Some(ok) => GoExpression::binary(ok.clone(), "&&", condition),
    }
}

/// Push one `name := subject.path` per binding. Inlined bindings produce no statement.
pub(crate) fn tree_binding_statements(
    planner: &mut Planner,
    statements: &mut Vec<LoweredStatement>,
    bindings: &[PatternBinding],
    subject_var: &str,
    consumers: &[&Expression],
) {
    for binding in bindings {
        let Some(ref go_name) = binding.go_name else {
            planner.scope.bind(&binding.lisette_name, "");
            continue;
        };

        let access_expression = binding.path.render(SubjectRoot::Var(subject_var));

        if analyze_inline_candidate(&binding.lisette_name, consumers) == InlineDecision::Inline {
            let composable = binding
                .path
                .render_composable(SubjectRoot::Var(subject_var));
            planner.scope.bind_inline_expr(
                &binding.lisette_name,
                InlineExpr::new(
                    composable,
                    vec![subject_var.to_string()],
                    binding.path.contains_deferred_evaluation(),
                ),
            );
            continue;
        }

        planner.scope.record_go_use(subject_var);
        let name = if planner.scope.has_binding_for_go_name(go_name) {
            let fresh = planner.fresh_var(Some(&binding.lisette_name));
            planner.scope.bind(&binding.lisette_name, &fresh);
            planner.try_declare(&fresh);
            fresh
        } else {
            let name = planner.scope.bind(&binding.lisette_name, go_name.clone());
            if !planner.package.is_package_block_name(&name) && planner.try_declare(&name) {
                name
            } else {
                let fresh = planner.fresh_var(Some(&binding.lisette_name));
                planner.scope.bind(&binding.lisette_name, &fresh);
                planner.try_declare(&fresh);
                fresh
            }
        };
        statements.push(define(name, access_expression));
    }
}

pub(crate) fn with_tree_bindings<R>(
    planner: &mut Planner,
    statements: &mut Vec<LoweredStatement>,
    bindings: &[PatternBinding],
    subject_var: &str,
    body: &Expression,
    f: impl FnOnce(&mut Planner, &mut Vec<LoweredStatement>) -> R,
) -> R {
    planner.with_binding_frame(|planner| {
        tree_binding_statements(planner, statements, bindings, subject_var, &[body]);
        f(planner, statements)
    })
}

/// Push `name = subject.path` leaves for or-pattern alternatives whose
/// bindings were pre-declared by `emit_binding_declarations_with_type`.
pub(crate) fn tree_assignment_statements(
    planner: &mut Planner,
    statements: &mut Vec<LoweredStatement>,
    bindings: &[PatternBinding],
    subject_var: &str,
) {
    for binding in bindings {
        if binding.go_name.is_none() {
            continue;
        }

        let Some(registered_name) = planner.scope.resolve_binding_go_name(&binding.lisette_name)
        else {
            continue;
        };
        let name = registered_name.to_string();
        planner.scope.record_go_use(subject_var);
        let access_expression = binding.path.render(SubjectRoot::Var(subject_var));
        statements.push(assign(GoExpression::name(name), access_expression));
    }
}
