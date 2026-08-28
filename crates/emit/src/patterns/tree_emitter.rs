use syntax::ast::{BinaryOperator, Expression, Literal, MatchArm, UnaryOperator};
use syntax::types::Type;

use crate::Planner;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate};
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::{is_catchall_pattern, is_unconditional_catchall};
use crate::patterns::binding_emit::{drop_inline_overlays, tree_binding_statements};
use crate::patterns::decision_tree::{
    ChainTest, Decision, PatternBinding, SubjectRoot, SwitchBranch,
    SwitchKind as PatternSwitchKind, SwitchShape, compile_expanded_arms, decision_is_exhaustive,
    expand_or_patterns, render_condition, tree_has_unguarded_terminal,
};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement, PlacePlan,
    SwitchCasePlan, SwitchKind, SwitchStatementPlan,
};
use crate::plan::placement::unreachable_panic_if_needed;
use crate::state::bindings::{BindingValue, InlineExpr};
use crate::utils::wrap_if_struct_literal;

struct FlatCase<'d> {
    conditions: Vec<String>,
    bindings: &'d [PatternBinding],
    decision: &'d Decision,
}

fn decision_arm_index(decision: &Decision) -> usize {
    match decision {
        Decision::Success { arm_index, .. } | Decision::Guard { arm_index, .. } => *arm_index,
        _ => unreachable!("a flattened case body lowers from an arm leaf"),
    }
}

fn binds_looser_than_conjunction(guard: &Expression) -> bool {
    matches!(
        guard,
        Expression::Binary {
            operator: BinaryOperator::Or,
            ..
        }
    )
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

#[derive(Clone, Copy)]
struct ChainGroup<'a> {
    indices: &'a [usize],
    tests: &'a [ChainTest],
    conditions: &'a [Option<String>],
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
    Var(String),
    Elements(Vec<String>),
}

impl MatchSubject {
    /// The one name a binding resolves against.
    fn var(&self) -> &str {
        match self {
            Self::Var(var) => var,
            Self::Elements(_) => unreachable!("tuple elements carry no bindings"),
        }
    }

    pub(crate) fn root(&self) -> SubjectRoot<'_> {
        match self {
            Self::Var(var) => SubjectRoot::Var(var),
            Self::Elements(names) => SubjectRoot::Elements(names),
        }
    }

    fn names(&self) -> Vec<&str> {
        self.root().names()
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
        let expanded = expand_or_patterns(self.arms);
        let compiled = compile_expanded_arms(self.planner, &expanded, &self.subject_ty);
        self.planner.require_packages(&compiled.packages);
        let tree = compiled.decision;

        let mut statements: Vec<LoweredStatement> = Vec::new();
        match &tree {
            Decision::Switch { .. } => {
                let ctx = WalkCtx::switch_case(place);
                self.walk(&mut statements, &tree, &ctx);
            }
            Decision::Success {
                arm_index,
                bindings,
            } => {
                self.render_single_catchall(&mut statements, *arm_index, bindings, place);
            }
            _ if self.arms.iter().any(|arm| arm.has_guard()) => {
                self.render_retry_loop(&mut statements, &tree, place);
            }
            _ => {
                self.render_chain_root(&mut statements, &tree, place);
            }
        }
        LoweredBlock { statements }
    }

    fn record_subject_use(&mut self) {
        for name in self.subject.names().to_vec() {
            self.planner.scope.record_go_use(name);
        }
    }

    fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.planner.enter_scope();
        let result = f(self);
        self.planner.exit_scope();
        result
    }

    fn with_optional_scope<R>(&mut self, scoped: bool, f: impl FnOnce(&mut Self) -> R) -> R {
        if scoped { self.with_scope(f) } else { f(self) }
    }

    fn render_single_catchall(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        arm_index: usize,
        bindings: &[PatternBinding],
        place: &PlacePlan,
    ) {
        let pattern_has_collisions = self
            .planner
            .pattern_has_binding_collisions(&self.arms[arm_index].pattern);
        let arm_body = &*self.arms[arm_index].expression;

        let (inner, needs_block) = self.with_scope(|this| {
            let mut inner: Vec<LoweredStatement> = Vec::new();
            this.with_bindings(&mut inner, bindings, &[arm_body], None, |this, inner| {
                this.emit_arm_body(inner, arm_index, place)
            });
            let needs_block =
                this.planner.scope.current_block_declared_nonempty() || pattern_has_collisions;
            (inner, needs_block)
        });

        if needs_block {
            statements.push(LoweredStatement::Block(LoweredBlock { statements: inner }));
        } else {
            statements.extend(inner);
        }
    }

    fn render_chain_root(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        tree: &Decision,
        place: &PlacePlan,
    ) {
        let chain_tail_is_exhaustive = decision_is_exhaustive(tree)
            || self
                .arms
                .last()
                .is_some_and(|arm| !arm.has_guard() && is_unconditional_catchall(&arm.pattern));
        self.emit_chain_root_decision(statements, tree, place);
        if let Some(panic) = unreachable_panic_if_needed(place, chain_tail_is_exhaustive) {
            statements.push(panic);
        }
    }

    fn emit_chain_root_decision(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        tree: &Decision,
        place: &PlacePlan,
    ) {
        match tree {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                let arm_body = &*self.arms[*arm_index].expression;
                self.with_bindings(
                    statements,
                    bindings,
                    &[arm_body],
                    None,
                    |this, statements| this.emit_arm_body(statements, *arm_index, place),
                );
            }
            Decision::Chain { tests, fallback } => {
                self.lower_chain_branch(statements, tests, fallback, place);
            }
            Decision::Unreachable => {}
            Decision::Guard { .. } => {
                self.walk(statements, tree, &WalkCtx::chain_test(place));
            }
            Decision::Switch { .. } => {
                self.walk(statements, tree, &WalkCtx::switch_case(place));
            }
        }
    }

    fn lower_chain_branch(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        tests: &[ChainTest],
        fallback: &Decision,
        place: &PlacePlan,
    ) {
        let last_is_catchall = chain_last_is_catchall(tests, fallback);
        let conditions = self.render_chain_conditions(tests);
        let regular_len = if last_is_catchall {
            tests.len() - 1
        } else {
            tests.len()
        };

        let guard_ctx = WalkCtx::switch_case(place);
        let chain_ctx = WalkCtx::chain_test(place);

        // Lower each branch body in its own scope, recording the last branch's
        // divergence so the trailing else decides else/flat structurally.
        let mut branches: Vec<ChainBranch> = Vec::with_capacity(regular_len);
        let mut last_diverges = false;
        for (test, condition) in tests[..regular_len].iter().zip(&conditions) {
            if condition.is_some() {
                self.record_subject_use();
            }
            let condition = condition.as_deref().unwrap_or("true").to_string();
            let walk_ctx = if matches!(test.decision, Decision::Guard { .. }) {
                &guard_ctx
            } else {
                &chain_ctx
            };
            let body = self.with_scope(|this| {
                let mut body: Vec<LoweredStatement> = Vec::new();
                this.walk(&mut body, &test.decision, walk_ctx);
                body
            });
            let body = LoweredBlock { statements: body };
            last_diverges = body.ends_with_diverge();
            branches.push(ChainBranch { condition, body });
        }

        let trailing = if last_is_catchall {
            let last_test = tests.last().unwrap();
            self.lower_else_or_flat(&last_test.decision, &chain_ctx, last_diverges)
        } else if matches!(fallback, Decision::Unreachable) {
            ElseArm::None
        } else {
            self.lower_else_or_flat(fallback, &chain_ctx, last_diverges)
        };

        if branches.is_empty() {
            // No regular branches: emit the catchall/fallback directly.
            match trailing {
                ElseArm::Else { body, .. } => statements.extend(body.statements),
                ElseArm::ElseIf(plan) => statements.push(LoweredStatement::If(*plan)),
                ElseArm::None => {}
            }
            return;
        }
        statements.push(LoweredStatement::If(build_chain_plan(branches, trailing)));
    }

    fn render_retry_loop(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
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
                statements.push(LoweredStatement::UnreachablePanic);
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
        let mut body: Vec<LoweredStatement> = Vec::new();
        self.walk(&mut body, tree, &ctx);
        if !unguarded_exit {
            body.push(LoweredStatement::Break(LoopTransfer::Labeled(
                label.clone(),
            )));
        }
        statements.push(LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            kind: LoopKind::Generated { label: Some(label) },
            header: "for {\n".to_string(),
            body: LoweredBlock { statements: body },
        }));
    }

    fn guarded_tree_flattens(&self, tree: &Decision) -> bool {
        match tree {
            Decision::Success { .. } | Decision::Unreachable => true,
            Decision::Guard {
                arm_index,
                bindings,
                success,
                failure,
            } => {
                self.arms[*arm_index]
                    .guard
                    .as_deref()
                    .is_some_and(guard_renders_inline)
                    && bindings
                        .iter()
                        .all(|binding| !binding.path.contains_deferred_evaluation())
                    && self.guarded_tree_flattens(success)
                    && self.guarded_tree_flattens(failure)
            }
            Decision::Chain { tests, fallback } => {
                tests
                    .iter()
                    .all(|test| self.guarded_tree_flattens(&test.decision))
                    && self.guarded_tree_flattens(fallback)
            }
            Decision::Switch {
                shape: SwitchShape::TypeSwitch,
                ..
            } => false,
            Decision::Switch {
                branches, fallback, ..
            } => {
                branches
                    .iter()
                    .all(|branch| self.guarded_tree_flattens(&branch.decision))
                    && fallback
                        .as_deref()
                        .is_none_or(|fallback| self.guarded_tree_flattens(fallback))
            }
        }
    }

    fn render_conditional_switch(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
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
                labels: case.conditions.join(" && "),
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

        let postlude = switch_postlude(place, has_default);
        statements.push(LoweredStatement::Switch(SwitchStatementPlan {
            kind: SwitchKind::Conditional,
            cases,
            default,
            postlude,
        }));
        true
    }

    fn collect_flat_cases<'d>(
        &mut self,
        decision: &'d Decision,
        conditions: &mut Vec<String>,
        out: &mut Vec<FlatCase<'d>>,
        tail: bool,
    ) -> bool {
        match decision {
            Decision::Unreachable => true,
            Decision::Success { .. } => {
                out.push(FlatCase {
                    conditions: conditions.clone(),
                    bindings: &[],
                    decision,
                });
                true
            }
            Decision::Guard {
                arm_index,
                bindings,
                success,
                failure,
            } => {
                let Some(condition) = self.guard_condition_over_paths(*arm_index, bindings) else {
                    return false;
                };
                conditions.push(condition);
                out.push(FlatCase {
                    conditions: conditions.clone(),
                    bindings,
                    decision: success,
                });
                conditions.pop();
                if !tail {
                    return true;
                }
                self.collect_flat_cases(failure, conditions, out, tail)
            }
            Decision::Chain { tests, fallback } => {
                let (cased, lifted) = split_chain_with_catchall_lift(tests, fallback);
                for test in cased {
                    if test.checks.is_empty() {
                        if !self.collect_flat_cases(&test.decision, conditions, out, false) {
                            return false;
                        }
                        continue;
                    }
                    self.record_subject_use();
                    conditions.push(render_condition(&test.checks, self.subject.root()));
                    let flattened = self.collect_flat_cases(&test.decision, conditions, out, false);
                    conditions.pop();
                    if !flattened {
                        return false;
                    }
                }
                match lifted {
                    Some(lifted) => self.collect_flat_cases(lifted, conditions, out, tail),
                    None => self.collect_flat_cases(fallback, conditions, out, tail),
                }
            }
            Decision::Switch {
                path,
                kind,
                shape,
                branches,
                fallback,
            } => {
                let rendered_path = path.render(self.subject.root());
                let (cased, lifted) = split_with_default_lift(branches, fallback.as_deref());
                for branch in cased {
                    self.record_subject_use();
                    conditions.push(switch_branch_condition(
                        &rendered_path,
                        kind,
                        shape,
                        &branch.case_label,
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

    fn guard_condition_over_paths(
        &mut self,
        arm_index: usize,
        bindings: &[PatternBinding],
    ) -> Option<String> {
        let overlays = self.install_path_overlays(bindings);
        let lowered = self.lower_guard_condition(arm_index);
        drop_inline_overlays(self.planner, &overlays);
        let (setup, condition) = lowered?;
        if !setup.is_empty() {
            return None;
        }
        let guard = self.arms[arm_index]
            .guard
            .as_deref()
            .expect("a Guard decision has a guard expression");
        Some(if binds_looser_than_conjunction(guard) {
            format!("({condition})")
        } else {
            condition
        })
    }

    fn install_path_overlays(
        &mut self,
        bindings: &[PatternBinding],
    ) -> Vec<(String, Option<BindingValue>)> {
        let mut overlays = Vec::with_capacity(bindings.len());
        for binding in bindings {
            if binding.go_name.is_none() {
                continue;
            }
            let previous = self
                .planner
                .scope
                .resolve_identifier_binding(&binding.lisette_name)
                .cloned();
            let text = binding.path.render_composable(self.subject.root());
            self.planner.scope.bind_inline_expr(
                &binding.lisette_name,
                InlineExpr::new(
                    text,
                    vec![self.subject.var().to_string()],
                    binding.path.contains_deferred_evaluation(),
                ),
            );
            overlays.push((binding.lisette_name.clone(), previous));
        }
        overlays
    }

    fn lower_flat_case_body(&mut self, case: &FlatCase, place: &PlacePlan) -> LoweredBlock {
        let ctx = WalkCtx::switch_case(place);
        self.with_scope(|this| {
            let mut body: Vec<LoweredStatement> = Vec::new();
            if case.bindings.is_empty() {
                this.walk(&mut body, case.decision, &ctx);
                return LoweredBlock { statements: body };
            }
            let arm_body = &*this.arms[decision_arm_index(case.decision)].expression;
            let bindings: Vec<PatternBinding> = case
                .bindings
                .iter()
                .filter(|binding| {
                    analyze_inline_candidate(&binding.lisette_name, &[arm_body])
                        != InlineDecision::Unused
                })
                .cloned()
                .collect();
            this.with_bindings(&mut body, &bindings, &[arm_body], None, |this, body| {
                this.walk(body, case.decision, &ctx);
            });
            LoweredBlock { statements: body }
        })
    }

    fn walk(&mut self, statements: &mut Vec<LoweredStatement>, decision: &Decision, ctx: &WalkCtx) {
        match decision {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                let wrap = ctx.leaf_scope_explicit();
                let arm_body = &*self.arms[*arm_index].expression;
                let leaf = self.with_optional_scope(wrap, |this| {
                    let mut leaf: Vec<LoweredStatement> = Vec::new();
                    this.with_bindings(&mut leaf, bindings, &[arm_body], None, |this, leaf| {
                        let mut body_statements: Vec<LoweredStatement> = Vec::new();
                        this.emit_arm_body(&mut body_statements, *arm_index, ctx.arm_place);
                        let body_diverges = capture_diverge(body_statements, leaf);
                        apply_leaf_terminator(leaf, ctx, body_diverges);
                    });
                    leaf
                });
                if wrap {
                    statements.push(LoweredStatement::Block(LoweredBlock { statements: leaf }));
                } else {
                    statements.extend(leaf);
                }
            }
            Decision::Guard { .. } => self.walk_guard(statements, decision, ctx),
            Decision::Switch { .. } => self.walk_switch(statements, decision, ctx),
            Decision::Chain { tests, fallback } => {
                if ctx.is_grouped_retry() {
                    self.emit_chain_grouped(statements, tests, fallback, ctx);
                } else {
                    self.lower_chain_branch(statements, tests, fallback, ctx.arm_place);
                }
            }
            Decision::Unreachable => {}
        }
    }

    fn walk_switch(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        decision: &Decision,
        ctx: &WalkCtx,
    ) {
        let Decision::Switch {
            path,
            kind,
            shape,
            branches,
            fallback,
        } = decision
        else {
            unreachable!("walk_switch requires a Switch decision");
        };
        let fallback = fallback.as_deref();
        let rendered_path = path.render(self.subject.root());
        match shape {
            SwitchShape::TypeSwitch => {
                self.record_subject_use();
                let plan = self.lower_type_switch(rendered_path, branches, fallback, ctx.arm_place);
                let body_diverges =
                    capture_diverge(vec![LoweredStatement::Switch(plan)], statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
            SwitchShape::Bool => {
                let true_branch = branches
                    .iter()
                    .find(|branch| branch.case_label == "true")
                    .expect("Bool shape requires a true-labeled branch");
                let false_branch = branches
                    .iter()
                    .find(|branch| branch.case_label == "false")
                    .expect("Bool shape requires a false-labeled branch");
                self.walk_condition_branch(
                    statements,
                    wrap_if_struct_literal(rendered_path),
                    &true_branch.decision,
                    &false_branch.decision,
                    ctx,
                );
            }
            SwitchShape::Binary => {
                let condition = format!(
                    "{} == {}",
                    render_switch_expression(&rendered_path, kind),
                    branches[0].case_label
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
                    let mut branch_statements: Vec<LoweredStatement> = Vec::new();
                    self.walk(&mut branch_statements, &branch.decision, &inner);
                    let body_diverges = capture_diverge(branch_statements, statements);
                    apply_leaf_terminator(statements, ctx, body_diverges);
                    return;
                };
                let condition = format!(
                    "{} == {}",
                    render_switch_expression(&rendered_path, kind),
                    branch.case_label
                );
                self.walk_condition_branch(statements, condition, &branch.decision, fallback, ctx);
            }
            SwitchShape::Multi => {
                self.record_subject_use();
                let expr = render_switch_expression(&rendered_path, kind);
                let plan = self.lower_value_switch(expr, branches, fallback, ctx.arm_place);
                let body_diverges =
                    capture_diverge(vec![LoweredStatement::Switch(plan)], statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
        }
    }

    fn walk_condition_branch(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        condition: String,
        then_branch: &Decision,
        else_branch: &Decision,
        ctx: &WalkCtx,
    ) {
        self.record_subject_use();
        let inner = WalkCtx::switch_case(ctx.arm_place);
        let then_statements = self.with_scope(|this| {
            this.planner.scope.establish_condition(condition.clone());
            let mut then_statements: Vec<LoweredStatement> = Vec::new();
            this.walk(&mut then_statements, then_branch, &inner);
            then_statements
        });
        let then_body = LoweredBlock {
            statements: then_statements,
        };
        let then_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_or_flat(else_branch, &inner, then_diverges);
        let plan = IfPlan {
            condition_setup: Vec::new(),
            condition,
            then_body,
            else_arm,
        };
        let body_diverges = capture_diverge(vec![LoweredStatement::If(plan)], statements);
        apply_leaf_terminator(statements, ctx, body_diverges);
    }

    fn walk_guard(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        decision: &Decision,
        ctx: &WalkCtx,
    ) {
        let Decision::Guard {
            arm_index,
            bindings,
            success,
            failure,
        } = decision
        else {
            unreachable!("walk_guard requires a Guard decision");
        };
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
            let mut guard_statements: Vec<LoweredStatement> = Vec::new();
            let guarded = this.with_bindings(
                &mut guard_statements,
                bindings,
                &guard_consumers,
                Some(failure),
                |this, _| {
                    let (condition_setup, condition) = this.lower_guard_condition(arm_index)?;
                    let then_body = this.with_scope(|this| {
                        let mut success_statements: Vec<LoweredStatement> = Vec::new();
                        this.walk(&mut success_statements, success, &ctx.nested());
                        LoweredBlock {
                            statements: success_statements,
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
                guard_statements.push(LoweredStatement::If(IfPlan {
                    condition_setup,
                    condition,
                    then_body,
                    else_arm,
                }));
            }
            guard_statements
        });
        if needs_pre_scope {
            statements.push(LoweredStatement::Block(LoweredBlock {
                statements: guard_statements,
            }));
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
        if self.is_empty_leaf(decision) {
            return ElseArm::None;
        }
        if preceding_diverges {
            let mut body: Vec<LoweredStatement> = Vec::new();
            self.walk(&mut body, decision, ctx);
            return ElseArm::from_body(LoweredBlock { statements: body }, true);
        }
        let body = self.with_scope(|this| {
            let mut body: Vec<LoweredStatement> = Vec::new();
            this.walk(&mut body, decision, ctx);
            body
        });
        ElseArm::from_body(LoweredBlock { statements: body }, false)
    }

    fn is_empty_leaf(&self, decision: &Decision) -> bool {
        match decision {
            Decision::Success {
                arm_index,
                bindings,
            } => bindings.is_empty() && body_is_unit_or_empty(&self.arms[*arm_index].expression),
            _ => false,
        }
    }

    fn lower_value_switch(
        &mut self,
        expr: String,
        branches: &[SwitchBranch],
        fallback: Option<&Decision>,
        place: &PlacePlan,
    ) -> SwitchStatementPlan {
        let (regular, default) = split_with_default_lift(branches, fallback);
        let case_plans = self.lower_switch_cases(regular, place, Some(&expr));
        let default_block = self.lower_switch_default(default, place);
        SwitchStatementPlan {
            kind: SwitchKind::Value { subject: expr },
            cases: case_plans,
            default: default_block,
            postlude: switch_postlude(place, default.is_some()),
        }
    }

    fn lower_type_switch(
        &mut self,
        base: String,
        branches: &[SwitchBranch],
        fallback: Option<&Decision>,
        place: &PlacePlan,
    ) -> SwitchStatementPlan {
        let (regular, default) = split_with_default_lift(branches, fallback);
        let arms = self.arms;
        let subject_ty = self.subject_ty.clone();
        let ((case_plans, default_block), used) = self.planner.capture_go_uses(|planner| {
            let mut nested =
                TreePlanner::new(planner, arms, MatchSubject::Var(base.clone()), subject_ty);
            let case_plans = nested.lower_switch_cases(regular, place, None);
            let default_block = nested.lower_switch_default(default, place);
            (case_plans, default_block)
        });

        // Keep the `base :=` type-switch binding only when a case references it;
        // Go rejects an unused `:= base` assignment otherwise.
        let references_base = used.contains(&base);
        let binding = references_base.then(|| base.clone());

        SwitchStatementPlan {
            kind: SwitchKind::Type {
                subject: base,
                binding,
            },
            cases: case_plans,
            default: default_block,
            postlude: switch_postlude(place, default.is_some()),
        }
    }

    fn lower_switch_cases(
        &mut self,
        branches: &[SwitchBranch],
        place: &PlacePlan,
        subject: Option<&str>,
    ) -> Vec<SwitchCasePlan> {
        let ctx = WalkCtx::switch_case(place);
        let mut case_plans = Vec::with_capacity(branches.len());
        for branch in branches {
            // A case listing several labels establishes none of them on its own.
            let established = subject
                .filter(|_| !branch.case_label.contains(','))
                .map(|subject| format!("{} == {}", subject, branch.case_label));
            let body = self.with_scope(|this| {
                if let Some(condition) = established {
                    this.planner.scope.establish_condition(condition);
                }
                let mut body: Vec<LoweredStatement> = Vec::new();
                this.walk(&mut body, &branch.decision, &ctx);
                body
            });
            case_plans.push(SwitchCasePlan {
                labels: branch.case_label.clone(),
                body: LoweredBlock { statements: body },
            });
        }
        case_plans
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
            let mut body: Vec<LoweredStatement> = Vec::new();
            this.walk(&mut body, default_decision, &ctx);
            body
        });
        (!body.is_empty()).then_some(LoweredBlock { statements: body })
    }

    fn emit_chain_grouped(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        tests: &[ChainTest],
        fallback: &Decision,
        ctx: &WalkCtx,
    ) {
        let last_is_catchall = chain_last_is_catchall(tests, fallback);
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

        if !matches!(fallback, Decision::Unreachable) {
            self.walk(statements, fallback, ctx);
        }
    }

    fn emit_chain_group(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
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
            let mut body: Vec<LoweredStatement> = Vec::new();
            this.emit_chain_group_tests(&mut body, indices, tests, ctx);
            body
        });

        let body = LoweredBlock { statements: body };
        let first_condition = &conditions[indices[0]];
        if first_condition.is_some() {
            self.record_subject_use();
        }
        match first_condition {
            Some(condition) => statements.push(LoweredStatement::If(IfPlan {
                condition_setup: Vec::new(),
                condition: condition.clone(),
                then_body: body,
                else_arm: ElseArm::None,
            })),
            None => statements.push(LoweredStatement::Block(body)),
        }
    }

    fn emit_chain_group_tests(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
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
        statements: &mut Vec<LoweredStatement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        if let Some(&ref_index) = indices
            .iter()
            .find(|&&index| !decision_top_bindings(&tests[index].decision).is_empty())
        {
            let mut consumers: Vec<&Expression> = Vec::new();
            for &index in indices {
                let decision = &tests[index].decision;
                let arm_index = match decision {
                    Decision::Success { arm_index, .. } | Decision::Guard { arm_index, .. } => {
                        Some(*arm_index)
                    }
                    _ => None,
                };
                if let Some(arm_index) = arm_index {
                    let arm = &self.arms[arm_index];
                    if let Some(guard) = arm.guard.as_deref() {
                        consumers.push(guard);
                    }
                    consumers.push(&arm.expression);
                }
            }
            self.with_bindings(
                statements,
                decision_top_bindings(&tests[ref_index].decision),
                &consumers,
                None,
                |this, statements| {
                    this.emit_chain_group_bodies(statements, indices, tests, ctx);
                },
            );
        } else {
            self.emit_chain_group_bodies(statements, indices, tests, ctx);
        }
    }

    fn emit_chain_group_bodies(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        for &test_index in indices {
            match &tests[test_index].decision {
                Decision::Success { arm_index, .. } => {
                    let mut body_statements: Vec<LoweredStatement> = Vec::new();
                    self.emit_arm_body(&mut body_statements, *arm_index, ctx.arm_place);
                    let body_diverges = capture_diverge(body_statements, statements);
                    apply_leaf_terminator(statements, ctx, body_diverges);
                }
                Decision::Guard { arm_index, .. } => {
                    if let Some((condition_setup, condition)) =
                        self.lower_guard_condition(*arm_index)
                    {
                        let then_body = self.with_scope(|this| {
                            let mut arm_body: Vec<LoweredStatement> = Vec::new();
                            this.emit_arm_body(&mut arm_body, *arm_index, ctx.arm_place);
                            let mut then_body: Vec<LoweredStatement> = Vec::new();
                            let body_diverges = capture_diverge(arm_body, &mut then_body);
                            apply_leaf_terminator(&mut then_body, ctx, body_diverges);
                            then_body
                        });
                        statements.push(LoweredStatement::If(IfPlan {
                            condition_setup,
                            condition,
                            then_body: LoweredBlock {
                                statements: then_body,
                            },
                            else_arm: ElseArm::None,
                        }));
                    }
                }
                _ => self.walk(statements, &tests[test_index].decision, ctx),
            }
        }
    }

    fn emit_chain_group_per_test(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        for (j, &test_index) in indices.iter().enumerate() {
            let is_last_in_group = j == indices.len() - 1;
            let needs_wrapper =
                !is_last_in_group && !decision_top_bindings(&tests[test_index].decision).is_empty();
            if needs_wrapper {
                let wrapped = self.with_scope(|this| {
                    let mut wrapped: Vec<LoweredStatement> = Vec::new();
                    this.walk(&mut wrapped, &tests[test_index].decision, ctx);
                    wrapped
                });
                statements.push(LoweredStatement::Block(LoweredBlock {
                    statements: wrapped,
                }));
            } else {
                self.walk(statements, &tests[test_index].decision, ctx);
            }
        }
    }

    /// Returns the overlay pairs installed for inline substitutions; pass to
    /// `drop_inline_overlays` to roll them back.
    fn emit_bindings(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        bindings: &[PatternBinding],
        consumers: &[&Expression],
        failure_blocker: Option<&Decision>,
    ) -> Vec<(String, Option<BindingValue>)> {
        let failure_trees: Vec<&Expression> = match failure_blocker {
            Some(failure) => {
                let mut reached: Vec<usize> = Vec::new();
                collect_reachable_arms(failure, &mut reached);
                let mut trees: Vec<&Expression> = Vec::with_capacity(reached.len() * 2);
                for index in reached {
                    let arm = &self.arms[index];
                    if let Some(guard) = arm.guard.as_ref() {
                        trees.push(guard);
                    }
                    trees.push(&arm.expression);
                }
                trees
            }
            None => Vec::new(),
        };

        if bindings.is_empty() {
            return Vec::new();
        }
        tree_binding_statements(
            self.planner,
            statements,
            bindings,
            self.subject.var(),
            consumers,
            &failure_trees,
        )
    }

    fn with_bindings<R>(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        bindings: &[PatternBinding],
        consumers: &[&Expression],
        failure_blocker: Option<&Decision>,
        f: impl FnOnce(&mut Self, &mut Vec<LoweredStatement>) -> R,
    ) -> R {
        let overlays = self.emit_bindings(statements, bindings, consumers, failure_blocker);
        let result = f(self, statements);
        drop_inline_overlays(self.planner, &overlays);
        result
    }

    fn emit_arm_body(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        arm_index: usize,
        place: &PlacePlan,
    ) {
        let arm = &self.arms[arm_index];
        let block = self.planner.lower_block_to_place(&arm.expression, place);
        statements.extend(block.statements);
    }

    /// Lower an arm's guard to `(condition_setup, condition)` for an `IfPlan`,
    /// or `None` when the arm has no guard. The caller owns the scope and body.
    fn lower_guard_condition(
        &mut self,
        arm_index: usize,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let guard_expression = self.arms[arm_index].guard.as_deref()?;
        let plan = self
            .planner
            .plan_operand(guard_expression, ExpressionContext::value().condition());
        let (setup, value) = plan.into_parts();
        Some((setup, wrap_if_struct_literal(value)))
    }

    fn render_chain_conditions(&self, tests: &[ChainTest]) -> Vec<Option<String>> {
        tests
            .iter()
            .map(|test| {
                (!test.checks.is_empty())
                    .then(|| render_condition(&test.checks, self.subject.root()))
            })
            .collect()
    }
}

fn chain_last_is_catchall(tests: &[ChainTest], fallback: &Decision) -> bool {
    matches!(fallback, Decision::Unreachable) && tests.len() > 1
}

fn split_chain_with_catchall_lift<'t>(
    tests: &'t [ChainTest],
    fallback: &Decision,
) -> (&'t [ChainTest], Option<&'t Decision>) {
    if !chain_last_is_catchall(tests, fallback) {
        return (tests, None);
    }
    match tests.split_last() {
        Some((last, rest)) if !matches!(last.decision, Decision::Guard { .. }) => {
            (rest, Some(&last.decision))
        }
        _ => (tests, None),
    }
}

fn split_with_default_lift<'t>(
    branches: &'t [SwitchBranch],
    fallback: Option<&'t Decision>,
) -> (&'t [SwitchBranch], Option<&'t Decision>) {
    match (fallback, branches.split_last()) {
        (None, Some((last, rest))) => (rest, Some(&last.decision)),
        _ => (branches, fallback),
    }
}

fn switch_branch_condition(
    rendered_path: &str,
    kind: &PatternSwitchKind,
    shape: &SwitchShape,
    case_label: &str,
) -> String {
    if matches!(shape, SwitchShape::Bool) && case_label == "true" {
        return wrap_if_struct_literal(rendered_path.to_string());
    }
    format!(
        "{} == {}",
        render_switch_expression(rendered_path, kind),
        case_label
    )
}

fn render_switch_expression(rendered_path: &str, kind: &PatternSwitchKind) -> String {
    match kind {
        PatternSwitchKind::EnumTag => wrap_if_struct_literal(format!("{}.Tag", rendered_path)),
        PatternSwitchKind::Value => wrap_if_struct_literal(rendered_path.to_string()),
        PatternSwitchKind::TypeSwitch => unreachable!("TypeSwitch handled separately"),
    }
}

fn body_is_unit_or_empty(expression: &Expression) -> bool {
    matches!(expression, Expression::Unit { .. })
        || matches!(expression, Expression::Block { items, .. } if items.is_empty())
}

fn decision_top_bindings(decision: &Decision) -> &[PatternBinding] {
    match decision {
        Decision::Guard { bindings, .. } | Decision::Success { bindings, .. } => bindings,
        _ => &[],
    }
}

fn collect_reachable_arms(decision: &Decision, out: &mut Vec<usize>) {
    match decision {
        Decision::Success { arm_index, .. } => {
            if !out.contains(arm_index) {
                out.push(*arm_index);
            }
        }
        Decision::Guard {
            arm_index,
            success,
            failure,
            ..
        } => {
            if !out.contains(arm_index) {
                out.push(*arm_index);
            }
            collect_reachable_arms(success, out);
            collect_reachable_arms(failure, out);
        }
        Decision::Switch {
            branches, fallback, ..
        } => {
            for branch in branches {
                collect_reachable_arms(&branch.decision, out);
            }
            if let Some(fallback) = fallback.as_deref() {
                collect_reachable_arms(fallback, out);
            }
        }
        Decision::Chain { tests, fallback } => {
            for test in tests {
                collect_reachable_arms(&test.decision, out);
            }
            collect_reachable_arms(fallback, out);
        }
        Decision::Unreachable => {}
    }
}

fn bindings_are_hoistable(tests: &[ChainTest], indices: &[usize]) -> bool {
    if indices.len() <= 1 {
        return false;
    }
    let reference = indices.iter().find_map(|&index| {
        let bindings = decision_top_bindings(&tests[index].decision);
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
        let bindings = decision_top_bindings(&tests[index].decision);
        bindings.is_empty()
            || (bindings.len() == reference.len()
                && bindings
                    .iter()
                    .zip(reference.iter())
                    .all(|(binding, reference_binding)| {
                        binding.lisette_name == reference_binding.lisette_name
                            && binding.go_name == reference_binding.go_name
                            && binding.path == reference_binding.path
                    }))
    })
}

fn group_chain_tests_by_condition(conditions: &[Option<String>]) -> Vec<(&str, Vec<usize>)> {
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (i, condition) in conditions.iter().enumerate() {
        let key = condition.as_deref().unwrap_or("");
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
    condition: String,
    body: LoweredBlock,
}

/// Assemble pattern-chain branches into a nested `if`/`else if` plan, with
/// `trailing` as the innermost `else` arm. `branches` must be non-empty.
fn build_chain_plan(branches: Vec<ChainBranch>, trailing: ElseArm) -> IfPlan {
    let mut branches = branches;
    let head = branches.remove(0);
    let mut else_arm = trailing;
    for branch in branches.into_iter().rev() {
        else_arm = ElseArm::ElseIf(Box::new(IfPlan {
            condition_setup: Vec::new(),
            condition: branch.condition,
            then_body: branch.body,
            else_arm,
        }));
    }
    IfPlan {
        condition_setup: Vec::new(),
        condition: head.condition,
        then_body: head.body,
        else_arm,
    }
}

/// Build the post-switch unreachable panic (when the place requires a tail
/// return and the switch is non-exhaustive), as a `RawGo` postlude.
fn switch_postlude(place: &PlacePlan, has_default: bool) -> Vec<LoweredStatement> {
    unreachable_panic_if_needed(place, has_default)
        .into_iter()
        .collect()
}

/// Compute `ends_with_diverge` of `body_statements`, then move them into `statements`.
fn capture_diverge(
    body_statements: Vec<LoweredStatement>,
    statements: &mut Vec<LoweredStatement>,
) -> bool {
    let block = LoweredBlock {
        statements: body_statements,
    };
    let diverges = block.ends_with_diverge();
    statements.extend(block.statements);
    diverges
}

fn apply_leaf_terminator(
    statements: &mut Vec<LoweredStatement>,
    ctx: &WalkCtx,
    body_diverges: bool,
) {
    if let Some(label) = ctx.break_label
        && !body_diverges
    {
        statements.push(LoweredStatement::Break(LoopTransfer::Labeled(
            label.to_string(),
        )));
    }
}
