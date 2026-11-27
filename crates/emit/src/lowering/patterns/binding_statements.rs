use crate::Planner;
use crate::lookup::facts::inline_uses::is_inline_candidate;
use crate::lookup::state::bindings::{BindingValue, InlineExpression};
use crate::lookup::state::scope::ScopeState;
use crate::lowering::patterns::decision_tree::{
    Check, PatternBinding, PatternInfo, SubjectRoot, lift_root_assertion, render_condition,
};
use crate::lowering::patterns::tree_planner::MatchSubject;
use crate::target::ir::bodies::{Definition, LoweredStatement, Statement, assign, define_many};
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{GoExpression, Stability};
use crate::target::types::go_type::GoType;
use syntax::ast::Expression;

pub(in crate::lowering) struct AssertedPattern {
    subject: MatchSubject,
    pub ok_test: Option<GoExpression>,
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
}

impl AssertedPattern {
    pub(in crate::lowering) fn root(&self) -> SubjectRoot<'_> {
        self.subject.root()
    }

    pub(in crate::lowering) fn is_irrefutable(&self) -> bool {
        self.ok_test.is_none() && self.checks.is_empty()
    }

    pub(in crate::lowering) fn condition(&self, scope: &mut ScopeState) -> GoExpression {
        let condition = render_condition(scope, &self.checks, self.root());
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
            bindings,
            ..
        } = info;
        let narrowed = match lift_root_assertion(&mut checks) {
            Some(go_types) if requires_asserted_subject(&checks, &bindings) => {
                let [go_type] = go_types.as_slice() else {
                    unreachable!("multi-type root assertions only reach match destructure paths")
                };
                let expression = GoExpression::type_assertion(subject.clone(), go_type.clone());
                let var = self.hoist_temp(statements, "asserted", expression);
                MatchSubject::Narrowed {
                    subject: subject.clone(),
                    go_type: go_type.clone(),
                    value: GoExpression::identifier(var),
                }
            }
            _ => MatchSubject::Var(subject.clone()),
        };

        AssertedPattern {
            subject: narrowed,
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
            bindings,
            ..
        } = info;
        let Some(go_types) = lift_root_assertion(&mut checks) else {
            return AssertedPattern {
                subject: MatchSubject::Var(subject.clone()),
                ok_test: None,
                checks,
                bindings,
            };
        };

        let needs_asserted = requires_asserted_subject(&checks, &bindings);
        let assertion_of =
            |go_type: &GoType| GoExpression::type_assertion(subject.clone(), go_type.clone());
        let (narrowed, ok_test) = match go_types.as_slice() {
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
                let narrowed = needs_asserted.then(|| MatchSubject::Narrowed {
                    subject: subject.clone(),
                    go_type: go_type.clone(),
                    value: GoExpression::identifier(asserted_lhs),
                });
                (narrowed, Some(GoExpression::identifier(ok)))
            }
            multiple => (
                None,
                Some(self.assert_any_root_type(statements, subject, multiple)),
            ),
        };

        AssertedPattern {
            subject: narrowed.unwrap_or_else(|| MatchSubject::Var(subject.clone())),
            ok_test,
            checks,
            bindings,
        }
    }

    fn assert_any_root_type(
        &mut self,
        statements: &mut Vec<Statement>,
        subject: &GoExpression,
        go_types: &[GoType],
    ) -> GoExpression {
        go_types
            .iter()
            .map(|go_type| {
                let ok = self.fresh_declared("ok");
                statements.push(define_many(
                    vec![GoIdentifier::blank(), ok.clone()],
                    GoExpression::type_assertion(subject.clone(), go_type.clone()),
                ));
                GoExpression::identifier(ok)
            })
            .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::Or, right))
            .expect("a multi-type assertion names at least one type")
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
                && is_inline_candidate(&binding.binding_ids, consumers)
            {
                let composable = binding.path.render(subject);
                let stability = self.queries().path_read_stability(&composable);
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
            let name = self.claim_binding(&binding.lisette_name, &binding.binding_ids, go_name);

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
        subject: SubjectRoot<'_>,
        body: &Expression,
        f: impl FnOnce(&mut Planner, &mut Vec<Statement>) -> R,
    ) -> R {
        self.with_binding_frame(|planner| {
            planner.tree_binding_statements(statements, bindings, subject, &[body]);
            f(planner, statements)
        })
    }

    pub(in crate::lowering) fn tree_assignment_statements(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        subject: SubjectRoot<'_>,
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

            let access_expression = binding.path.render(subject);
            let (setup, access_expression) =
                binding.coercion.clone().lower(self, access_expression);
            statements.extend(setup);
            statements.push(assign(GoExpression::identifier(name), access_expression));
        }
    }
}
