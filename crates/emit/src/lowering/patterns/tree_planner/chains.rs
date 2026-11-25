use super::{TreePlanner, WalkCtx, merge_binding_ids};
use crate::target::ir::go_expression::Position;

use syntax::ast::Expression;

use crate::lowering::control_flow::branches::Divergence;
use crate::lowering::patterns::binding_declarations::is_unconditional_catchall;
use crate::lowering::patterns::decision_tree::{
    ArmLeaf, ChainArm, ChainTail, ChainTest, render_condition,
};
use crate::lowering::statements::placement::unreachable_panic_if_needed;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, direct_first, directed_first,
};
use crate::target::ir::values::GoExpression;

impl<'a, 'e> TreePlanner<'a, 'e> {
    pub(super) fn render_chain_root(
        &mut self,
        statements: &mut Vec<Statement>,
        tests: &[ChainTest],
        tail: &ChainTail,
        place: &PlacePlan,
    ) {
        let chain_tail_is_exhaustive = !matches!(tail, ChainTail::Open)
            || self
                .arms
                .last()
                .is_some_and(|arm| !arm.has_guard() && is_unconditional_catchall(&arm.pattern));
        self.lower_chain_branch(statements, tests, tail, place);
        if let Some(panic) = unreachable_panic_if_needed(place, chain_tail_is_exhaustive) {
            statements.push(panic);
        }
    }

    pub(super) fn lower_chain_branch(
        &mut self,
        statements: &mut Vec<Statement>,
        tests: &[ChainTest],
        tail: &ChainTail,
        place: &PlacePlan,
    ) {
        let conditions = self.render_chain_conditions(tests);
        let regular_len = match tail {
            ChainTail::LastTest => tests.len() - 1,
            ChainTail::Catchall(_) | ChainTail::Open => tests.len(),
        };

        let guard_ctx = WalkCtx::switch_case(place);
        let chain_ctx = WalkCtx::chain_test(place);

        let mut branches: Vec<ChainBranch> = Vec::with_capacity(regular_len);
        let mut all_diverge = regular_len > 0;
        for (test, condition) in tests[..regular_len].iter().zip(&conditions) {
            let condition = condition
                .clone()
                .unwrap_or_else(|| GoExpression::literal("true".to_string()));
            let body = self.with_scope(|this| {
                let mut body: Vec<Statement> = Vec::new();
                match &test.arm {
                    ChainArm::Leaf(leaf) => this.walk_leaf(&mut body, leaf, &chain_ctx),
                    ChainArm::Guard(guard) => this.walk_guard(&mut body, guard, &guard_ctx),
                }
                body
            });

            let body = LoweredBlock { statements: body };
            all_diverge &= body.ends_with_diverge();
            let line = self.arm_line(test.arm.leaf().arm_index);
            branches.push(ChainBranch {
                condition,
                body,
                line,
            });
        }

        let all_divergence = if all_diverge {
            Divergence::Diverges
        } else {
            Divergence::FallsThrough
        };
        let trailing = match tail {
            ChainTail::LastTest => match &tests[regular_len].arm {
                ChainArm::Leaf(leaf) => {
                    self.lower_leaf_else_or_flat(leaf, &chain_ctx, all_divergence)
                }
                ChainArm::Guard(guard) => self.lower_else_body(
                    all_divergence,
                    Some(guard.leaf.arm_index),
                    |this, body| this.walk_guard(body, guard, &chain_ctx),
                ),
            },
            ChainTail::Catchall(catchall) => {
                self.lower_leaf_else_or_flat(catchall, &chain_ctx, all_divergence)
            }
            ChainTail::Open => ElseArm::None,
        };

        if branches.is_empty() {
            match trailing {
                ElseArm::Else { body, .. } => statements.extend(body.statements),
                ElseArm::ElseIf(plan) => statements.push(LoweredStatement::If(*plan).into()),
                ElseArm::None => {}
            }
            return;
        }
        statements.push(LoweredStatement::If(build_chain_plan(branches, trailing)).into());
    }

    pub(super) fn lower_chain_grouped(
        &mut self,
        statements: &mut Vec<Statement>,
        tests: &[ChainTest],
        tail: &ChainTail,
        ctx: &WalkCtx,
    ) {
        let conditions = self.render_chain_conditions(tests);
        let inner_ctx = ctx.nested();
        let groups = group_chain_tests_by_condition(&conditions);
        let group_count = groups.len();

        for (g, (_condition, indices)) in groups.iter().enumerate() {
            let start = statements.len();
            let is_last_group = g == group_count - 1;
            let collapse_as_catchall = is_last_group && matches!(tail, ChainTail::LastTest);
            self.lower_chain_group(
                statements,
                ChainGroup {
                    indices,
                    tests,
                    conditions: &conditions,
                },
                &inner_ctx,
                collapse_as_catchall,
            );
            if g > 0 {
                let line = self.arm_line(tests[indices[0]].arm.leaf().arm_index);
                direct_first(&mut statements[start..], line);
            }
        }

        if let ChainTail::Catchall(catchall) = tail {
            self.walk_leaf(statements, catchall, ctx);
        }
    }

    fn lower_chain_group(
        &mut self,
        statements: &mut Vec<Statement>,
        group: ChainGroup<'_>,
        ctx: &WalkCtx,
        collapse_as_catchall: bool,
    ) {
        let ChainGroup {
            indices,
            tests,
            conditions,
        } = group;
        if collapse_as_catchall {
            self.lower_chain_group_tests(statements, indices, tests, ctx);
            return;
        }

        let body = self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            this.lower_chain_group_tests(&mut body, indices, tests, ctx);
            body
        });

        let body = LoweredBlock { statements: body };
        let first_condition = &conditions[indices[0]];
        match first_condition {
            Some(condition) => statements.push(
                LoweredStatement::If(IfPlan::for_match(condition.clone(), body, ElseArm::None))
                    .into(),
            ),
            None => statements.push(LoweredStatement::Block(body).into()),
        }
    }

    fn lower_chain_group_tests(
        &mut self,
        statements: &mut Vec<Statement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        if bindings_are_hoistable(tests, indices) {
            self.lower_chain_group_hoisted(statements, indices, tests, ctx);
        } else {
            self.lower_chain_group_per_test(statements, indices, tests, ctx);
        }
    }

    fn lower_chain_group_hoisted(
        &mut self,
        statements: &mut Vec<Statement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        if let Some(&ref_index) = indices
            .iter()
            .find(|&&index| !tests[index].arm.leaf().bindings.is_empty())
        {
            let mut bindings = tests[ref_index].arm.leaf().bindings.clone();
            for &index in indices {
                merge_binding_ids(&mut bindings, &tests[index].arm.leaf().bindings);
            }
            let mut consumers: Vec<&Expression> = Vec::new();
            for &index in indices {
                let arm = &self.arms[tests[index].arm.leaf().arm_index];
                if let Some(guard) = arm.guard.as_deref() {
                    consumers.push(guard);
                }
                consumers.push(&arm.expression);
            }
            self.with_bindings(statements, &bindings, &consumers, |this, statements| {
                this.lower_chain_group_bodies(statements, indices, tests, ctx);
            });
        } else {
            self.lower_chain_group_bodies(statements, indices, tests, ctx);
        }
    }

    fn lower_chain_group_bodies(
        &mut self,
        statements: &mut Vec<Statement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        for &test_index in indices {
            match &tests[test_index].arm {
                ChainArm::Leaf(leaf) => self.lower_arm_leaf(statements, leaf.arm_index, ctx),
                ChainArm::Guard(guard) => {
                    let arm_index = guard.leaf.arm_index;
                    if let Some((condition_setup, condition)) =
                        self.try_plan_guard_condition(arm_index)
                    {
                        let then_body = self.with_scope(|this| {
                            let mut then_body: Vec<Statement> = Vec::new();
                            this.lower_arm_leaf(&mut then_body, arm_index, ctx);
                            then_body
                        });
                        let mut guarded = condition_setup;
                        guarded.push(
                            LoweredStatement::If(IfPlan::for_match(
                                condition,
                                LoweredBlock {
                                    statements: then_body,
                                },
                                ElseArm::None,
                            ))
                            .into(),
                        );
                        statements.extend(directed_first(self.arm_line(arm_index), guarded));
                    }
                }
            }
        }
    }

    fn lower_chain_group_per_test(
        &mut self,
        statements: &mut Vec<Statement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        for (j, &test_index) in indices.iter().enumerate() {
            let is_last_in_group = j == indices.len() - 1;
            let arm = &tests[test_index].arm;
            let needs_wrapper = !is_last_in_group && !arm.leaf().bindings.is_empty();
            if needs_wrapper {
                let wrapped = self.with_scope(|this| {
                    let mut wrapped: Vec<Statement> = Vec::new();
                    this.walk_chain_arm(&mut wrapped, arm, ctx);
                    wrapped
                });
                statements.push(
                    LoweredStatement::Block(LoweredBlock {
                        statements: wrapped,
                    })
                    .into(),
                );
            } else {
                self.walk_chain_arm(statements, arm, ctx);
            }
        }
    }

    fn walk_chain_arm(&mut self, statements: &mut Vec<Statement>, arm: &ChainArm, ctx: &WalkCtx) {
        match arm {
            ChainArm::Leaf(leaf) => self.walk_leaf(statements, leaf, ctx),
            ChainArm::Guard(guard) => self.walk_guard(statements, guard, ctx),
        }
    }

    fn render_chain_conditions(&mut self, tests: &[ChainTest]) -> Vec<Option<GoExpression>> {
        tests
            .iter()
            .map(|test| {
                (!test.checks.is_empty()).then(|| {
                    render_condition(&mut self.planner.scope, &test.checks, self.subject.root())
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
struct ChainGroup<'a> {
    indices: &'a [usize],
    tests: &'a [ChainTest],
    conditions: &'a [Option<GoExpression>],
}

pub(super) fn split_chain_with_catchall_lift<'t>(
    tests: &'t [ChainTest],
    tail: &'t ChainTail,
) -> (&'t [ChainTest], Option<&'t ArmLeaf>) {
    match (tail, tests.split_last()) {
        (ChainTail::Catchall(catchall), _) => (tests, Some(catchall)),
        (
            ChainTail::LastTest,
            Some((
                ChainTest {
                    arm: ChainArm::Leaf(leaf),
                    ..
                },
                rest,
            )),
        ) => (rest, Some(leaf)),
        _ => (tests, None),
    }
}

fn bindings_are_hoistable(tests: &[ChainTest], indices: &[usize]) -> bool {
    if indices.len() <= 1 {
        return false;
    }
    let reference = indices.iter().find_map(|&index| {
        let bindings = &tests[index].arm.leaf().bindings;
        if !bindings.is_empty() {
            Some(bindings)
        } else {
            None
        }
    });

    let Some(reference) = reference else {
        return false;
    };
    indices.iter().all(|&index| {
        let bindings = &tests[index].arm.leaf().bindings;
        bindings.is_empty()
            || (bindings.len() == reference.len()
                && bindings
                    .iter()
                    .zip(reference.iter())
                    .all(|(binding, reference_binding)| {
                        binding.lisette_name == reference_binding.lisette_name
                            && binding.target.go_name() == reference_binding.target.go_name()
                            && binding.path == reference_binding.path
                    }))
    })
}

fn group_chain_tests_by_condition(
    conditions: &[Option<GoExpression>],
) -> Vec<(Option<&GoExpression>, Vec<usize>)> {
    let mut groups: Vec<(Option<&GoExpression>, Vec<usize>)> = Vec::new();
    for (i, condition) in conditions.iter().enumerate() {
        let key = condition.as_ref();
        if let Some((last_key, indices)) = groups.last_mut()
            && *last_key == key
        {
            indices.push(i);
            continue;
        }
        groups.push((key, vec![i]));
    }
    groups
}

struct ChainBranch {
    condition: GoExpression,
    body: LoweredBlock,
    line: Option<Position>,
}

fn build_chain_plan(branches: Vec<ChainBranch>, trailing: ElseArm) -> IfPlan {
    let mut branches = branches;
    let head = branches.remove(0);
    let mut else_arm = trailing;
    for branch in branches.into_iter().rev() {
        let mut plan = IfPlan::for_match(branch.condition, branch.body, else_arm);
        plan.else_if_line = branch.line;
        else_arm = ElseArm::ElseIf(Box::new(plan));
    }
    IfPlan::for_match(head.condition, head.body, else_arm)
}
