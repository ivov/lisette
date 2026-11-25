use crate::Planner;
use crate::lookup::facts::inline_uses::{InlineDecision, analyze_inline_candidate_ids};
use crate::lookup::state::bindings::{BindingValue, InlineExpression};
use crate::lookup::state::scope::ScopeState;
use crate::lowering::patterns::decision_tree::{
    Check, PatternBinding, PatternInfo, SubjectRoot, lift_root_assertion, render_condition,
};
use crate::lowering::planner::OuterBindings;
use crate::target::ir::bodies::{Definition, LoweredStatement, Statement, assign, define_many};
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{GoExpression, Stability};
use crate::target::types::go_type::GoType;
use syntax::ast::Expression;

pub(in crate::lowering) struct AssertedPattern {
    pub subject: GoExpression,
    pub ok_test: Option<GoExpression>,
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
}

impl AssertedPattern {
    pub(in crate::lowering) fn is_irrefutable(&self) -> bool {
        self.ok_test.is_none() && self.checks.is_empty()
    }

    pub(in crate::lowering) fn condition(&self, scope: &mut ScopeState) -> GoExpression {
        let condition = render_condition(scope, &self.checks, SubjectRoot::Var(&self.subject));
        match &self.ok_test {
            None => condition,
            Some(ok) if self.checks.is_empty() => ok.clone(),
            Some(ok) => GoExpression::binary(ok.clone(), GoBinaryOperator::And, condition),
        }
    }
}

fn requires_asserted_subject(checks: &[Check], bindings: &[PatternBinding]) -> bool {
    !checks.is_empty() || bindings.iter().any(|b| b.target.is_named())
}

impl Planner<'_> {
    pub(in crate::lowering) fn apply_root_assertion(
        &mut self,
        statements: &mut Vec<Statement>,
        info: PatternInfo,
        subject: &GoExpression,
    ) -> AssertedPattern {
        let PatternInfo {
            mut checks,
            mut bindings,
            ..
        } = info;
        let subject = match lift_root_assertion(&mut checks, &mut bindings) {
            Some(go_types) if requires_asserted_subject(&checks, &bindings) => {
                let [go_type] = go_types.as_slice() else {
                    unreachable!("multi-type root assertions only reach match destructure paths")
                };
                let expression = GoExpression::type_assertion(subject.clone(), go_type.clone());
                let var = self.hoist_temp(statements, "asserted", expression);
                GoExpression::identifier(var)
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

    pub(in crate::lowering) fn apply_refutable_root_assertion(
        &mut self,
        statements: &mut Vec<Statement>,
        info: PatternInfo,
        subject: &GoExpression,
    ) -> AssertedPattern {
        let PatternInfo {
            mut checks,
            mut bindings,
            ..
        } = info;
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
            |go_type: &GoType| GoExpression::type_assertion(subject.clone(), go_type.clone());
        let (subject, ok_test) = match go_types.as_slice() {
            [go_type] => {
                let asserted_lhs = if needs_asserted {
                    self.fresh_declared("asserted")
                } else {
                    GoIdentifier::blank()
                };
                let ok = self.fresh_declared("ok");
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
                let oks = multiple
                    .iter()
                    .map(|t| {
                        let ok = self.fresh_declared("ok");
                        statements.push(define_many(
                            vec![GoIdentifier::blank(), ok.clone()],
                            assertion_of(t),
                        ));
                        GoExpression::identifier(ok)
                    })
                    .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::Or, right))
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

    pub(in crate::lowering) fn tree_binding_statements(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        subject: SubjectRoot<'_>,
        consumers: &[&Expression],
    ) {
        for binding in bindings {
            let Some(go_name) = binding.target.go_name() else {
                self.bind_inline_unit(binding);
                continue;
            };

            let access_expression = binding.path.render(subject);

            if binding.coercion.is_identity()
                && analyze_inline_candidate_ids(&binding.binding_ids, consumers)
                    == InlineDecision::Inline
            {
                let composable = binding.path.render(subject);
                let stability = self.path_read_stability(&composable);
                self.scope.bind_inline_expression(
                    &binding.lisette_name,
                    &binding.binding_ids,
                    InlineExpression::new(composable, stability),
                );
                continue;
            }
            let (setup, access_expression) =
                binding.coercion.clone().lower(self, access_expression);
            statements.extend(setup);
            let name = self.claim_block_binding(
                &binding.lisette_name,
                &binding.binding_ids,
                go_name,
                OuterBindings::StayVisible,
            );

            statements
                .push(LoweredStatement::Define(Definition::single(name, access_expression)).into());
        }
    }

    pub(in crate::lowering) fn bind_inline_unit(&mut self, binding: &PatternBinding) {
        let unit = self.unit_value(None, None);
        self.scope.bind_inline_expression(
            &binding.lisette_name,
            &binding.binding_ids,
            InlineExpression::new(unit, Stability::Fixed),
        );
    }

    pub(in crate::lowering) fn with_tree_bindings<R>(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        subject: &GoExpression,
        body: &Expression,
        f: impl FnOnce(&mut Planner, &mut Vec<Statement>) -> R,
    ) -> R {
        self.with_binding_frame(|planner| {
            planner.tree_binding_statements(
                statements,
                bindings,
                SubjectRoot::Var(subject),
                &[body],
            );
            f(planner, statements)
        })
    }

    pub(in crate::lowering) fn tree_assignment_statements(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        subject: &GoExpression,
    ) {
        for binding in bindings {
            if !binding.target.is_named() {
                continue;
            }

            let name = binding
                .binding_ids
                .iter()
                .find_map(|id| self.scope.resolve_binding_id(*id))
                .and_then(BindingValue::as_go_identifier)
                .expect("a named alternative binding is registered")
                .clone();

            let access_expression = binding.path.render(SubjectRoot::Var(subject));
            let (setup, access_expression) =
                binding.coercion.clone().lower(self, access_expression);
            statements.extend(setup);
            statements.push(assign(GoExpression::identifier(name), access_expression));
        }
    }
}
