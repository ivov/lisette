use syntax::ast::{Expression, MatchArm};
use syntax::types::Type;

use crate::Planner;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate, region_blocks_inline};
use crate::context::expression::ExpressionContext;
use crate::patterns::binding_decls::{is_catchall_pattern, is_unconditional_catchall};
use crate::patterns::binding_emit::drop_inline_overlays;
use crate::patterns::decision_tree::{
    ChainTest, Decision, PatternBinding, SwitchBranch, SwitchKind as PatternSwitchKind,
    SwitchShape, compile_expanded_arms, decision_is_exhaustive, expand_or_patterns,
    render_condition, tree_has_unguarded_terminal,
};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopPlan, LoweredBlock, LoweredStatement, PlacePlan, SwitchCasePlan,
    SwitchKind, SwitchStatementPlan,
};
use crate::plan::placement::unreachable_panic_if_needed;
use crate::state::bindings::{BindingValue, InlineExpr};
use crate::utils::wrap_if_struct_literal;

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

pub(crate) struct TreePlanner<'a, 'e> {
    planner: &'a mut Planner<'e>,
    arms: &'a [MatchArm],
    current_subject: String,
    subject_ty: Type,
}

impl<'a, 'e> TreePlanner<'a, 'e> {
    pub(crate) fn new(
        planner: &'a mut Planner<'e>,
        arms: &'a [MatchArm],
        subject_var: String,
        subject_ty: Type,
    ) -> Self {
        Self {
            planner,
            arms,
            current_subject: subject_var,
            subject_ty,
        }
    }

    pub(crate) fn lower(mut self, place: &PlacePlan) -> LoweredBlock {
        let expanded = expand_or_patterns(self.arms);
        let compiled = compile_expanded_arms(self.planner, &expanded, &self.subject_ty);
        self.planner.absorb_effects(&compiled.effects);
        let tree = compiled.decision;
        if decision_needs_stdlib(&tree) {
            self.planner.require_stdlib();
        }

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

    fn with_subject<R>(&mut self, subject: String, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = std::mem::replace(&mut self.current_subject, subject);
        let result = f(self);
        self.current_subject = previous;
        result
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

        self.planner.enter_scope();
        let mut inner: Vec<LoweredStatement> = Vec::new();
        let inlines = self.emit_bindings(&mut inner, bindings, &[arm_body], None);
        self.emit_arm_body(&mut inner, arm_index, place);
        drop_inline_overlays(self.planner, &inlines);
        let needs_block =
            self.planner.scope.current_block_declared_nonempty() || pattern_has_collisions;
        self.planner.exit_scope();

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
                let inlines = self.emit_bindings(statements, bindings, &[arm_body], None);
                self.emit_arm_body(statements, *arm_index, place);
                drop_inline_overlays(self.planner, &inlines);
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
                self.planner.scope.record_go_use(&self.current_subject);
            }
            let condition = condition.as_deref().unwrap_or("true").to_string();
            let walk_ctx = if matches!(test.decision, Decision::Guard { .. }) {
                &guard_ctx
            } else {
                &chain_ctx
            };
            self.planner.enter_scope();
            let mut body: Vec<LoweredStatement> = Vec::new();
            self.walk(&mut body, &test.decision, walk_ctx);
            self.planner.exit_scope();
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

        // Wrap the tree in a labeled `for { ... }` retry loop.
        let label = self.planner.fresh_var(Some("match"));
        let ctx = WalkCtx::retry_loop(place, Some(label.as_str()));
        let mut body: Vec<LoweredStatement> = Vec::new();
        self.walk(&mut body, tree, &ctx);
        if !unguarded_exit {
            body.push(LoweredStatement::Break {
                label: Some(label.clone()),
            });
        }
        statements.push(LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            label: Some(label),
            header: "for {\n".to_string(),
            body: LoweredBlock { statements: body },
        }));
    }

    fn walk(&mut self, statements: &mut Vec<LoweredStatement>, decision: &Decision, ctx: &WalkCtx) {
        match decision {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                let wrap = ctx.leaf_scope_explicit();
                let arm_body = &*self.arms[*arm_index].expression;
                if wrap {
                    self.planner.enter_scope();
                }
                let mut leaf: Vec<LoweredStatement> = Vec::new();
                let inlines = self.emit_bindings(&mut leaf, bindings, &[arm_body], None);
                let mut body_statements: Vec<LoweredStatement> = Vec::new();
                self.emit_arm_body(&mut body_statements, *arm_index, ctx.arm_place);
                let body_diverges = capture_diverge(body_statements, &mut leaf);
                apply_leaf_terminator(&mut leaf, ctx, body_diverges);
                drop_inline_overlays(self.planner, &inlines);
                if wrap {
                    self.planner.exit_scope();
                    statements.push(LoweredStatement::Block(LoweredBlock { statements: leaf }));
                } else {
                    statements.extend(leaf);
                }
            }
            Decision::Guard {
                arm_index,
                bindings,
                success,
                failure,
            } => self.walk_guard(statements, *arm_index, bindings, success, failure, ctx),
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
        let rendered_path = path.render(&self.current_subject);
        match shape {
            SwitchShape::TypeSwitch => {
                self.planner.scope.record_go_use(&self.current_subject);
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
                self.planner.scope.record_go_use(&self.current_subject);
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
        self.planner.scope.record_go_use(&self.current_subject);
        let inner = WalkCtx::switch_case(ctx.arm_place);
        self.planner.enter_scope();
        let mut then_statements: Vec<LoweredStatement> = Vec::new();
        self.walk(&mut then_statements, then_branch, &inner);
        self.planner.exit_scope();
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
        arm_index: usize,
        bindings: &[PatternBinding],
        success: &Decision,
        failure: &Decision,
        ctx: &WalkCtx,
    ) {
        let needs_pre_scope = ctx.leaf_scope_explicit() && !bindings.is_empty();
        if needs_pre_scope {
            self.planner.enter_scope();
        }
        let arm = &self.arms[arm_index];
        let arm_body = &*arm.expression;
        let mut guard_consumers: Vec<&Expression> = Vec::with_capacity(2);
        if let Some(guard) = arm.guard.as_deref() {
            guard_consumers.push(guard);
        }
        guard_consumers.push(arm_body);

        // Collect the bindings and the guard `if` into one block so a pre-scope
        // can wrap them as a single `LoweredStatement::Block`.
        let mut guard_statements: Vec<LoweredStatement> = Vec::new();
        let inlines = self.emit_bindings(
            &mut guard_statements,
            bindings,
            &guard_consumers,
            Some(failure),
        );
        if let Some((condition_setup, condition)) = self.lower_guard_condition(arm_index) {
            self.planner.enter_scope();
            let mut success_statements: Vec<LoweredStatement> = Vec::new();
            self.walk(&mut success_statements, success, &ctx.nested());
            let then_body = LoweredBlock {
                statements: success_statements,
            };
            let success_diverges = then_body.ends_with_diverge();
            self.planner.exit_scope();
            drop_inline_overlays(self.planner, &inlines);
            let else_arm = if ctx.role == WalkRole::SwitchCase {
                self.lower_else_or_flat(failure, ctx, success_diverges)
            } else {
                ElseArm::None
            };
            guard_statements.push(LoweredStatement::If(IfPlan {
                condition_setup,
                condition,
                then_body,
                else_arm,
            }));
        } else {
            drop_inline_overlays(self.planner, &inlines);
        }
        if needs_pre_scope {
            self.planner.exit_scope();
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
            return ElseArm::Else {
                body: LoweredBlock { statements: body },
                inline: true,
            };
        }
        self.planner.enter_scope();
        let mut body: Vec<LoweredStatement> = Vec::new();
        self.walk(&mut body, decision, ctx);
        self.planner.exit_scope();
        ElseArm::Else {
            body: LoweredBlock { statements: body },
            inline: false,
        }
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
        let case_plans = self.lower_switch_cases(regular, place);
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
        self.planner.scope.enter_use_region();
        let (case_plans, default_block) = self.with_subject(base.clone(), |tree_planner| {
            let case_plans = tree_planner.lower_switch_cases(regular, place);
            let default_block = tree_planner.lower_switch_default(default, place);
            (case_plans, default_block)
        });
        let used = self.planner.scope.exit_use_region();

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
    ) -> Vec<SwitchCasePlan> {
        let ctx = WalkCtx::switch_case(place);
        let mut case_plans = Vec::with_capacity(branches.len());
        for branch in branches {
            let mut body: Vec<LoweredStatement> = Vec::new();
            self.planner.enter_scope();
            self.walk(&mut body, &branch.decision, &ctx);
            self.planner.exit_scope();
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
        let mut body: Vec<LoweredStatement> = Vec::new();
        self.planner.enter_scope();
        self.walk(&mut body, default_decision, &ctx);
        self.planner.exit_scope();
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
            let collapse_as_catchall = is_last_group && last_is_catchall && indices.len() == 1;
            self.emit_chain_group(
                statements,
                indices,
                tests,
                &conditions,
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
        indices: &[usize],
        tests: &[ChainTest],
        conditions: &[Option<String>],
        ctx: &WalkCtx,
        collapse_as_catchall: bool,
    ) {
        if collapse_as_catchall {
            self.walk(statements, &tests[indices[0]].decision, ctx);
            return;
        }

        self.planner.enter_scope();
        let mut body: Vec<LoweredStatement> = Vec::new();
        if bindings_are_hoistable(tests, indices) {
            self.emit_chain_group_hoisted(&mut body, indices, tests, ctx);
        } else {
            self.emit_chain_group_per_test(&mut body, indices, tests, ctx);
        }
        self.planner.exit_scope();

        let body = LoweredBlock { statements: body };
        let first_condition = &conditions[indices[0]];
        if first_condition.is_some() {
            self.planner.scope.record_go_use(&self.current_subject);
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

    fn emit_chain_group_hoisted(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &WalkCtx,
    ) {
        let mut inlines: Vec<(String, Option<BindingValue>)> = Vec::new();
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
            inlines = self.emit_bindings(
                statements,
                decision_top_bindings(&tests[ref_index].decision),
                &consumers,
                None,
            );
        }
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
                        self.planner.enter_scope();
                        let mut arm_body: Vec<LoweredStatement> = Vec::new();
                        self.emit_arm_body(&mut arm_body, *arm_index, ctx.arm_place);
                        let mut then_body: Vec<LoweredStatement> = Vec::new();
                        let body_diverges = capture_diverge(arm_body, &mut then_body);
                        apply_leaf_terminator(&mut then_body, ctx, body_diverges);
                        self.planner.exit_scope();
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
        drop_inline_overlays(self.planner, &inlines);
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
                self.planner.enter_scope();
                let mut wrapped: Vec<LoweredStatement> = Vec::new();
                self.walk(&mut wrapped, &tests[test_index].decision, ctx);
                self.planner.exit_scope();
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

        let mut installed_inlines: Vec<(String, Option<BindingValue>)> = Vec::new();
        for binding in bindings {
            let Some(ref go_name) = binding.go_name else {
                self.planner.scope.bind(&binding.lisette_name, "");
                continue;
            };

            let previous = self
                .planner
                .scope
                .resolve_identifier_binding(&binding.lisette_name)
                .cloned();
            if self.try_inline_binding(binding, consumers, &failure_trees) {
                installed_inlines.push((binding.lisette_name.clone(), previous));
                continue;
            }

            let access_expression = binding.path.render(&self.current_subject);
            self.planner.scope.record_go_use(&self.current_subject);
            if self.planner.scope.has_binding_for_go_name(go_name) {
                let fresh = self.planner.fresh_var(Some(&binding.lisette_name));
                self.planner.scope.bind(&binding.lisette_name, &fresh);
                self.planner.try_declare(&fresh);
                statements.push(LoweredStatement::RawGo(format!(
                    "{} := {}\n",
                    fresh, access_expression
                )));
            } else {
                let name = self
                    .planner
                    .scope
                    .bind(&binding.lisette_name, go_name.clone());
                if self.planner.try_declare(&name) {
                    statements.push(LoweredStatement::RawGo(format!(
                        "{} := {}\n",
                        name, access_expression
                    )));
                } else {
                    let fresh = self.planner.fresh_var(Some(&binding.lisette_name));
                    self.planner.scope.bind(&binding.lisette_name, &fresh);
                    self.planner.try_declare(&fresh);
                    statements.push(LoweredStatement::RawGo(format!(
                        "{} := {}\n",
                        fresh, access_expression
                    )));
                }
            }
        }
        installed_inlines
    }

    fn try_inline_binding(
        &mut self,
        binding: &PatternBinding,
        consumers: &[&Expression],
        failure_trees: &[&Expression],
    ) -> bool {
        if consumers.is_empty() {
            return false;
        }
        if analyze_inline_candidate(&binding.lisette_name, consumers) != InlineDecision::Inline {
            return false;
        }
        if !failure_trees.is_empty()
            && region_blocks_inline(failure_trees.iter().copied(), &binding.lisette_name)
        {
            return false;
        }
        let composable_access = binding.path.render_composable(&self.current_subject);
        self.planner.scope.bind_inline_expr(
            &binding.lisette_name,
            InlineExpr::with_refs(composable_access, vec![self.current_subject.clone()]),
        );
        true
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
                    .then(|| render_condition(&test.checks, &self.current_subject))
            })
            .collect()
    }
}

fn chain_last_is_catchall(tests: &[ChainTest], fallback: &Decision) -> bool {
    matches!(fallback, Decision::Unreachable) && tests.len() > 1
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

fn render_switch_expression(rendered_path: &str, kind: &PatternSwitchKind) -> String {
    match kind {
        PatternSwitchKind::EnumTag => wrap_if_struct_literal(format!("{}.Tag", rendered_path)),
        PatternSwitchKind::Value => wrap_if_struct_literal(rendered_path.to_string()),
        PatternSwitchKind::TypeSwitch => unreachable!("TypeSwitch handled separately"),
    }
}

fn decision_needs_stdlib(decision: &Decision) -> bool {
    match decision {
        Decision::Success { .. } | Decision::Unreachable => false,
        Decision::Guard {
            success, failure, ..
        } => decision_needs_stdlib(success) || decision_needs_stdlib(failure),
        Decision::Switch {
            branches, fallback, ..
        } => {
            branches
                .iter()
                .any(|branch| branch.needs_stdlib || decision_needs_stdlib(&branch.decision))
                || fallback.as_deref().is_some_and(decision_needs_stdlib)
        }
        Decision::Chain { tests, fallback } => {
            tests
                .iter()
                .any(|test| decision_needs_stdlib(&test.decision))
                || decision_needs_stdlib(fallback)
        }
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
        statements.push(LoweredStatement::Break {
            label: Some(label.to_string()),
        });
    }
}
