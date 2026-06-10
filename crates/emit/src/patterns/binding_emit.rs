use crate::Planner;
use crate::Renderer;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate};
use crate::patterns::decision_tree::{Check, PatternBinding, PatternInfo, render_condition};
use crate::plan::bodies::{LoweredBlock, LoweredStatement};
use crate::state::bindings::{BindingValue, InlineExpr};
use crate::write_line;

/// Hoist a root type assertion as `asserted := subject.(T)` for irrefutable
/// destructure paths (the pattern compiler has already verified the type).
pub(crate) fn apply_root_assertion<'s>(
    planner: &mut Planner,
    output: &mut String,
    info: &PatternInfo,
    subject: &'s str,
) -> std::borrow::Cow<'s, str> {
    let Some(assertion) = info.root_assertion.as_ref() else {
        return std::borrow::Cow::Borrowed(subject);
    };
    if !info.requires_asserted_subject() {
        return std::borrow::Cow::Borrowed(subject);
    }
    let [go_type] = assertion.go_types.as_slice() else {
        unreachable!("multi-type root assertions only reach match destructure paths")
    };
    planner.scope.record_go_use(subject);
    let expression = format!("{}.({})", subject, go_type);
    let var = planner.hoist_tmp_value(output, "asserted", &expression);
    std::borrow::Cow::Owned(var)
}

/// Hoist a root type assertion as comma-ok for refutable contexts (while-let,
/// select arms, or-pattern let-else). Returns `(effective_subject, ok_var)`.
pub(crate) fn apply_refutable_root_assertion<'s>(
    planner: &mut Planner,
    output: &mut String,
    info: &PatternInfo,
    subject: &'s str,
) -> (std::borrow::Cow<'s, str>, Option<String>) {
    let Some(assertion) = info.root_assertion.as_ref() else {
        return (std::borrow::Cow::Borrowed(subject), None);
    };
    planner.scope.record_go_use(subject);
    let needs_asserted = info.requires_asserted_subject();
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
            write_line!(
                output,
                "{}, {} := {}.({})",
                asserted_lhs,
                ok,
                subject,
                go_type
            );
            let effective = if needs_asserted {
                std::borrow::Cow::Owned(asserted_lhs)
            } else {
                std::borrow::Cow::Borrowed(subject)
            };
            (effective, Some(ok))
        }
        multiple => {
            // No-binding interface or-pattern (`A | B`): no single asserted
            // form is possible across types.
            let oks: Vec<String> = multiple
                .iter()
                .map(|t| {
                    let ok = planner.fresh_var(Some("ok"));
                    planner.declare(&ok);
                    write_line!(output, "_, {} := {}.({})", ok, subject, t);
                    ok
                })
                .collect();
            (
                std::borrow::Cow::Borrowed(subject),
                Some(format!("({})", oks.join(" || "))),
            )
        }
    }
}

/// Combine an optional `ok` variable with rendered checks into a guard
/// condition; returns `"true"` when both are absent.
pub(crate) fn compose_refutable_condition(
    ok_var: Option<&str>,
    checks: &[Check],
    effective_subject: &str,
) -> String {
    let condition = render_condition(checks, effective_subject);
    match ok_var {
        None => condition,
        Some(ok) if condition == "true" => ok.to_string(),
        Some(ok) => format!("{} && {}", ok, condition),
    }
}

pub(crate) fn emit_tree_bindings_with_consumers(
    planner: &mut Planner,
    output: &mut String,
    bindings: &[PatternBinding],
    subject_var: &str,
    consumers: &[&syntax::ast::Expression],
) -> Vec<(String, Option<BindingValue>)> {
    let mut statements = Vec::new();
    let installed_inlines =
        tree_binding_statements(planner, &mut statements, bindings, subject_var, consumers);
    let block = LoweredBlock { statements };
    Renderer.render_lowered_block(output, &block);
    installed_inlines
}

/// Push one `name := subject.path` per binding. Inlined bindings produce no
/// statement; their overlay pairs are returned for `drop_inline_overlays`.
pub(crate) fn tree_binding_statements(
    planner: &mut Planner,
    statements: &mut Vec<LoweredStatement>,
    bindings: &[PatternBinding],
    subject_var: &str,
    consumers: &[&syntax::ast::Expression],
) -> Vec<(String, Option<BindingValue>)> {
    let mut installed_inlines = Vec::new();
    for binding in bindings {
        let Some(ref go_name) = binding.go_name else {
            planner.scope.bind(&binding.lisette_name, "");
            continue;
        };

        let access_expression = binding.path.render(subject_var);

        if !consumers.is_empty()
            && analyze_inline_candidate(&binding.lisette_name, consumers) == InlineDecision::Inline
        {
            let previous = planner
                .scope
                .resolve_identifier_binding(&binding.lisette_name)
                .cloned();
            let safe_text = binding.path.render_composable(subject_var);
            planner.scope.bind_inline_expr(
                &binding.lisette_name,
                InlineExpr::with_refs(safe_text, vec![subject_var.to_string()]),
            );
            installed_inlines.push((binding.lisette_name.clone(), previous));
            continue;
        }

        planner.scope.record_go_use(subject_var);
        let line = if planner.scope.has_binding_for_go_name(go_name) {
            let fresh = planner.fresh_var(Some(&binding.lisette_name));
            planner.scope.bind(&binding.lisette_name, &fresh);
            planner.try_declare(&fresh);
            format!("{} := {}\n", fresh, access_expression)
        } else {
            let name = planner.scope.bind(&binding.lisette_name, go_name.clone());
            if planner.try_declare(&name) {
                format!("{} := {}\n", name, access_expression)
            } else {
                let fresh = planner.fresh_var(Some(&binding.lisette_name));
                planner.scope.bind(&binding.lisette_name, &fresh);
                planner.try_declare(&fresh);
                format!("{} := {}\n", fresh, access_expression)
            }
        };
        statements.push(LoweredStatement::RawGo(line));
    }
    installed_inlines
}

pub(crate) fn drop_inline_overlays(
    planner: &mut Planner,
    installed: &[(String, Option<BindingValue>)],
) {
    for (name, previous) in installed {
        match previous {
            Some(BindingValue::GoName(go)) => {
                planner.scope.bind(name.as_str(), go.as_str());
            }
            Some(BindingValue::InlineExpr(expr)) => {
                planner.scope.bind_inline_expr(name.as_str(), expr.clone());
            }
            None => {
                planner.scope.remove_binding(name);
            }
        }
    }
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
        let access_expression = binding.path.render(subject_var);
        statements.push(LoweredStatement::RawGo(format!(
            "{} = {}\n",
            name, access_expression
        )));
    }
}
