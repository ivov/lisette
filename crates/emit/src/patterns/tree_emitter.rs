use syntax::ast::{Expression, MatchArm};
use syntax::types::Type;

use crate::EmitEffects;
use crate::Planner;
use crate::analyze::inline_uses::{InlineDecision, analyze_inline_candidate, region_blocks_inline};
use crate::context::expression::ExpressionContext;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::patterns::binding_emit::drop_inline_overlays;
use crate::patterns::decision_tree::{Decision, compile_expanded_arms, expand_or_patterns};
use crate::patterns::emit_plan::{
    ChainPlan, EmitBinding, EmitCase, EmitChainTest, EmitDecision, MatchEmitPlan, RetryLoopPlan,
    SingleCatchallPlan, is_empty_emit_decision, lower_match,
};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopPlan, LoweredBlock, LoweredStatement, PlacePlan, SwitchCasePlan,
    SwitchKind, SwitchStatementPlan,
};
use crate::plan::placement::unreachable_panic_if_needed;
use crate::state::bindings::{BindingValue, InlineExpr};

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
    subject_var: String,
    subject_ty: Type,
    fx: &'a mut EmitEffects,
}

impl<'a, 'e> TreePlanner<'a, 'e> {
    pub(crate) fn new(
        planner: &'a mut Planner<'e>,
        arms: &'a [MatchArm],
        subject_var: String,
        subject_ty: Type,
        fx: &'a mut EmitEffects,
    ) -> Self {
        Self {
            planner,
            arms,
            subject_var,
            subject_ty,
            fx,
        }
    }

    pub(crate) fn lower(mut self, place: &PlacePlan) -> LoweredBlock {
        let expanded = expand_or_patterns(self.arms);
        let compiled = compile_expanded_arms(self.planner, &expanded, &self.subject_ty);
        self.fx.extend(&compiled.effects);
        let tree = compiled.decision;

        let single_catchall_has_collisions = match &tree {
            Decision::Success { arm_index, .. } => self
                .planner
                .pattern_has_binding_collisions(&self.arms[*arm_index].pattern),
            _ => false,
        };

        let lowered = lower_match(
            self.arms,
            self.subject_var.clone(),
            &tree,
            single_catchall_has_collisions,
        );
        if lowered.effects.needs_stdlib {
            self.fx.require_stdlib();
        }

        let mut statements: Vec<LoweredStatement> = Vec::new();
        match lowered.plan {
            MatchEmitPlan::Switch { tree } => {
                let ctx = WalkCtx::switch_case(place);
                self.walk(&mut statements, &tree, &ctx);
            }
            MatchEmitPlan::SingleCatchall(plan) => {
                self.render_single_catchall(&mut statements, &plan, place);
            }
            MatchEmitPlan::Chain(plan) => {
                self.render_chain_root(&mut statements, &plan, place);
            }
            MatchEmitPlan::RetryLoop(plan) => {
                self.render_retry_loop(&mut statements, &plan, place);
            }
        }
        LoweredBlock { statements }
    }

    fn render_single_catchall(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        plan: &SingleCatchallPlan,
        place: &PlacePlan,
    ) {
        let arm_body = &*self.arms[plan.arm_index].expression;

        self.planner.enter_scope();
        let mut inner: Vec<LoweredStatement> = Vec::new();
        let inlines = self.emit_bindings(&mut inner, &plan.bindings, &[arm_body], None);
        self.emit_arm_body(&mut inner, plan.arm_index, place);
        self.drop_inline_bindings(&inlines);
        let needs_block =
            self.planner.scope.current_block_declared_nonempty() || plan.pattern_has_collisions;
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
        plan: &ChainPlan,
        place: &PlacePlan,
    ) {
        self.emit_chain_root_decision(statements, &plan.tree, place);
        if let Some(panic) = unreachable_panic_if_needed(place, plan.chain_tail_is_exhaustive) {
            statements.push(panic);
        }
    }

    fn emit_chain_root_decision(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        tree: &EmitDecision,
        place: &PlacePlan,
    ) {
        match tree {
            EmitDecision::Success {
                arm_index,
                bindings,
                ..
            } => {
                let arm_body = &*self.arms[*arm_index].expression;
                let inlines = self.emit_bindings(statements, bindings, &[arm_body], None);
                self.emit_arm_body(statements, *arm_index, place);
                self.drop_inline_bindings(&inlines);
            }
            EmitDecision::Chain {
                tests,
                fallback,
                last_is_catchall,
            } => {
                self.lower_chain_branch(statements, tests, fallback, *last_is_catchall, place);
            }
            EmitDecision::Unreachable => {}
            EmitDecision::Guard { .. } => {
                self.walk(statements, tree, &WalkCtx::chain_test(place));
            }
            _ => {
                self.walk(statements, tree, &WalkCtx::switch_case(place));
            }
        }
    }

    fn lower_chain_branch(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        tests: &[EmitChainTest],
        fallback: &EmitDecision,
        last_is_catchall: bool,
        place: &PlacePlan,
    ) {
        let regular = if last_is_catchall {
            &tests[..tests.len() - 1]
        } else {
            tests
        };

        let guard_ctx = WalkCtx::switch_case(place);
        let chain_ctx = WalkCtx::chain_test(place);

        // Lower each branch body in its own scope, recording the last branch's
        // divergence so the trailing else decides else/flat structurally.
        let mut branches: Vec<ChainBranch> = Vec::with_capacity(regular.len());
        let mut last_diverges = false;
        for test in regular {
            if let Some(subject) = &test.subject {
                self.planner.scope.record_go_use(subject);
            }
            let condition = test.condition.as_deref().unwrap_or("true").to_string();
            let walk_ctx = if matches!(test.decision.as_ref(), EmitDecision::Guard { .. }) {
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
        } else if matches!(fallback, EmitDecision::Unreachable) {
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
        plan: &RetryLoopPlan,
        place: &PlacePlan,
    ) {
        let use_direct_return = place.is_return();
        let unguarded_exit = plan.root_has_unguarded_terminal || plan.last_arm_is_any_catchall;
        let skip_wrapper = !use_direct_return && unguarded_exit && plan.all_arms_diverge;

        // No `for { ... }` wrapper: walk the tree flat (direct-return or a
        // diverging-exit fast path).
        if use_direct_return || skip_wrapper {
            let ctx = WalkCtx::retry_loop(place, None);
            self.walk(statements, &plan.tree, &ctx);
            if use_direct_return && !plan.root_has_unguarded_terminal {
                statements.push(LoweredStatement::UnreachablePanic);
            }
            return;
        }

        // Wrap the tree in a labeled `for { ... }` retry loop.
        let label = self.planner.fresh_var(Some("match"));
        let ctx = WalkCtx::retry_loop(place, Some(label.as_str()));
        let mut body: Vec<LoweredStatement> = Vec::new();
        self.walk(&mut body, &plan.tree, &ctx);
        if !unguarded_exit {
            body.push(LoweredStatement::Break {
                directive: String::new(),
                label: Some(label.clone()),
            });
        }
        statements.push(LoweredStatement::Loop(LoopPlan {
            directive: String::new(),
            prologue: String::new(),
            label: Some(label),
            header: "for {\n".to_string(),
            body: LoweredBlock { statements: body },
        }));
    }

    fn walk(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        decision: &EmitDecision,
        ctx: &WalkCtx,
    ) {
        match decision {
            EmitDecision::Success {
                arm_index,
                bindings,
                ..
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
                self.drop_inline_bindings(&inlines);
                if wrap {
                    self.planner.exit_scope();
                    statements.push(LoweredStatement::Block(LoweredBlock { statements: leaf }));
                } else {
                    statements.extend(leaf);
                }
            }
            EmitDecision::Guard {
                arm_index,
                bindings,
                success,
                failure,
            } => self.walk_guard(statements, *arm_index, bindings, success, failure, ctx),
            EmitDecision::IfElse {
                condition,
                subject,
                then_branch,
                else_branch,
            } => {
                if let Some(subject) = subject {
                    self.planner.scope.record_go_use(subject);
                }
                let plan =
                    self.lower_if_else_block(condition, then_branch, else_branch.as_deref(), ctx);
                let body_diverges = capture_diverge(vec![LoweredStatement::If(plan)], statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
            EmitDecision::InlineBranch { branch } => {
                let inner = WalkCtx::switch_case(ctx.arm_place);
                let mut branch_statements: Vec<LoweredStatement> = Vec::new();
                self.walk(&mut branch_statements, branch, &inner);
                let body_diverges = capture_diverge(branch_statements, statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
            EmitDecision::Switch {
                expr,
                subject,
                cases,
                default,
            } => {
                self.planner.scope.record_go_use(subject);
                let plan = self.lower_value_switch(expr, cases, default.as_deref(), ctx.arm_place);
                let body_diverges =
                    capture_diverge(vec![LoweredStatement::Switch(plan)], statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
            EmitDecision::TypeSwitch {
                base,
                subject,
                cases,
                default,
            } => {
                self.planner.scope.record_go_use(subject);
                let plan = self.lower_type_switch(base, cases, default.as_deref(), ctx.arm_place);
                let body_diverges =
                    capture_diverge(vec![LoweredStatement::Switch(plan)], statements);
                apply_leaf_terminator(statements, ctx, body_diverges);
            }
            EmitDecision::Chain {
                tests,
                fallback,
                last_is_catchall,
            } => {
                if ctx.is_grouped_retry() {
                    self.emit_chain_grouped(statements, tests, fallback, *last_is_catchall, ctx);
                } else {
                    self.lower_chain_branch(
                        statements,
                        tests,
                        fallback,
                        *last_is_catchall,
                        ctx.arm_place,
                    );
                }
            }
            EmitDecision::Unreachable => {}
        }
    }

    fn walk_guard(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        arm_index: usize,
        bindings: &[EmitBinding],
        success: &EmitDecision,
        failure: &EmitDecision,
        ctx: &WalkCtx,
    ) {
        let needs_pre_scope = ctx.leaf_scope_explicit() && !bindings.is_empty();
        if needs_pre_scope {
            self.planner.enter_scope();
        }
        let arm = &self.arms[arm_index];
        let arm_body = &*arm.expression;
        let mut guard_consumers: Vec<&Expression> = Vec::with_capacity(2);
        if let Some(g) = arm.guard.as_deref() {
            guard_consumers.push(g);
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
            self.drop_inline_bindings(&inlines);
            let else_arm = if ctx.role == WalkRole::SwitchCase {
                self.lower_else_or_flat(failure, ctx, success_diverges)
            } else {
                ElseArm::None
            };
            guard_statements.push(LoweredStatement::If(IfPlan {
                directive: String::new(),
                condition_setup,
                condition,
                then_body,
                else_arm,
            }));
        } else {
            self.drop_inline_bindings(&inlines);
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
        decision: &EmitDecision,
        ctx: &WalkCtx,
        preceding_diverges: bool,
    ) -> ElseArm {
        if is_empty_emit_decision(decision) {
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

    fn lower_if_else_block(
        &mut self,
        condition: &str,
        then_branch: &EmitDecision,
        else_branch: Option<&EmitDecision>,
        ctx: &WalkCtx,
    ) -> IfPlan {
        let inner = WalkCtx::switch_case(ctx.arm_place);
        self.planner.enter_scope();
        let mut then_statements: Vec<LoweredStatement> = Vec::new();
        self.walk(&mut then_statements, then_branch, &inner);
        self.planner.exit_scope();
        let then_body = LoweredBlock {
            statements: then_statements,
        };
        let then_diverges = then_body.ends_with_diverge();
        let else_arm = match else_branch {
            Some(decision) => self.lower_else_or_flat(decision, &inner, then_diverges),
            None => ElseArm::None,
        };
        IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: condition.to_string(),
            then_body,
            else_arm,
        }
    }

    fn lower_value_switch(
        &mut self,
        expr: &str,
        cases: &[EmitCase],
        default: Option<&EmitDecision>,
        place: &PlacePlan,
    ) -> SwitchStatementPlan {
        let case_plans = self.lower_switch_cases(cases, place);
        let default_block = self.lower_switch_default(default, place);
        SwitchStatementPlan {
            directive: String::new(),
            kind: SwitchKind::Value {
                subject: expr.to_string(),
            },
            cases: case_plans,
            default: default_block,
            postlude: switch_postlude(place, default.is_some()),
        }
    }

    fn lower_type_switch(
        &mut self,
        base: &str,
        cases: &[EmitCase],
        default: Option<&EmitDecision>,
        place: &PlacePlan,
    ) -> SwitchStatementPlan {
        self.planner.scope.enter_use_region();
        let case_plans = self.lower_switch_cases(cases, place);
        let default_block = self.lower_switch_default(default, place);
        let used = self.planner.scope.exit_use_region();

        // Keep the `base :=` type-switch binding only when a case references it;
        // Go rejects an unused `:= base` assignment otherwise.
        let references_base = used.contains(base);
        let binding = references_base.then(|| base.to_string());

        SwitchStatementPlan {
            directive: String::new(),
            kind: SwitchKind::Type {
                subject: base.to_string(),
                binding,
            },
            cases: case_plans,
            default: default_block,
            postlude: switch_postlude(place, default.is_some()),
        }
    }

    fn lower_switch_cases(&mut self, cases: &[EmitCase], place: &PlacePlan) -> Vec<SwitchCasePlan> {
        let ctx = WalkCtx::switch_case(place);
        let mut case_plans = Vec::with_capacity(cases.len());
        for case in cases {
            let mut body: Vec<LoweredStatement> = Vec::new();
            self.planner.enter_scope();
            self.walk(&mut body, &case.decision, &ctx);
            self.planner.exit_scope();
            case_plans.push(SwitchCasePlan {
                labels: case.case_label.clone(),
                body: LoweredBlock { statements: body },
            });
        }
        case_plans
    }

    /// Lower the default arm, dropping it when its body lowers to nothing (Go
    /// would otherwise emit a bare `default:`).
    fn lower_switch_default(
        &mut self,
        default: Option<&EmitDecision>,
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
        tests: &[EmitChainTest],
        fallback: &EmitDecision,
        last_is_catchall: bool,
        ctx: &WalkCtx,
    ) {
        let inner_ctx = ctx.nested();
        let groups = group_chain_tests_by_condition(tests);
        let group_count = groups.len();

        for (g, (_condition, indices)) in groups.iter().enumerate() {
            let is_last_group = g == group_count - 1;
            let collapse_as_catchall = is_last_group && last_is_catchall && indices.len() == 1;
            self.emit_chain_group(statements, indices, tests, &inner_ctx, collapse_as_catchall);
        }

        if !matches!(fallback, EmitDecision::Unreachable) {
            self.walk(statements, fallback, ctx);
        }
    }

    fn emit_chain_group(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        indices: &[usize],
        tests: &[EmitChainTest],
        ctx: &WalkCtx,
        collapse_as_catchall: bool,
    ) {
        if collapse_as_catchall {
            self.walk(statements, &tests[indices[0]].decision, ctx);
            return;
        }

        let first = &tests[indices[0]];
        self.planner.enter_scope();
        let mut body: Vec<LoweredStatement> = Vec::new();
        if bindings_are_hoistable(tests, indices) {
            self.emit_chain_group_hoisted(&mut body, indices, tests, ctx);
        } else {
            self.emit_chain_group_per_test(&mut body, indices, tests, ctx);
        }
        self.planner.exit_scope();

        let body = LoweredBlock { statements: body };
        if let Some(subject) = &first.subject {
            self.planner.scope.record_go_use(subject);
        }
        match &first.condition {
            Some(condition) => statements.push(LoweredStatement::If(IfPlan {
                directive: String::new(),
                condition_setup: String::new(),
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
        tests: &[EmitChainTest],
        ctx: &WalkCtx,
    ) {
        let mut inlines: Vec<(String, Option<BindingValue>)> = Vec::new();
        if let Some(&ref_index) = indices
            .iter()
            .find(|&&index| !decision_top_bindings(&tests[index].decision).is_empty())
        {
            let mut consumers: Vec<&Expression> = Vec::new();
            for &index in indices {
                let decision = &*tests[index].decision;
                let arm_index = match decision {
                    EmitDecision::Success { arm_index, .. }
                    | EmitDecision::Guard { arm_index, .. } => Some(*arm_index),
                    _ => None,
                };
                if let Some(arm_index) = arm_index {
                    let arm = &self.arms[arm_index];
                    if let Some(g) = arm.guard.as_deref() {
                        consumers.push(g);
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
            match &*tests[test_index].decision {
                EmitDecision::Success { arm_index, .. } => {
                    let mut body_statements: Vec<LoweredStatement> = Vec::new();
                    self.emit_arm_body(&mut body_statements, *arm_index, ctx.arm_place);
                    let body_diverges = capture_diverge(body_statements, statements);
                    apply_leaf_terminator(statements, ctx, body_diverges);
                }
                EmitDecision::Guard { arm_index, .. } => {
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
                            directive: String::new(),
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
        self.drop_inline_bindings(&inlines);
    }

    fn emit_chain_group_per_test(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        indices: &[usize],
        tests: &[EmitChainTest],
        ctx: &WalkCtx,
    ) {
        for (j, &test_index) in indices.iter().enumerate() {
            let is_last_in_group = j == indices.len() - 1;
            let needs_wrapper =
                !is_last_in_group && decision_has_bindings(&tests[test_index].decision);
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
    /// `drop_inline_bindings` to roll them back.
    fn emit_bindings(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        bindings: &[EmitBinding],
        consumers: &[&Expression],
        failure_blocker: Option<&EmitDecision>,
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
            let access_expression = &binding.rendered_access;

            let previous = self
                .planner
                .scope
                .resolve_identifier_binding(&binding.lisette_name)
                .cloned();
            if self.try_inline_binding(
                &binding.lisette_name,
                &binding.composable_access,
                &binding.subject,
                consumers,
                &failure_trees,
            ) {
                installed_inlines.push((binding.lisette_name.clone(), previous));
                continue;
            }

            self.planner.scope.record_go_use(&binding.subject);
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

    fn drop_inline_bindings(&mut self, installed: &[(String, Option<BindingValue>)]) {
        drop_inline_overlays(self.planner, installed);
    }

    fn try_inline_binding(
        &mut self,
        lisette_name: &str,
        composable_access: &str,
        subject: &str,
        consumers: &[&Expression],
        failure_trees: &[&Expression],
    ) -> bool {
        if consumers.is_empty() {
            return false;
        }
        if analyze_inline_candidate(lisette_name, consumers) != InlineDecision::Inline {
            return false;
        }
        if !failure_trees.is_empty()
            && region_blocks_inline(failure_trees.iter().copied(), lisette_name)
        {
            return false;
        }
        self.planner.scope.bind_inline_expr(
            lisette_name,
            InlineExpr::with_refs(composable_access, vec![subject.to_string()]),
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
        let block = self
            .planner
            .lower_block_to_place(&arm.expression, place, self.fx);
        statements.extend(block.statements);
    }

    /// Lower an arm's guard to `(condition_setup, condition)` for an `IfPlan`,
    /// or `None` when the arm has no guard. The caller owns the scope and body.
    fn lower_guard_condition(&mut self, arm_index: usize) -> Option<(String, String)> {
        let guard_expression = self.arms[arm_index].guard.as_deref()?;
        let mut condition_setup = String::new();
        let guard_str = self.planner.emit_operand(
            &mut condition_setup,
            guard_expression,
            ExpressionContext::value().condition(),
            self.fx,
        );
        Some((condition_setup, wrap_if_struct_literal(guard_str)))
    }
}

fn decision_top_bindings(decision: &EmitDecision) -> &[EmitBinding] {
    match decision {
        EmitDecision::Guard { bindings, .. } | EmitDecision::Success { bindings, .. } => bindings,
        _ => &[],
    }
}

fn collect_reachable_arms(decision: &EmitDecision, out: &mut Vec<usize>) {
    match decision {
        EmitDecision::Success { arm_index, .. } => {
            if !out.contains(arm_index) {
                out.push(*arm_index);
            }
        }
        EmitDecision::Guard {
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
        EmitDecision::IfElse {
            then_branch,
            else_branch,
            ..
        } => {
            collect_reachable_arms(then_branch, out);
            if let Some(else_b) = else_branch.as_deref() {
                collect_reachable_arms(else_b, out);
            }
        }
        EmitDecision::InlineBranch { branch } => collect_reachable_arms(branch, out),
        EmitDecision::Switch { cases, default, .. }
        | EmitDecision::TypeSwitch { cases, default, .. } => {
            for c in cases {
                collect_reachable_arms(&c.decision, out);
            }
            if let Some(d) = default.as_deref() {
                collect_reachable_arms(d, out);
            }
        }
        EmitDecision::Chain {
            tests, fallback, ..
        } => {
            for t in tests {
                collect_reachable_arms(&t.decision, out);
            }
            collect_reachable_arms(fallback, out);
        }
        EmitDecision::Unreachable => {}
    }
}

fn decision_has_bindings(decision: &EmitDecision) -> bool {
    !decision_top_bindings(decision).is_empty()
}

fn bindings_are_hoistable(tests: &[EmitChainTest], indices: &[usize]) -> bool {
    if indices.len() <= 1 {
        return false;
    }
    let reference = indices.iter().find_map(|&index| {
        let b = decision_top_bindings(&tests[index].decision);
        if !b.is_empty() { Some(b) } else { None }
    });
    let Some(reference) = reference else {
        return false;
    };
    indices.iter().all(|&index| {
        let b = decision_top_bindings(&tests[index].decision);
        b.is_empty()
            || (b.len() == reference.len()
                && b.iter().zip(reference.iter()).all(|(a, r)| {
                    a.lisette_name == r.lisette_name
                        && a.go_name == r.go_name
                        && a.rendered_access == r.rendered_access
                }))
    })
}

fn group_chain_tests_by_condition<'a>(tests: &'a [EmitChainTest]) -> Vec<(&'a str, Vec<usize>)> {
    let mut groups: Vec<(&'a str, Vec<usize>)> = Vec::new();
    for (i, test) in tests.iter().enumerate() {
        let key = test.condition.as_deref().unwrap_or("");
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
            directive: String::new(),
            condition_setup: String::new(),
            condition: branch.condition,
            then_body: branch.body,
            else_arm,
        }));
    }
    IfPlan {
        directive: String::new(),
        condition_setup: String::new(),
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
            directive: String::new(),
            label: Some(label.to_string()),
        });
    }
}
