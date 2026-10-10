use crate::OuterBindings;
use crate::Planner;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate_ids};
use crate::patterns::decision_tree::{
    Check, PatternBinding, PatternInfo, SubjectRoot, lift_root_assertion, render_condition,
};
use crate::plan::bodies::{Definition, LoweredStatement, Statement, assign, define_many};
use crate::plan::go_expression::BinaryOp;
use crate::plan::local::GoIdentifier;
use crate::plan::values::{GoExpression, Stability};
use crate::state::bindings::InlineExpr;
use syntax::ast::Expression;

pub(crate) struct AssertedPattern {
    pub subject: GoExpression,
    pub ok_test: Option<GoExpression>,
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
}

impl AssertedPattern {
    pub(crate) fn is_irrefutable(&self) -> bool {
        self.ok_test.is_none() && self.checks.is_empty()
    }

    pub(crate) fn condition(&self) -> GoExpression {
        let condition = render_condition(&self.checks, SubjectRoot::Var(&self.subject));
        match &self.ok_test {
            None => condition,
            Some(ok) if self.checks.is_empty() => ok.clone(),
            Some(ok) => GoExpression::binary(ok.clone(), BinaryOp::And, condition),
        }
    }
}

/// True when a downstream consumer will reference the asserted value.
fn requires_asserted_subject(checks: &[Check], bindings: &[PatternBinding]) -> bool {
    !checks.is_empty() || bindings.iter().any(|b| b.target.is_named())
}

/// Hoist a root type assertion as `asserted := subject.(T)` for irrefutable
/// destructure paths (the pattern compiler has already verified the type).
pub(crate) fn apply_root_assertion(
    planner: &mut Planner,
    statements: &mut Vec<Statement>,
    info: PatternInfo,
    subject: &GoExpression,
) -> AssertedPattern {
    let PatternInfo {
        mut checks,
        mut bindings,
        packages,
        ..
    } = info;
    planner.require_packages(&packages);
    let subject = match lift_root_assertion(&mut checks, &mut bindings) {
        Some(go_types) if requires_asserted_subject(&checks, &bindings) => {
            let [go_type] = go_types.as_slice() else {
                unreachable!("multi-type root assertions only reach match destructure paths")
            };
            let expression = GoExpression::type_assertion(subject.clone(), go_type.clone());
            let var = planner.hoist_tmp_value_statement(statements, "asserted", expression);
            GoExpression::name(var)
        }
        _ => subject.clone(),
    };
    AssertedPattern {
        subject,
        ok_test: None,
        checks,
        bindings,
    }
}

/// Hoist a root type assertion as comma-ok for refutable contexts (while-let,
pub(crate) fn apply_refutable_root_assertion(
    planner: &mut Planner,
    statements: &mut Vec<Statement>,
    info: PatternInfo,
    subject: &GoExpression,
) -> AssertedPattern {
    let PatternInfo {
        mut checks,
        mut bindings,
        packages,
        ..
    } = info;
    planner.require_packages(&packages);
    let Some(go_types) = lift_root_assertion(&mut checks, &mut bindings) else {
        return AssertedPattern {
            subject: subject.clone(),
            ok_test: None,
            checks,
            bindings,
        };
    };
    let needs_asserted = requires_asserted_subject(&checks, &bindings);
    let assertion_of =
        |go_type: &String| GoExpression::type_assertion(subject.clone(), go_type.clone());
    let (subject, ok_test) = match go_types.as_slice() {
        [go_type] => {
            let asserted_lhs = if needs_asserted {
                let v = planner.fresh_var(Some("asserted"));
                planner.declare(&v);
                planner.scope.generated_identifier(&v)
            } else {
                GoIdentifier::name("_".to_string())
            };
            let ok = planner.fresh_var(Some("ok"));
            planner.declare(&ok);
            let ok = planner.scope.generated_identifier(&ok);
            statements.push(define_many(
                vec![asserted_lhs.clone(), ok.clone()],
                assertion_of(go_type),
            ));
            let effective = if needs_asserted {
                GoExpression::identifier(asserted_lhs)
            } else {
                subject.clone()
            };
            (effective, Some(GoExpression::identifier(ok)))
        }
        multiple => {
            // No-binding interface or-pattern (`A | B`): no single asserted
            // form is possible across types.
            let oks = multiple
                .iter()
                .map(|t| {
                    let ok = planner.fresh_var(Some("ok"));
                    planner.declare(&ok);
                    let ok = planner.scope.generated_identifier(&ok);
                    statements.push(define_many(
                        vec![GoIdentifier::name("_".to_string()), ok.clone()],
                        assertion_of(t),
                    ));
                    GoExpression::identifier(ok)
                })
                .reduce(|left, right| GoExpression::binary(left, BinaryOp::Or, right))
                .expect("a multi-type assertion names at least one type");
            (subject.clone(), Some(oks))
        }
    };
    AssertedPattern {
        subject,
        ok_test,
        checks,
        bindings,
    }
}

/// Push one `name := subject.path` per binding. Inlined bindings produce no statement.
pub(crate) fn tree_binding_statements(
    planner: &mut Planner,
    statements: &mut Vec<Statement>,
    bindings: &[PatternBinding],
    subject: SubjectRoot<'_>,
    consumers: &[&Expression],
) {
    for binding in bindings {
        let Some(go_name) = binding.target.go_name() else {
            bind_inline_unit(planner, binding);
            continue;
        };

        let access_expression = binding.path.render(subject);

        if analyze_inline_candidate_ids(&binding.binding_ids, consumers) == InlineDecision::Inline {
            let composable = binding.path.render(subject);
            let stability = planner.path_read_stability(&composable);
            planner.scope.bind_inline_expr(
                &binding.lisette_name,
                &binding.binding_ids,
                InlineExpr::new(composable, stability),
            );
            continue;
        }
        let name = planner.claim_block_binding(
            &binding.lisette_name,
            &binding.binding_ids,
            go_name,
            OuterBindings::StayVisible,
        );
        statements
            .push(LoweredStatement::Define(Definition::single(name, access_expression)).into());
    }
}

pub(crate) fn bind_inline_unit(planner: &mut Planner, binding: &PatternBinding) {
    let unit = GoExpression::empty_composite("struct{}".to_string());
    planner.scope.bind_inline_expr(
        &binding.lisette_name,
        &binding.binding_ids,
        InlineExpr::new(unit, Stability::Fixed),
    );
}

pub(crate) fn with_tree_bindings<R>(
    planner: &mut Planner,
    statements: &mut Vec<Statement>,
    bindings: &[PatternBinding],
    subject: &GoExpression,
    body: &Expression,
    f: impl FnOnce(&mut Planner, &mut Vec<Statement>) -> R,
) -> R {
    planner.with_binding_frame(|planner| {
        tree_binding_statements(
            planner,
            statements,
            bindings,
            SubjectRoot::Var(subject),
            &[body],
        );
        f(planner, statements)
    })
}

/// Push `name = subject.path` leaves for or-pattern alternatives.
pub(crate) fn tree_assignment_statements(
    planner: &mut Planner,
    statements: &mut Vec<Statement>,
    bindings: &[PatternBinding],
    subject: &GoExpression,
) {
    for binding in bindings {
        if !binding.target.is_named() {
            continue;
        }

        let Some(registered_name) = planner.scope.resolve_binding_go_name(&binding.lisette_name)
        else {
            continue;
        };
        let name = registered_name.to_string();
        let access_expression = binding.path.render(SubjectRoot::Var(subject));
        statements.push(assign(GoExpression::name(name), access_expression));
    }
}
