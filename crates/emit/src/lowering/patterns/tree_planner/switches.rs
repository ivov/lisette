use super::chains::split_chain_with_catchall_lift;
use super::{
    BranchTest, MatchSubject, TreePlanner, WalkContext, apply_leaf_terminator, capture_diverge,
    contains_call, join_and,
};
use crate::lowering::patterns::tree_planner::guard_renders_inline;
use std::iter;
use syntax::go_names::ENUM_TAG_FIELD;

use crate::lookup::facts::inline_uses::{InlineDecision, analyze_inline_candidate_ids};
use crate::lookup::state::bindings::InlineExpression;
use crate::lowering::patterns::decision_tree::{
    ArmLeaf, ChainArm, ChainTail, ChainTest, Decision, GuardedLeaf, PatternBinding, SwitchBranch,
    SwitchKind as PatternSwitchKind, SwitchShape, ValueSwitch, boolean_literal, render_condition,
};
use crate::lowering::statements::placement::unreachable_panic_if_needed;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::target::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, SwitchCasePlan, SwitchKind, SwitchStatementPlan,
};
use crate::target::ir::go_expression::{GoBinaryOperator, GoExpressionNode};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;

impl<'a, 'e> TreePlanner<'a, 'e> {
    pub(super) fn guarded_tree_flattens(&self, tree: &Decision) -> bool {
        match tree {
            Decision::Success(_) | Decision::Unreachable => true,
            Decision::Guard(guard) => self.guarded_leaf_flattens(guard),
            Decision::Chain { tests, .. } => tests.iter().all(|test| match &test.arm {
                ChainArm::Leaf(_) => true,
                ChainArm::Guard(guard) => self.guarded_leaf_flattens(guard),
            }),
            Decision::TypeSwitch { .. } => false,
            Decision::Switch(ValueSwitch {
                branches, fallback, ..
            }) => {
                branches
                    .iter()
                    .all(|branch| self.guarded_tree_flattens(&branch.decision))
                    && fallback
                        .as_deref()
                        .is_none_or(|fallback| self.guarded_tree_flattens(fallback))
            }
        }
    }

    fn guarded_leaf_flattens(&self, guard: &GuardedLeaf) -> bool {
        let Some(condition) = self.arms[guard.leaf.arm_index].guard.as_deref() else {
            return false;
        };
        let calls = contains_call(condition);
        guard_renders_inline(&self.planner.facts, condition)
            && guard.leaf.bindings.iter().all(|binding| {
                !binding.path.contains_deferred_evaluation()
                    && !(calls && binding.path.payload_is_index())
            })
            && self.guarded_tree_flattens(&guard.failure)
    }

    pub(super) fn render_conditional_switch(
        &mut self,
        statements: &mut Vec<Statement>,
        tree: &Decision,
        place: &PlacePlan,
    ) -> bool {
        let checkpoint = self.planner.planning_snapshot();
        let mut collected: Vec<FlatCase> = Vec::new();
        if !self.collect_flat_cases(tree, &mut Vec::new(), &mut collected, CasePosition::Tail) {
            self.planner.restore_planning(checkpoint);
            return false;
        }
        let mut labeled = Vec::new();
        let mut default_case = None;
        for case in collected {
            match join_and(&case.conditions) {
                Some(label) => labeled.push((label, case)),
                None => {
                    default_case = Some(case);
                    break;
                }
            }
        }
        let has_default = default_case.is_some();

        let cases = labeled
            .into_iter()
            .map(|(label, case)| SwitchCasePlan {
                labels: vec![label],
                body: self.lower_flat_case_body(&case, place),
                line: self.arm_line(case.body.arm_index()),
            })
            .collect::<Vec<_>>();

        let default_line = default_case
            .as_ref()
            .and_then(|case| self.arm_line(case.body.arm_index()));
        let default = default_case
            .map(|case| self.lower_flat_case_body(&case, place))
            .filter(|body| !body.renders_empty());

        if cases.is_empty() {
            if let Some(body) = default {
                statements.extend(body.statements);
            }
            return true;
        }

        statements.push(
            LoweredStatement::Switch(SwitchStatementPlan {
                kind: SwitchKind::Conditional,
                cases,
                default,
                default_line,
            })
            .into(),
        );
        statements.extend(unreachable_panic_if_needed(place, has_default));
        true
    }

    fn collect_flat_cases<'d>(
        &mut self,
        decision: &'d Decision,
        conditions: &mut Vec<GoExpression>,
        out: &mut Vec<FlatCase<'d>>,
        position: CasePosition,
    ) -> bool {
        match decision {
            Decision::Unreachable => true,
            Decision::Success(leaf) => {
                out.push(FlatCase {
                    conditions: conditions.clone(),
                    body: FlatCaseBody::Leaf(leaf),
                });
                true
            }
            Decision::Guard(guard) => {
                if !self.collect_guard_case(&guard.leaf, conditions, out) {
                    return false;
                }
                position == CasePosition::Nested
                    || self.collect_flat_cases(&guard.failure, conditions, out, position)
            }
            Decision::Chain { tests, tail } => {
                self.collect_chain_cases(tests, tail, conditions, out)
            }
            Decision::TypeSwitch { .. } => false,
            Decision::Switch(switch) => {
                self.collect_switch_cases(switch, conditions, out, position)
            }
        }
    }

    fn collect_chain_cases<'d>(
        &mut self,
        tests: &'d [ChainTest],
        tail: &'d ChainTail,
        conditions: &mut Vec<GoExpression>,
        out: &mut Vec<FlatCase<'d>>,
    ) -> bool {
        let (cased, lifted) = split_chain_with_catchall_lift(tests, tail);
        for test in cased {
            let has_checks = !test.checks.is_empty();
            if has_checks {
                conditions.push(render_condition(
                    &mut self.planner.scope,
                    &test.checks,
                    self.subject.root(),
                ));
            }
            let flattened = match &test.arm {
                ChainArm::Leaf(leaf) => {
                    out.push(FlatCase {
                        conditions: conditions.clone(),
                        body: FlatCaseBody::Leaf(leaf),
                    });
                    true
                }
                ChainArm::Guard(guard) => self.collect_guard_case(&guard.leaf, conditions, out),
            };

            if has_checks {
                conditions.pop();
            }
            if !flattened {
                return false;
            }
        }
        if let Some(leaf) = lifted {
            out.push(FlatCase {
                conditions: conditions.clone(),
                body: FlatCaseBody::Leaf(leaf),
            });
        }
        true
    }

    fn collect_switch_cases<'d>(
        &mut self,
        switch: &'d ValueSwitch,
        conditions: &mut Vec<GoExpression>,
        out: &mut Vec<FlatCase<'d>>,
        position: CasePosition,
    ) -> bool {
        let rendered_path = switch.path.render(self.subject.root());
        let shape = switch.shape();
        let (cased, lifted) = split_with_default_lift(&switch.branches, switch.fallback.as_deref());
        for branch in cased {
            let condition_of =
                |label| switch_branch_condition(&rendered_path, &switch.kind, shape, label);
            conditions.push(
                branch
                    .extra_labels
                    .iter()
                    .map(condition_of)
                    .fold(condition_of(&branch.label), |left, right| {
                        GoExpression::binary(left, GoBinaryOperator::Or, right)
                    }),
            );
            let flattened =
                self.collect_flat_cases(&branch.decision, conditions, out, CasePosition::Nested);
            conditions.pop();
            if !flattened {
                return false;
            }
        }
        match lifted {
            Some(lifted) => self.collect_flat_cases(lifted, conditions, out, position),
            None => true,
        }
    }

    fn collect_guard_case<'d>(
        &mut self,
        leaf: &'d ArmLeaf,
        conditions: &mut Vec<GoExpression>,
        out: &mut Vec<FlatCase<'d>>,
    ) -> bool {
        let Some(condition) = self.guard_condition_over_paths(leaf.arm_index, &leaf.bindings)
        else {
            return false;
        };
        conditions.push(condition);
        out.push(FlatCase {
            conditions: conditions.clone(),
            body: FlatCaseBody::Guard(leaf),
        });
        conditions.pop();
        true
    }

    fn guard_condition_over_paths(
        &mut self,
        arm_index: usize,
        bindings: &[PatternBinding],
    ) -> Option<GoExpression> {
        if bindings
            .iter()
            .any(|binding| binding.coercion.changes_layout())
        {
            return None;
        }
        let lowered = self.with_binding_frame(|this| {
            this.install_path_overlays(bindings);
            this.try_plan_guard_condition(arm_index)
        });
        let (setup, condition) = lowered?;
        if !setup.is_empty() {
            return None;
        }
        Some(condition)
    }

    fn install_path_overlays(&mut self, bindings: &[PatternBinding]) {
        for binding in bindings {
            if !binding.target.is_named() {
                continue;
            }
            let composable = binding.path.render(self.subject.root());
            let stability = self.planner.queries().path_read_stability(&composable);
            self.planner.scope.bind_inline_expression(
                &binding.lisette_name,
                &binding.binding_ids,
                InlineExpression::new(composable, stability),
            );
        }
    }

    fn lower_flat_case_body(&mut self, case: &FlatCase, place: &PlacePlan) -> LoweredBlock {
        let ctx = WalkContext::switch_case(place);
        self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            match case.body {
                FlatCaseBody::Leaf(leaf) => this.walk_leaf(&mut body, leaf, &ctx),
                FlatCaseBody::Guard(leaf) => {
                    let arm_body = &*this.arms[leaf.arm_index].expression;
                    let bindings: Vec<PatternBinding> = leaf
                        .bindings
                        .iter()
                        .filter(|binding| {
                            analyze_inline_candidate_ids(&binding.binding_ids, &[arm_body])
                                != InlineDecision::Unused
                        })
                        .cloned()
                        .collect();

                    this.with_bindings(&mut body, &bindings, &[arm_body], |this, body| {
                        this.lower_arm_leaf(body, leaf.arm_index, &ctx);
                    });
                }
            }
            LoweredBlock { statements: body }
        })
    }

    pub(super) fn walk_switch(
        &mut self,
        statements: &mut Vec<Statement>,
        switch: &ValueSwitch,
        ctx: &WalkContext,
    ) {
        let ValueSwitch {
            path,
            kind,
            branches,
            fallback,
        } = switch;

        let fallback = fallback.as_deref();
        let rendered_path = path.render(self.subject.root());
        match switch.shape() {
            SwitchShape::Bool {
                true_branch,
                false_branch,
            } => {
                self.walk_condition_branch(
                    statements,
                    BranchTest {
                        condition: rendered_path,
                        else_label: None,
                    },
                    &true_branch.decision,
                    &false_branch.decision,
                    ctx,
                );
            }
            SwitchShape::Binary { first, second } => {
                let subject = render_switch_expression(rendered_path, kind);
                let condition =
                    GoExpression::binary(subject, GoBinaryOperator::Equal, first.label.clone());
                let test = BranchTest {
                    condition,
                    else_label: Some(second.label.clone()),
                };
                self.walk_condition_branch(
                    statements,
                    test,
                    &first.decision,
                    &second.decision,
                    ctx,
                );
            }
            SwitchShape::SingleArm(branch) => {
                let subject = render_switch_expression(rendered_path, kind);
                self.walk_single_arm_switch(statements, subject, branch, fallback, ctx);
            }
            SwitchShape::Multi => {
                let expression = render_switch_expression(rendered_path, kind);
                let switch = self.lower_value_switch(expression, branches, fallback, ctx.arm_place);
                let divergence = capture_diverge(switch, statements);
                apply_leaf_terminator(statements, ctx, divergence);
            }
        }
    }

    fn walk_single_arm_switch(
        &mut self,
        statements: &mut Vec<Statement>,
        subject: GoExpression,
        branch: &SwitchBranch<GoExpression>,
        fallback: Option<&Decision>,
        ctx: &WalkContext,
    ) {
        let Some(fallback) = fallback else {
            let inner = WalkContext::switch_case(ctx.arm_place);
            let mut branch_statements: Vec<Statement> = Vec::new();
            self.walk(&mut branch_statements, &branch.decision, &inner);
            let divergence = capture_diverge(branch_statements, statements);
            apply_leaf_terminator(statements, ctx, divergence);
            return;
        };

        let condition =
            GoExpression::binary(subject, GoBinaryOperator::Equal, branch.label.clone());
        self.walk_condition_branch(
            statements,
            BranchTest {
                condition,
                else_label: None,
            },
            &branch.decision,
            fallback,
            ctx,
        );
    }

    fn lower_value_switch(
        &mut self,
        subject: GoExpression,
        branches: &[SwitchBranch<GoExpression>],
        fallback: Option<&Decision>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        let (regular, default) = split_with_default_lift(branches, fallback);
        let case_plans = regular
            .iter()
            .map(|branch| {
                let established = branch.extra_labels.is_empty().then(|| {
                    GoExpression::binary(
                        subject.clone(),
                        GoBinaryOperator::Equal,
                        branch.label.clone(),
                    )
                });
                let labels = iter::once(&branch.label)
                    .chain(&branch.extra_labels)
                    .cloned()
                    .collect();
                self.lower_switch_case(labels, established, &branch.decision, place)
            })
            .collect();

        let default_block = self.try_lower_switch_default(default, place);
        switch_statements(
            SwitchStatementPlan {
                kind: SwitchKind::Value { subject },
                cases: case_plans,
                default: default_block,
                default_line: self.decision_line(default),
            },
            place,
            default.is_some(),
        )
    }

    pub(super) fn lower_type_switch(
        &mut self,
        subject: GoExpression,
        branches: &[SwitchBranch<Vec<GoType>>],
        fallback: Option<&Decision>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        let (regular, default) = (branches, fallback);
        let arms = self.arms;
        let subject_ty = self.subject_ty.clone();
        let binding_name = self.type_switch_binding_name(&subject);

        let mut nested = TreePlanner::new(
            self.planner,
            arms,
            MatchSubject::Var(GoExpression::identifier(binding_name.clone())),
            subject_ty,
        );

        let case_plans: Vec<SwitchCasePlan> = regular
            .iter()
            .map(|branch| {
                let labels = branch
                    .label
                    .iter()
                    .map(|go_type| GoExpression::type_name(go_type.clone()))
                    .collect();
                nested.lower_switch_case(labels, None, &branch.decision, place)
            })
            .collect();

        let default_block = nested.try_lower_switch_default(default, place);

        switch_statements(
            SwitchStatementPlan {
                kind: SwitchKind::Type {
                    subject,
                    binding: Some(binding_name),
                },
                cases: case_plans,
                default: default_block,
                default_line: self.decision_line(default),
            },
            place,
            default.is_some(),
        )
    }

    fn type_switch_binding_name(&mut self, subject: &GoExpression) -> GoIdentifier {
        match subject.node() {
            GoExpressionNode::Identifier(name) => {
                GoIdentifier::local(name.to_string(), self.planner.scope.new_local_id())
            }
            _ => {
                let name = self.planner.fresh_local("subject");
                self.planner.scope.declare_go_name(&name);
                name
            }
        }
    }

    fn lower_switch_case(
        &mut self,
        labels: Vec<GoExpression>,
        established: Option<GoExpression>,
        decision: &Decision,
        place: &PlacePlan,
    ) -> SwitchCasePlan {
        let body = self.with_scope(|this| {
            if let Some(condition) = established {
                this.planner.scope.establish_condition(condition);
            }
            let mut body: Vec<Statement> = Vec::new();
            this.walk(&mut body, decision, &WalkContext::switch_case(place));
            body
        });

        SwitchCasePlan {
            labels,
            body: LoweredBlock { statements: body },
            line: self.decision_line(Some(decision)),
        }
    }

    fn try_lower_switch_default(
        &mut self,
        default: Option<&Decision>,
        place: &PlacePlan,
    ) -> Option<LoweredBlock> {
        let default_decision = default?;
        let ctx = WalkContext::switch_case(place);
        let body = self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            this.walk(&mut body, default_decision, &ctx);
            body
        });
        (!body.is_empty()).then_some(LoweredBlock { statements: body })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CasePosition {
    Tail,
    Nested,
}

struct FlatCase<'d> {
    conditions: Vec<GoExpression>,
    body: FlatCaseBody<'d>,
}

enum FlatCaseBody<'d> {
    Leaf(&'d ArmLeaf),
    Guard(&'d ArmLeaf),
}

impl FlatCaseBody<'_> {
    fn arm_index(&self) -> usize {
        match self {
            FlatCaseBody::Leaf(leaf) | FlatCaseBody::Guard(leaf) => leaf.arm_index,
        }
    }
}

fn split_with_default_lift<'t, L>(
    branches: &'t [SwitchBranch<L>],
    fallback: Option<&'t Decision>,
) -> (&'t [SwitchBranch<L>], Option<&'t Decision>) {
    match (fallback, branches.split_last()) {
        (None, Some((last, rest))) => (rest, Some(&last.decision)),
        _ => (branches, fallback),
    }
}

fn switch_branch_condition(
    rendered_path: &GoExpression,
    kind: &PatternSwitchKind,
    shape: SwitchShape<'_>,
    label: &GoExpression,
) -> GoExpression {
    if matches!(shape, SwitchShape::Bool { .. }) && boolean_literal(label) == Some(true) {
        return rendered_path.clone();
    }
    GoExpression::binary(
        render_switch_expression(rendered_path.clone(), kind),
        GoBinaryOperator::Equal,
        label.clone(),
    )
}

fn render_switch_expression(rendered_path: GoExpression, kind: &PatternSwitchKind) -> GoExpression {
    match kind {
        PatternSwitchKind::EnumTag => {
            GoExpression::selector(rendered_path, ENUM_TAG_FIELD.to_string())
        }
        PatternSwitchKind::Value => rendered_path,
    }
}

fn switch_statements(
    switch: SwitchStatementPlan,
    place: &PlacePlan,
    has_default: bool,
) -> Vec<Statement> {
    let mut statements = vec![LoweredStatement::Switch(switch).into()];
    statements.extend(unreachable_panic_if_needed(place, has_default));
    statements
}
