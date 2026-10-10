use crate::plan::bodies::GoUses;
use syntax::ast::{BinaryOperator, Expression, Literal, MatchArm, UnaryOperator};
use syntax::types::Type;

use crate::Planner;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate_ids};
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::{is_catchall_pattern, is_unconditional_catchall};
use crate::patterns::binding_emit::tree_binding_statements;
use crate::patterns::decision_tree::{
    AccessPath, ArmLeaf, ChainArm, ChainTest, Decision, GuardedLeaf, PatternBinding, SubjectRoot,
    SwitchBranch, SwitchKind as PatternSwitchKind, SwitchShape, ValueSwitch, boolean_literal,
    compile_match_arms, render_condition, tree_has_unguarded_terminal,
};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    PlacePlan, Statement, SwitchCasePlan, SwitchKind, SwitchStatementPlan,
};
use crate::plan::go_expression::{BinaryOp, GoExpressionNode};
use crate::plan::local::GoIdentifier;
use crate::plan::placement::unreachable_panic_if_needed;
use crate::plan::values::GoExpression;
use crate::state::bindings::InlineExpr;

struct FlatCase<'d> {
    conditions: Vec<GoExpression>,
    body: FlatCaseBody<'d>,
}

enum FlatCaseBody<'d> {
    Leaf(&'d ArmLeaf),
    Guard(&'d ArmLeaf),
}

fn guard_renders_inline(guard: &Expression) -> bool {
    match guard {
        Expression::Literal { literal, ty, .. } => {
            matches!(
                literal,
                Literal::Integer { .. }
                    | Literal::Float { .. }
                    | Literal::Imaginary(_)
                    | Literal::Boolean(_)
                    | Literal::String { .. }
                    | Literal::Char(_)
            ) && ty.as_simple().is_some()
        }
        Expression::Identifier { ty, .. } => ty.as_simple().is_some(),
        Expression::Paren { expression, .. } => guard_renders_inline(expression),
        Expression::Unary {
            operator,
            expression,
            ..
        } => {
            matches!(
                operator,
                UnaryOperator::Not | UnaryOperator::Negative | UnaryOperator::BitwiseNot
            ) && guard_renders_inline(expression)
        }
        Expression::Binary {
            left,
            operator,
            right,
            ..
        } => {
            !matches!(operator, BinaryOperator::Pipeline)
                && guard_renders_inline(left)
                && guard_renders_inline(right)
        }
        _ => false,
    }
}

fn join_and(conditions: Vec<GoExpression>) -> GoExpression {
    conditions
        .into_iter()
        .reduce(|left, right| GoExpression::binary(left, BinaryOp::And, right))
        .expect("join_and requires at least one condition")
}

#[derive(Clone, Copy)]
struct ChainGroup<'a> {
    indices: &'a [usize],
    tests: &'a [ChainTest],
    conditions: &'a [Option<GoExpression>],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WalkRole {
    SwitchCase,
    ChainBody,
    RetryLoopTop,
    RetryLoopNested,
}

#[derive(Clone, Copy)]
struct WalkCtx<'a> {
    arm_place: &'a PlacePlan<'a>,
    role: WalkRole,
    /// `Some` on retry-loop walks needing a `break <label>` terminator at
    /// non-divergent leaves.
    break_label: Option<&'a str>,
}

impl<'a> WalkCtx<'a> {
    fn switch_case(arm_place: &'a PlacePlan<'a>) -> Self {
        Self {
            arm_place,
            role: WalkRole::SwitchCase,
            break_label: None,
        }
    }

    fn chain_test(arm_place: &'a PlacePlan<'a>) -> Self {
        Self {
            arm_place,
            role: WalkRole::ChainBody,
            break_label: None,
        }
    }

    fn retry_loop(arm_place: &'a PlacePlan<'a>, break_label: Option<&'a str>) -> Self {
        Self {
            arm_place,
            role: WalkRole::RetryLoopTop,
            break_label,
        }
    }

    fn nested(self) -> Self {
        let role = match self.role {
            WalkRole::SwitchCase | WalkRole::ChainBody => WalkRole::ChainBody,
            WalkRole::RetryLoopTop | WalkRole::RetryLoopNested => WalkRole::RetryLoopNested,
        };
        Self { role, ..self }
    }

    fn is_grouped_retry(&self) -> bool {
        matches!(
            self.role,
            WalkRole::RetryLoopTop | WalkRole::RetryLoopNested
        )
    }

    fn leaf_scope_explicit(&self) -> bool {
        matches!(self.role, WalkRole::RetryLoopTop)
    }
}

pub(crate) enum MatchSubject {
    Var(GoExpression),
    Elements(Vec<GoExpression>),
}

impl MatchSubject {
    pub(crate) fn root(&self) -> SubjectRoot<'_> {
        match self {
            Self::Var(var) => SubjectRoot::Var(var),
            Self::Elements(elements) => SubjectRoot::Elements(elements),
        }
    }
}

pub(crate) struct TreePlanner<'a, 'e> {
    planner: &'a mut Planner<'e>,
    arms: &'a [MatchArm],
    subject: MatchSubject,
    subject_ty: Type,
}

impl<'a, 'e> TreePlanner<'a, 'e> {
    pub(crate) fn new(
        planner: &'a mut Planner<'e>,
        arms: &'a [MatchArm],
        subject_var: MatchSubject,
        subject_ty: Type,
    ) -> Self {
        Self {
            planner,
            arms,
            subject: subject_var,
            subject_ty,
        }
    }

    pub(crate) fn lower(mut self, place: &PlacePlan) -> LoweredBlock {
        let tree = compile_match_arms(self.planner, self.arms, &self.subject_ty);

        let mut statements: Vec<Statement> = Vec::new();
        match &tree {
            Decision::Switch(_) | Decision::TypeSwitch { .. } => {
                let ctx = WalkCtx::switch_case(place);
                self.walk(&mut statements, &tree, &ctx);
            }
            Decision::Success(leaf) => {
                self.render_single_catchall(&mut statements, leaf, place);
            }
            _ if self.arms.iter().any(|arm| arm.has_guard()) => {
                self.render_retry_loop(&mut statements, &tree, place);
            }
            Decision::Chain { tests, catchall } => {
                self.render_chain_root(&mut statements, tests, catchall.as_ref(), place);
            }
            Decision::Unreachable => {
                self.render_chain_root(&mut statements, &[], None, place);
            }
            Decision::Guard(_) => unreachable!("a guard root implies a guarded arm"),
        }
        LoweredBlock { statements }
    }

    fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.planner.enter_scope();
        let result = f(self);
        self.planner.exit_scope();
        result
    }

    fn with_binding_frame<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.planner.scope.push_binding_frame();
        let result = f(self);
        self.planner.scope.pop_binding_frame();
        result
    }

    fn with_optional_scope<R>(&mut self, scoped: bool, f: impl FnOnce(&mut Self) -> R) -> R {
        if scoped { self.with_scope(f) } else { f(self) }
    }

    fn render_single_catchall(
        &mut self,
        statements: &mut Vec<Statement>,
        leaf: &ArmLeaf,
        place: &PlacePlan,
    ) {
        let ArmLeaf {
            arm_index,
            bindings,
        } = leaf;
        let arm_index = *arm_index;
        let pattern_has_collisions = self
            .planner
            .pattern_has_binding_collisions(&self.arms[arm_index].pattern);
        let arm_body = &*self.arms[arm_index].expression;

        let (inner, needs_block) = self.with_scope(|this| {
            let mut inner: Vec<Statement> = Vec::new();
            this.with_bindings(&mut inner, bindings, &[arm_body], |this, inner| {
                this.emit_arm_body(inner, arm_index, place)
            });
            let needs_block =
                this.planner.scope.current_block_declared_nonempty() || pattern_has_collisions;
            (inner, needs_block)
        });

        if needs_block {
            statements.push(LoweredStatement::Block(LoweredBlock { statements: inner }).into());
        } else {
            statements.extend(inner);
        }
    }

    fn render_chain_root(
        &mut self,
        statements: &mut Vec<Statement>,
        tests: &[ChainTest],
        catchall: Option<&ArmLeaf>,
        place: &PlacePlan,
    ) {
        let chain_tail_is_exhaustive = catchall.is_some()
            || chain_last_is_catchall(tests, catchall)
            || self
                .arms
                .last()
                .is_some_and(|arm| !arm.has_guard() && is_unconditional_catchall(&arm.pattern));
        self.lower_chain_branch(statements, tests, catchall, place);
        if let Some(panic) = unreachable_panic_if_needed(place, chain_tail_is_exhaustive) {
            statements.push(panic);
        }
    }

    fn lower_chain_branch(
        &mut self,
        statements: &mut Vec<Statement>,
        tests: &[ChainTest],
        catchall: Option<&ArmLeaf>,
        place: &PlacePlan,
    ) {
        let last_is_catchall = chain_last_is_catchall(tests, catchall);
        let conditions = self.render_chain_conditions(tests);
        let regular_len = if last_is_catchall {
            tests.len() - 1
        } else {
            tests.len()
        };

        let guard_ctx = WalkCtx::switch_case(place);
        let chain_ctx = WalkCtx::chain_test(place);

        // Lower each branch body in its own scope, recording whether every
        // branch diverges so the trailing else decides else/flat structurally.
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
            branches.push(ChainBranch { condition, body });
        }

        let trailing = if last_is_catchall {
            match &tests.last().unwrap().arm {
                ChainArm::Leaf(leaf) => self.lower_leaf_else_or_flat(leaf, &chain_ctx, all_diverge),
                ChainArm::Guard(guard) => self.lower_else_body(all_diverge, |this, body| {
                    this.walk_guard(body, guard, &chain_ctx)
                }),
            }
        } else if let Some(catchall) = catchall {
            self.lower_leaf_else_or_flat(catchall, &chain_ctx, all_diverge)
        } else {
            ElseArm::None
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

    fn render_retry_loop(
        &mut self,
        statements: &mut Vec<Statement>,
        tree: &Decision,
        place: &PlacePlan,
    ) {
        let all_arms_diverge = self
            .arms
            .iter()
            .all(|arm| arm.expression.diverges().is_some());
        let root_has_unguarded_terminal = tree_has_unguarded_terminal(tree);
        let last_arm_is_any_catchall = self
            .arms
            .last()
            .is_some_and(|arm| !arm.has_guard() && is_catchall_pattern(&arm.pattern));

        let use_direct_return = place.is_return();
        let unguarded_exit = root_has_unguarded_terminal || last_arm_is_any_catchall;
        let skip_wrapper = !use_direct_return && unguarded_exit && all_arms_diverge;

        // No `for { ... }` wrapper: walk the tree flat (direct-return or a
        // diverging-exit fast path).
        if use_direct_return || skip_wrapper {
            let ctx = WalkCtx::retry_loop(place, None);
            self.walk(statements, tree, &ctx);
            if use_direct_return && !root_has_unguarded_terminal {
                statements.push(LoweredStatement::UnreachablePanic.into());
            }
            return;
        }

        if self.guarded_tree_flattens(tree)
            && self.render_conditional_switch(statements, tree, place)
        {
            return;
        }

        // Wrap the tree in a labeled `for { ... }` retry loop.
        let label = self.planner.fresh_var(Some("match"));
        let ctx = WalkCtx::retry_loop(place, Some(label.as_str()));
        let mut body: Vec<Statement> = Vec::new();
        self.walk(&mut body, tree, &ctx);
        if !unguarded_exit {
            body.push(LoweredStatement::Break(LoopTransfer::Labeled(label.clone())).into());
        }
        statements.push(
            LoweredStatement::Loop(LoopPlan {
                kind: LoopKind::Generated { label: Some(label) },
                header: LoopHeader::Infinite,
                body: LoweredBlock { statements: body },
            })
            .into(),
        );
    }

    fn guarded_tree_flattens(&self, tree: &Decision) -> bool {
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
        self.arms[guard.leaf.arm_index]
            .guard
            .as_deref()
            .is_some_and(guard_renders_inline)
            && guard
                .leaf
                .bindings
                .iter()
                .all(|binding| !binding.path.contains_deferred_evaluation())
            && self.guarded_tree_flattens(&guard.failure)
    }

    fn render_conditional_switch(
        &mut self,
        statements: &mut Vec<Statement>,
        tree: &Decision,
        place: &PlacePlan,
    ) -> bool {
        let mut collected: Vec<FlatCase> = Vec::new();
        if !self.collect_flat_cases(tree, &mut Vec::new(), &mut collected, true) {
            return false;
        }
        let default_at = collected.iter().position(|case| case.conditions.is_empty());
        let default_case = default_at.map(|at| collected.split_off(at).remove(0));
        let has_default = default_case.is_some();

        let cases = collected
            .into_iter()
            .map(|case| SwitchCasePlan {
                labels: vec![join_and(case.conditions.clone())],
                body: self.lower_flat_case_body(&case, place),
            })
            .collect::<Vec<_>>();
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
        tail: bool,
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
                !tail || self.collect_flat_cases(&guard.failure, conditions, out, tail)
            }
            Decision::Chain { tests, catchall } => {
                let (cased, lifted) = split_chain_with_catchall_lift(tests, catchall.as_ref());
                for test in cased {
                    let has_checks = !test.checks.is_empty();
                    if has_checks {
                        conditions.push(render_condition(&test.checks, self.subject.root()));
                    }
                    let flattened = match &test.arm {
                        ChainArm::Leaf(leaf) => {
                            out.push(FlatCase {
                                conditions: conditions.clone(),
                                body: FlatCaseBody::Leaf(leaf),
                            });
                            true
                        }
                        ChainArm::Guard(guard) => {
                            self.collect_guard_case(&guard.leaf, conditions, out)
                        }
                    };
                    if has_checks {
                        conditions.pop();
                    }
                    if !flattened {
                        return false;
                    }
                }
                if let Some(leaf) = lifted.or(catchall.as_ref()) {
                    out.push(FlatCase {
                        conditions: conditions.clone(),
                        body: FlatCaseBody::Leaf(leaf),
                    });
                }
                true
            }
            Decision::TypeSwitch { .. } => false,
            Decision::Switch(switch) => {
                let rendered_path = switch.path.render(self.subject.root());
                let shape = switch.shape();
                let (cased, lifted) =
                    split_with_default_lift(&switch.branches, switch.fallback.as_deref());
                for branch in cased {
                    conditions.push(switch_branch_condition(
                        &rendered_path,
                        &switch.kind,
                        &shape,
                        &branch.label,
                    ));
                    let flattened =
                        self.collect_flat_cases(&branch.decision, conditions, out, false);
                    conditions.pop();
                    if !flattened {
                        return false;
                    }
                }
                match lifted {
                    Some(lifted) => self.collect_flat_cases(lifted, conditions, out, tail),
                    None => true,
                }
            }
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
        let lowered = self.with_binding_frame(|this| {
            this.install_path_overlays(bindings);
            this.lower_guard_condition(arm_index)
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
            let stability = self.planner.path_read_stability(&composable);
            self.planner.scope.bind_inline_expr(
                &binding.lisette_name,
                &binding.binding_ids,
                InlineExpr::new(composable, stability),
            );
        }
    }

    fn lower_flat_case_body(&mut self, case: &FlatCase, place: &PlacePlan) -> LoweredBlock {
        let ctx = WalkCtx::switch_case(place);
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
                        this.emit_arm_leaf(body, leaf.arm_index, &ctx);
                    });
                }
            }
            LoweredBlock { statements: body }
        })
    }

    fn walk(&mut self, statements: &mut Vec<Statement>, decision: &Decision, ctx: &WalkCtx) {
        match decision {
            Decision::Success(leaf) => self.walk_leaf(statements, leaf, ctx),
            Decision::Guard(guard) => self.walk_guard(statements, guard, ctx),
            Decision::Switch(switch) => self.walk_switch(statements, switch, ctx),
            Decision::TypeSwitch { branches, fallback } => {
                let subject = AccessPath::root().render(self.subject.root());
                let switch =
                    self.lower_type_switch(subject, branches, fallback.as_deref(), ctx.arm_place);
                let body_diverges = capture_diverge(switch, statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
            Decision::Chain { tests, catchall } => {
                if ctx.is_grouped_retry() {
                    self.emit_chain_grouped(statements, tests, catchall.as_ref(), ctx);
                } else {
                    self.lower_chain_branch(statements, tests, catchall.as_ref(), ctx.arm_place);
                }
            }
            Decision::Unreachable => {}
        }
    }

    fn walk_leaf(&mut self, statements: &mut Vec<Statement>, leaf: &ArmLeaf, ctx: &WalkCtx) {
        let wrap = ctx.leaf_scope_explicit();
        let arm_body = &*self.arms[leaf.arm_index].expression;
        let lowered = self.with_optional_scope(wrap, |this| {
            let mut lowered: Vec<Statement> = Vec::new();
            this.with_bindings(
                &mut lowered,
                &leaf.bindings,
                &[arm_body],
                |this, lowered| {
                    this.emit_arm_leaf(lowered, leaf.arm_index, ctx);
                },
            );
            lowered
        });
        if wrap {
            statements.push(
                LoweredStatement::Block(LoweredBlock {
                    statements: lowered,
                })
                .into(),
            );
        } else {
            statements.extend(lowered);
        }
    }

    fn emit_arm_leaf(&mut self, statements: &mut Vec<Statement>, arm_index: usize, ctx: &WalkCtx) {
        let mut body_statements: Vec<Statement> = Vec::new();
        self.emit_arm_body(&mut body_statements, arm_index, ctx.arm_place);
        let body_diverges = capture_diverge(body_statements, statements);
        apply_leaf_terminator(statements, ctx, body_diverges);
    }

    fn walk_switch(
        &mut self,
        statements: &mut Vec<Statement>,
        switch: &ValueSwitch,
        ctx: &WalkCtx,
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
            SwitchShape::Bool => {
                let true_branch = branches
                    .iter()
                    .find(|branch| boolean_literal(&branch.label) == Some(true))
                    .expect("Bool shape requires a true-labeled branch");
                let false_branch = branches
                    .iter()
                    .find(|branch| boolean_literal(&branch.label) == Some(false))
                    .expect("Bool shape requires a false-labeled branch");
                self.walk_condition_branch(
                    statements,
                    rendered_path,
                    &true_branch.decision,
                    &false_branch.decision,
                    ctx,
                );
            }
            SwitchShape::Binary => {
                let condition = GoExpression::binary(
                    render_switch_expression(rendered_path, kind),
                    BinaryOp::Eq,
                    branches[0].label.clone(),
                );
                self.walk_condition_branch(
                    statements,
                    condition,
                    &branches[0].decision,
                    &branches[1].decision,
                    ctx,
                );
            }
            SwitchShape::SingleArm => {
                let branch = &branches[0];
                let Some(fallback) = fallback else {
                    let inner = WalkCtx::switch_case(ctx.arm_place);
                    let mut branch_statements: Vec<Statement> = Vec::new();
                    self.walk(&mut branch_statements, &branch.decision, &inner);
                    let body_diverges = capture_diverge(branch_statements, statements);
                    apply_leaf_terminator(statements, ctx, body_diverges);
                    return;
                };
                let condition = GoExpression::binary(
                    render_switch_expression(rendered_path, kind),
                    BinaryOp::Eq,
                    branch.label.clone(),
                );
                self.walk_condition_branch(statements, condition, &branch.decision, fallback, ctx);
            }
            SwitchShape::Multi => {
                let expr = render_switch_expression(rendered_path, kind);
                let switch = self.lower_value_switch(expr, branches, fallback, ctx.arm_place);
                let body_diverges = capture_diverge(switch, statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
        }
    }

    fn walk_condition_branch(
        &mut self,
        statements: &mut Vec<Statement>,
        condition: GoExpression,
        then_branch: &Decision,
        else_branch: &Decision,
        ctx: &WalkCtx,
    ) {
        let inner = WalkCtx::switch_case(ctx.arm_place);
        let then_statements = self.with_scope(|this| {
            this.planner.scope.establish_condition(condition.clone());
            let mut then_statements: Vec<Statement> = Vec::new();
            this.walk(&mut then_statements, then_branch, &inner);
            then_statements
        });
        let then_body = LoweredBlock {
            statements: then_statements,
        };
        let then_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_or_flat(else_branch, &inner, then_diverges);
        let plan = IfPlan::plain(condition, then_body, else_arm);
        let body_diverges = capture_diverge(vec![LoweredStatement::If(plan).into()], statements);
        apply_leaf_terminator(statements, ctx, body_diverges);
    }

    fn walk_guard(&mut self, statements: &mut Vec<Statement>, guard: &GuardedLeaf, ctx: &WalkCtx) {
        let GuardedLeaf {
            leaf: ArmLeaf {
                arm_index,
                bindings,
            },
            failure,
        } = guard;
        let arm_index = *arm_index;
        let needs_pre_scope = ctx.leaf_scope_explicit() && !bindings.is_empty();
        let arm = &self.arms[arm_index];
        let arm_body = &*arm.expression;
        let mut guard_consumers: Vec<&Expression> = Vec::with_capacity(2);
        if let Some(guard) = arm.guard.as_deref() {
            guard_consumers.push(guard);
        }
        guard_consumers.push(arm_body);

        // Collect the bindings and the guard `if` into one block so a pre-scope
        // can wrap them as a single `LoweredStatement::Block`.
        let guard_statements = self.with_optional_scope(needs_pre_scope, |this| {
            let mut guard_statements: Vec<Statement> = Vec::new();
            let guarded = this.with_bindings(
                &mut guard_statements,
                bindings,
                &guard_consumers,
                |this, _| {
                    let (condition_setup, condition) = this.lower_guard_condition(arm_index)?;
                    let then_body = this.with_scope(|this| {
                        let mut then_statements: Vec<Statement> = Vec::new();
                        this.emit_arm_leaf(&mut then_statements, arm_index, &ctx.nested());
                        LoweredBlock {
                            statements: then_statements,
                        }
                    });
                    Some((condition_setup, condition, then_body))
                },
            );
            if let Some((condition_setup, condition, then_body)) = guarded {
                let success_diverges = then_body.ends_with_diverge();
                let else_arm = if ctx.role == WalkRole::SwitchCase {
                    this.lower_else_or_flat(failure, ctx, success_diverges)
                } else {
                    ElseArm::None
                };
                guard_statements.extend(condition_setup);
                guard_statements.push(
                    LoweredStatement::If(IfPlan {
                        initializer: None,
                        condition,
                        then_body,
                        else_arm,
                    })
                    .into(),
                );
            }
            guard_statements
        });
        if needs_pre_scope {
            statements.push(
                LoweredStatement::Block(LoweredBlock {
                    statements: guard_statements,
                })
                .into(),
            );
        } else {
            statements.extend(guard_statements);
        }
        if ctx.role == WalkRole::RetryLoopTop {
            self.walk(statements, failure, ctx);
        }
    }

    /// Build the `else` arm for a chain/guard branch. An empty decision yields
    /// no else; when the preceding branch diverges the decision is flattened
    /// after the `if` (`ElseArm::Else { inline: true }`) instead of nesting in
    /// an `else` block.
    fn lower_else_or_flat(
        &mut self,
        decision: &Decision,
        ctx: &WalkCtx,
        preceding_diverges: bool,
    ) -> ElseArm {
        match decision {
            Decision::Success(leaf) => self.lower_leaf_else_or_flat(leaf, ctx, preceding_diverges),
            _ => self.lower_else_body(preceding_diverges, |this, body| {
                this.walk(body, decision, ctx)
            }),
        }
    }

    fn lower_leaf_else_or_flat(
        &mut self,
        leaf: &ArmLeaf,
        ctx: &WalkCtx,
        preceding_diverges: bool,
    ) -> ElseArm {
        if leaf.bindings.is_empty() && body_is_unit_or_empty(&self.arms[leaf.arm_index].expression)
        {
            return ElseArm::None;
        }
        self.lower_else_body(preceding_diverges, |this, body| {
            this.walk_leaf(body, leaf, ctx)
        })
    }

    fn lower_else_body(
        &mut self,
        preceding_diverges: bool,
        lower: impl FnOnce(&mut Self, &mut Vec<Statement>),
    ) -> ElseArm {
        if preceding_diverges {
            let mut body: Vec<Statement> = Vec::new();
            lower(self, &mut body);
            return ElseArm::from_body(LoweredBlock { statements: body }, true);
        }
        let body = self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            lower(this, &mut body);
            body
        });
        ElseArm::from_body(LoweredBlock { statements: body }, false)
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
                let established =
                    GoExpression::binary(subject.clone(), BinaryOp::Eq, branch.label.clone());
                self.lower_switch_case(
                    vec![branch.label.clone()],
                    Some(established),
                    &branch.decision,
                    place,
                )
            })
            .collect();
        let default_block = self.lower_switch_default(default, place);
        switch_statements(
            SwitchStatementPlan {
                kind: SwitchKind::Value { subject },
                cases: case_plans,
                default: default_block,
            },
            place,
            default.is_some(),
        )
    }

    fn lower_type_switch(
        &mut self,
        subject: GoExpression,
        branches: &[SwitchBranch<Vec<String>>],
        fallback: Option<&Decision>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        let (regular, default) = split_with_default_lift(branches, fallback);
        let arms = self.arms;
        let subject_ty = self.subject_ty.clone();
        let binding_name = match subject.node() {
            GoExpressionNode::Identifier(name) => {
                GoIdentifier::local(name.to_string(), self.planner.scope.new_local_id())
            }
            _ => {
                let name = self.planner.fresh_var(Some("subject"));
                self.planner.declare(&name);
                self.planner.scope.generated_identifier(&name)
            }
        };
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
        let default_block = nested.lower_switch_default(default, place);

        // Keep the `base :=` type-switch binding only when a case references it;
        // Go rejects an unused `:= base` assignment otherwise.
        let mut used = GoUses::default();
        used.extend_cases(&case_plans);
        if let Some(block) = &default_block {
            used.extend(&block.statements);
        }
        let binding = used
            .contains_identifier(&binding_name)
            .then_some(binding_name);

        switch_statements(
            SwitchStatementPlan {
                kind: SwitchKind::Type { subject, binding },
                cases: case_plans,
                default: default_block,
            },
            place,
            default.is_some(),
        )
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
            this.walk(&mut body, decision, &WalkCtx::switch_case(place));
            body
        });
        SwitchCasePlan {
            labels,
            body: LoweredBlock { statements: body },
        }
    }

    /// Lower the default arm, dropping it when its body lowers to nothing (Go
    /// would otherwise emit a bare `default:`).
    fn lower_switch_default(
        &mut self,
        default: Option<&Decision>,
        place: &PlacePlan,
    ) -> Option<LoweredBlock> {
        let default_decision = default?;
        let ctx = WalkCtx::switch_case(place);
        let body = self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            this.walk(&mut body, default_decision, &ctx);
            body
        });
        (!body.is_empty()).then_some(LoweredBlock { statements: body })
    }

    fn emit_chain_grouped(
        &mut self,
        statements: &mut Vec<Statement>,
        tests: &[ChainTest],
        catchall: Option<&ArmLeaf>,
        ctx: &WalkCtx,
    ) {
        let last_is_catchall = chain_last_is_catchall(tests, catchall);
        let conditions = self.render_chain_conditions(tests);
        let inner_ctx = ctx.nested();
        let groups = group_chain_tests_by_condition(&conditions);
        let group_count = groups.len();

        for (g, (_condition, indices)) in groups.iter().enumerate() {
            let is_last_group = g == group_count - 1;
            let collapse_as_catchall = is_last_group && last_is_catchall;
            self.emit_chain_group(
                statements,
                ChainGroup {
                    indices,
                    tests,
                    conditions: &conditions,
                },
                &inner_ctx,
                collapse_as_catchall,
            );
        }

        if let Some(catchall) = catchall {
            self.walk_leaf(statements, catchall, ctx);
        }
    }

    fn emit_chain_group(
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
            self.emit_chain_group_tests(statements, indices, tests, ctx);
            return;
        }

        let body = self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            this.emit_chain_group_tests(&mut body, indices, tests, ctx);
            body
        });

        let body = LoweredBlock { statements: body };
        let first_condition = &conditions[indices[0]];
        match first_condition {
            Some(condition) => statements.push(
                LoweredStatement::If(IfPlan::plain(condition.clone(), body, ElseArm::None)).into(),
            ),
            None => statements.push(LoweredStatement::Block(body).into()),
        }
    }

    fn emit_chain_group_tests(
        &mut self,
        statements: &mut Vec<Statement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        if bindings_are_hoistable(tests, indices) {
            self.emit_chain_group_hoisted(statements, indices, tests, ctx);
        } else {
            self.emit_chain_group_per_test(statements, indices, tests, ctx);
        }
    }

    fn emit_chain_group_hoisted(
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
                this.emit_chain_group_bodies(statements, indices, tests, ctx);
            });
        } else {
            self.emit_chain_group_bodies(statements, indices, tests, ctx);
        }
    }

    fn emit_chain_group_bodies(
        &mut self,
        statements: &mut Vec<Statement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        for &test_index in indices {
            match &tests[test_index].arm {
                ChainArm::Leaf(leaf) => self.emit_arm_leaf(statements, leaf.arm_index, ctx),
                ChainArm::Guard(guard) => {
                    let arm_index = guard.leaf.arm_index;
                    if let Some((condition_setup, condition)) =
                        self.lower_guard_condition(arm_index)
                    {
                        let then_body = self.with_scope(|this| {
                            let mut then_body: Vec<Statement> = Vec::new();
                            this.emit_arm_leaf(&mut then_body, arm_index, ctx);
                            then_body
                        });
                        statements.extend(condition_setup);
                        statements.push(
                            LoweredStatement::If(IfPlan {
                                initializer: None,
                                condition,
                                then_body: LoweredBlock {
                                    statements: then_body,
                                },
                                else_arm: ElseArm::None,
                            })
                            .into(),
                        );
                    }
                }
            }
        }
    }

    fn emit_chain_group_per_test(
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

    fn with_bindings<R>(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        consumers: &[&Expression],
        f: impl FnOnce(&mut Self, &mut Vec<Statement>) -> R,
    ) -> R {
        self.with_binding_frame(|this| {
            if !bindings.is_empty() {
                tree_binding_statements(
                    this.planner,
                    statements,
                    bindings,
                    this.subject.root(),
                    consumers,
                );
            }
            f(this, statements)
        })
    }

    fn emit_arm_body(
        &mut self,
        statements: &mut Vec<Statement>,
        arm_index: usize,
        place: &PlacePlan,
    ) {
        let arm = &self.arms[arm_index];
        let block = self.planner.lower_block_to_place(&arm.expression, place);
        statements.extend(block.statements);
    }

    /// Lower an arm's guard to the setup statements and the `IfPlan` condition,
    /// or `None` when the arm has no guard. The caller owns the scope and body.
    fn lower_guard_condition(
        &mut self,
        arm_index: usize,
    ) -> Option<(Vec<Statement>, GoExpression)> {
        let guard_expression = self.arms[arm_index].guard.as_deref()?;
        let plan = self
            .planner
            .plan_operand(guard_expression, ExpressionContext::value());
        let (setup, value) = plan.into_parts();
        Some((setup, value))
    }

    fn render_chain_conditions(&self, tests: &[ChainTest]) -> Vec<Option<GoExpression>> {
        tests
            .iter()
            .map(|test| {
                (!test.checks.is_empty())
                    .then(|| render_condition(&test.checks, self.subject.root()))
            })
            .collect()
    }
}

fn chain_last_is_catchall(tests: &[ChainTest], catchall: Option<&ArmLeaf>) -> bool {
    catchall.is_none() && tests.len() > 1
}

fn split_chain_with_catchall_lift<'t>(
    tests: &'t [ChainTest],
    catchall: Option<&ArmLeaf>,
) -> (&'t [ChainTest], Option<&'t ArmLeaf>) {
    if !chain_last_is_catchall(tests, catchall) {
        return (tests, None);
    }
    match tests.split_last() {
        Some((
            ChainTest {
                arm: ChainArm::Leaf(leaf),
                ..
            },
            rest,
        )) => (rest, Some(leaf)),
        _ => (tests, None),
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
    shape: &SwitchShape,
    label: &GoExpression,
) -> GoExpression {
    if matches!(shape, SwitchShape::Bool) && boolean_literal(label) == Some(true) {
        return rendered_path.clone();
    }
    GoExpression::binary(
        render_switch_expression(rendered_path.clone(), kind),
        BinaryOp::Eq,
        label.clone(),
    )
}

fn render_switch_expression(rendered_path: GoExpression, kind: &PatternSwitchKind) -> GoExpression {
    match kind {
        PatternSwitchKind::EnumTag => GoExpression::selector(rendered_path, "Tag".to_string()),
        PatternSwitchKind::Value => rendered_path,
    }
}

fn body_is_unit_or_empty(expression: &Expression) -> bool {
    matches!(expression, Expression::Unit { .. })
        || matches!(expression, Expression::Block { items, .. } if items.is_empty())
}

fn merge_binding_ids(bindings: &mut [PatternBinding], alternatives: &[PatternBinding]) {
    for alternative in alternatives {
        let Some(binding) = bindings
            .iter_mut()
            .find(|binding| binding.lisette_name == alternative.lisette_name)
        else {
            continue;
        };
        for id in &alternative.binding_ids {
            if !binding.binding_ids.contains(id) {
                binding.binding_ids.push(*id);
            }
        }
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

/// One non-catchall branch of a pattern chain: its condition and lowered body.
struct ChainBranch {
    condition: GoExpression,
    body: LoweredBlock,
}

/// Assemble pattern-chain branches into a nested `if`/`else if` plan, with
/// `trailing` as the innermost `else` arm. `branches` must be non-empty.
fn build_chain_plan(branches: Vec<ChainBranch>, trailing: ElseArm) -> IfPlan {
    let mut branches = branches;
    let head = branches.remove(0);
    let mut else_arm = trailing;
    for branch in branches.into_iter().rev() {
        else_arm = ElseArm::ElseIf(Box::new(IfPlan::plain(
            branch.condition,
            branch.body,
            else_arm,
        )));
    }
    IfPlan::plain(head.condition, head.body, else_arm)
}

/// The switch, then an unreachable panic when the place needs a tail return
/// and the switch is not exhaustive.
fn switch_statements(
    switch: SwitchStatementPlan,
    place: &PlacePlan,
    has_default: bool,
) -> Vec<Statement> {
    let mut statements = vec![LoweredStatement::Switch(switch).into()];
    statements.extend(unreachable_panic_if_needed(place, has_default));
    statements
}

/// Compute `ends_with_diverge` of `body_statements`, then move them into `statements`.
fn capture_diverge(body_statements: Vec<Statement>, statements: &mut Vec<Statement>) -> bool {
    let block = LoweredBlock {
        statements: body_statements,
    };
    let diverges = block.ends_with_diverge();
    statements.extend(block.statements);
    diverges
}

fn apply_leaf_terminator(statements: &mut Vec<Statement>, ctx: &WalkCtx, body_diverges: bool) {
    if let Some(label) = ctx.break_label
        && !body_diverges
    {
        statements.push(LoweredStatement::Break(LoopTransfer::Labeled(label.to_string())).into());
    }
}
