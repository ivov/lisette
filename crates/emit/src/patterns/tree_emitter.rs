use syntax::ast::MatchArm;
use syntax::types::Type;

use crate::Emitter;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::expressions::context::ExpressionContext;
use crate::patterns::decision_tree::{Decision, compile_expanded_arms, expand_or_patterns};
use crate::patterns::emit_plan::{
    ChainPlan, EmitBinding, EmitCase, EmitChainTest, EmitDecision, MatchEmitPlan, RetryLoopPlan,
    SingleCatchallPlan, is_empty_emit_decision, lower_match,
};
use crate::placement::{BodyPlace, emit_unreachable_panic_if_needed};
use crate::utils::{output_ends_with_diverge, output_references_var};
use crate::write_line;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WalkRole {
    SwitchCase,
    ChainBody,
    RetryLoopTop,
    RetryLoopNested,
}

#[derive(Clone, Copy)]
struct WalkCtx<'a> {
    arm_place: &'a BodyPlace<'a>,
    role: WalkRole,
    /// Set on retry-loop walks that need a `break <label>` terminator at
    /// non-divergent leaves; `None` for switch-case/chain-body and for
    /// direct-return or skip-wrapper retry loops.
    break_label: Option<&'a str>,
}

impl<'a> WalkCtx<'a> {
    fn switch_case(arm_place: &'a BodyPlace<'a>) -> Self {
        Self {
            arm_place,
            role: WalkRole::SwitchCase,
            break_label: None,
        }
    }

    fn chain_test(arm_place: &'a BodyPlace<'a>) -> Self {
        Self {
            arm_place,
            role: WalkRole::ChainBody,
            break_label: None,
        }
    }

    fn retry_loop(arm_place: &'a BodyPlace<'a>, break_label: Option<&'a str>) -> Self {
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

pub(crate) struct TreeEmitter<'a, 'e> {
    emitter: &'a mut Emitter<'e>,
    arms: &'a [MatchArm],
    subject_var: String,
    subject_ty: Type,
}

impl<'a, 'e> TreeEmitter<'a, 'e> {
    pub(crate) fn new(
        emitter: &'a mut Emitter<'e>,
        arms: &'a [MatchArm],
        subject_var: String,
        subject_ty: Type,
    ) -> Self {
        Self {
            emitter,
            arms,
            subject_var,
            subject_ty,
        }
    }

    pub(crate) fn emit(mut self, output: &mut String, place: &BodyPlace) {
        let expanded = expand_or_patterns(self.arms);
        let compiled = compile_expanded_arms(self.emitter, &expanded, &self.subject_ty);
        self.emitter.requirements.apply_effects(&compiled.effects);
        let tree = compiled.decision;

        let single_catchall_has_collisions = match &tree {
            Decision::Success { arm_index, .. } => self
                .emitter
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
            self.emitter.requirements.require_stdlib();
        }

        match lowered.plan {
            MatchEmitPlan::Switch { tree } => {
                let ctx = WalkCtx::switch_case(place);
                self.walk(output, &tree, &ctx);
            }
            MatchEmitPlan::SingleCatchall(plan) => {
                self.render_single_catchall(output, &plan, place);
            }
            MatchEmitPlan::Chain(plan) => {
                self.render_chain_root(output, &plan, place);
            }
            MatchEmitPlan::RetryLoop(plan) => {
                self.render_retry_loop(output, &plan, place);
            }
        }
    }

    fn render_single_catchall(
        &mut self,
        output: &mut String,
        plan: &SingleCatchallPlan,
        place: &BodyPlace,
    ) {
        let emits_any_binding = plan.bindings.iter().any(|b| b.go_name.is_some());
        let needs_block = emits_any_binding || plan.pattern_has_collisions;

        if needs_block {
            output.push_str("{\n");
            self.emitter.enter_scope();
        }

        self.emit_bindings(output, &plan.bindings);
        self.emit_arm_body(output, plan.arm_index, place);

        if needs_block {
            self.emitter.exit_scope();
            output.push_str("}\n");
        }
    }

    fn render_chain_root(&mut self, output: &mut String, plan: &ChainPlan, place: &BodyPlace) {
        self.emit_chain_root_decision(output, &plan.tree, place);
        emit_unreachable_panic_if_needed(output, place, plan.chain_tail_is_exhaustive);
    }

    fn emit_chain_root_decision(
        &mut self,
        output: &mut String,
        tree: &EmitDecision,
        place: &BodyPlace,
    ) {
        match tree {
            EmitDecision::Success {
                arm_index,
                bindings,
                ..
            } => {
                self.emit_bindings(output, bindings);
                self.emit_arm_body(output, *arm_index, place);
            }
            EmitDecision::Chain {
                tests,
                fallback,
                last_is_catchall,
            } => {
                self.emit_chain_branch_header_loop(
                    output,
                    tests,
                    fallback,
                    *last_is_catchall,
                    place,
                );
            }
            EmitDecision::Unreachable => {}
            EmitDecision::Guard { .. } => {
                self.walk(output, tree, &WalkCtx::chain_test(place));
            }
            _ => {
                self.walk(output, tree, &WalkCtx::switch_case(place));
            }
        }
    }

    /// Shared by chain root and nested cascading chain rendering.
    fn emit_chain_branch_header_loop(
        &mut self,
        output: &mut String,
        tests: &[EmitChainTest],
        fallback: &EmitDecision,
        last_is_catchall: bool,
        place: &BodyPlace,
    ) {
        let regular = if last_is_catchall {
            &tests[..tests.len() - 1]
        } else {
            tests
        };

        let guard_ctx = WalkCtx::switch_case(place);
        let chain_ctx = WalkCtx::chain_test(place);
        for (i, test) in regular.iter().enumerate() {
            let is_catchall = test.cond.is_none();
            let condition = test.cond.as_deref().unwrap_or("");
            self.emitter
                .emit_branch_header(output, condition, is_catchall, i == 0);
            if matches!(test.decision.as_ref(), EmitDecision::Guard { .. }) {
                self.walk(output, &test.decision, &guard_ctx);
            } else {
                self.walk(output, &test.decision, &chain_ctx);
            }
        }

        self.emitter.exit_scope();
        if last_is_catchall {
            let last_test = tests.last().unwrap();
            self.walk_else_or_flat(output, &last_test.decision, &chain_ctx);
        } else if matches!(fallback, EmitDecision::Unreachable) {
            output.push_str("}\n");
        } else {
            self.walk_else_or_flat(output, fallback, &chain_ctx);
        }
    }

    fn render_retry_loop(&mut self, output: &mut String, plan: &RetryLoopPlan, place: &BodyPlace) {
        let use_direct_return = place.is_return();
        let unguarded_exit = plan.root_has_unguarded_terminal || plan.last_arm_is_any_catchall;
        let skip_wrapper = !use_direct_return && unguarded_exit && plan.all_arms_diverge;

        let label = if use_direct_return || skip_wrapper {
            String::new()
        } else {
            let l = self.emitter.fresh_var(Some("match"));
            write_line!(output, "{}:\nfor {{", l);
            l
        };

        let break_label = (!label.is_empty()).then_some(label.as_str());
        let ctx = WalkCtx::retry_loop(place, break_label);
        self.walk(output, &plan.tree, &ctx);

        if use_direct_return {
            if !plan.root_has_unguarded_terminal {
                output.push_str("panic(\"unreachable\")\n");
            }
        } else if !skip_wrapper {
            if !unguarded_exit {
                write_line!(output, "break {}", label);
            }
            output.push_str("}\n");
        }
    }

    fn walk(&mut self, output: &mut String, decision: &EmitDecision, ctx: &WalkCtx) {
        match decision {
            EmitDecision::Success {
                arm_index,
                bindings,
                ..
            } => {
                let wrap = ctx.leaf_scope_explicit();
                if wrap {
                    output.push_str("{\n");
                    self.emitter.enter_scope();
                }
                self.emit_bindings(output, bindings);
                self.emit_arm_body(output, *arm_index, ctx.arm_place);
                self.apply_leaf_terminator(output, ctx);
                if wrap {
                    self.emitter.exit_scope();
                    output.push_str("}\n");
                }
            }
            EmitDecision::Guard {
                arm_index,
                bindings,
                success,
                failure,
            } => {
                let needs_pre_scope = ctx.leaf_scope_explicit() && !bindings.is_empty();
                if needs_pre_scope {
                    output.push_str("{\n");
                    self.emitter.enter_scope();
                }
                self.emit_bindings(output, bindings);
                if self.emit_guard_header(output, *arm_index) {
                    self.walk(output, success, &ctx.nested());
                    self.emitter.exit_scope();
                    if ctx.role == WalkRole::SwitchCase {
                        self.walk_else_or_flat(output, failure, ctx);
                    } else {
                        output.push_str("}\n");
                    }
                }
                if needs_pre_scope {
                    self.emitter.exit_scope();
                    output.push_str("}\n");
                }
                if ctx.role == WalkRole::RetryLoopTop {
                    self.walk(output, failure, ctx);
                }
            }
            EmitDecision::IfElse {
                cond,
                then_branch,
                else_branch,
            } => {
                self.emit_if_else_block(output, cond, then_branch, else_branch.as_deref(), ctx);
                self.apply_leaf_terminator(output, ctx);
            }
            EmitDecision::InlineBranch { branch } => {
                let inner = WalkCtx::switch_case(ctx.arm_place);
                self.walk(output, branch, &inner);
                self.apply_leaf_terminator(output, ctx);
            }
            EmitDecision::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                self.emit_value_switch(output, expr, cases, default.as_deref(), ctx.arm_place);
                self.apply_leaf_terminator(output, ctx);
            }
            EmitDecision::TypeSwitch {
                base,
                cases,
                default,
            } => {
                self.emit_type_switch(output, base, cases, default.as_deref(), ctx.arm_place);
                self.apply_leaf_terminator(output, ctx);
            }
            EmitDecision::Chain {
                tests,
                fallback,
                last_is_catchall,
            } => {
                if ctx.is_grouped_retry() {
                    self.emit_chain_grouped(output, tests, fallback, *last_is_catchall, ctx);
                } else {
                    self.emit_chain_branch_header_loop(
                        output,
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

    fn apply_leaf_terminator(&mut self, output: &mut String, ctx: &WalkCtx) {
        if let Some(label) = ctx.break_label
            && !output_ends_with_diverge(output)
        {
            write_line!(output, "break {}", label);
        }
    }

    fn walk_else_or_flat(&mut self, output: &mut String, decision: &EmitDecision, ctx: &WalkCtx) {
        if is_empty_emit_decision(decision) {
            output.push_str("}\n");
        } else if output_ends_with_diverge(output) {
            output.push_str("}\n");
            self.walk(output, decision, ctx);
        } else {
            output.push_str("} else {\n");
            self.emitter.enter_scope();
            self.walk(output, decision, ctx);
            self.emitter.exit_scope();
            output.push_str("}\n");
        }
    }

    fn emit_if_else_block(
        &mut self,
        output: &mut String,
        cond: &str,
        then_branch: &EmitDecision,
        else_branch: Option<&EmitDecision>,
        ctx: &WalkCtx,
    ) {
        let inner = WalkCtx::switch_case(ctx.arm_place);
        write_line!(output, "if {} {{", cond);
        self.emitter.enter_scope();
        self.walk(output, then_branch, &inner);
        self.emitter.exit_scope();
        match else_branch {
            Some(d) => self.walk_else_or_flat(output, d, &inner),
            None => output.push_str("}\n"),
        }
    }

    fn emit_value_switch(
        &mut self,
        output: &mut String,
        expr: &str,
        cases: &[EmitCase],
        default: Option<&EmitDecision>,
        place: &BodyPlace,
    ) {
        write_line!(output, "switch {} {{", expr);
        let ctx = WalkCtx::switch_case(place);
        for case in cases {
            write_line!(output, "case {}:", case.case_label);
            self.emitter.enter_scope();
            self.walk(output, &case.decision, &ctx);
            self.emitter.exit_scope();
        }
        if let Some(default_decision) = default {
            let pre = output.len();
            self.emitter.enter_scope();
            self.walk(output, default_decision, &ctx);
            self.emitter.exit_scope();
            if output.len() > pre {
                output.insert_str(pre, "default:\n");
            }
        }
        output.push_str("}\n");
        emit_unreachable_panic_if_needed(output, place, default.is_some());
    }

    fn emit_type_switch(
        &mut self,
        output: &mut String,
        base: &str,
        cases: &[EmitCase],
        default: Option<&EmitDecision>,
        place: &BodyPlace,
    ) {
        let header_start = output.len();
        write_line!(output, "switch {} := {}.(type) {{", base, base);

        let (body, ()) = self.capture_output(output, |this, out| {
            let ctx = WalkCtx::switch_case(place);
            for case in cases {
                write_line!(out, "case {}:", case.case_label);
                this.emitter.enter_scope();
                this.walk(out, &case.decision, &ctx);
                this.emitter.exit_scope();
            }
            if let Some(default_decision) = default {
                let pre = out.len();
                this.emitter.enter_scope();
                this.walk(out, default_decision, &ctx);
                this.emitter.exit_scope();
                if out.len() > pre {
                    out.insert_str(pre, "default:\n");
                }
            }
            out.push_str("}\n");
        });

        if !output_references_var(&body, base) {
            output.truncate(header_start);
            write_line!(output, "switch {}.(type) {{", base);
        }
        output.push_str(&body);

        emit_unreachable_panic_if_needed(output, place, default.is_some());
    }

    fn capture_output<R>(
        &mut self,
        output: &mut String,
        f: impl FnOnce(&mut Self, &mut String) -> R,
    ) -> (String, R) {
        let before = output.len();
        let result = f(self, output);
        let captured = output[before..].to_string();
        output.truncate(before);
        (captured, result)
    }

    fn emit_chain_grouped(
        &mut self,
        output: &mut String,
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
            self.emit_chain_group(output, indices, tests, &inner_ctx, collapse_as_catchall);
        }

        if !matches!(fallback, EmitDecision::Unreachable) {
            self.walk(output, fallback, ctx);
        }
    }

    fn emit_chain_group(
        &mut self,
        output: &mut String,
        indices: &[usize],
        tests: &[EmitChainTest],
        ctx: &WalkCtx,
        collapse_as_catchall: bool,
    ) {
        if collapse_as_catchall {
            self.walk(output, &tests[indices[0]].decision, ctx);
            return;
        }

        let first = &tests[indices[0]];
        if let Some(cond) = &first.cond {
            write_line!(output, "if {} {{", cond);
        } else {
            output.push_str("{\n");
        }
        self.emitter.enter_scope();

        if bindings_are_hoistable(tests, indices) {
            self.emit_chain_group_hoisted(output, indices, tests, ctx);
        } else {
            self.emit_chain_group_per_test(output, indices, tests, ctx);
        }

        self.emitter.exit_scope();
        output.push_str("}\n");
    }

    fn emit_chain_group_hoisted(
        &mut self,
        output: &mut String,
        indices: &[usize],
        tests: &[EmitChainTest],
        ctx: &WalkCtx,
    ) {
        if let Some(&ref_idx) = indices
            .iter()
            .find(|&&idx| !decision_top_bindings(&tests[idx].decision).is_empty())
        {
            self.emit_bindings(output, decision_top_bindings(&tests[ref_idx].decision));
        }
        for &test_idx in indices {
            match &*tests[test_idx].decision {
                EmitDecision::Success { arm_index, .. } => {
                    self.emit_arm_body(output, *arm_index, ctx.arm_place);
                    self.apply_leaf_terminator(output, ctx);
                }
                EmitDecision::Guard { arm_index, .. } => {
                    if self.emit_guard_header(output, *arm_index) {
                        self.emit_arm_body(output, *arm_index, ctx.arm_place);
                        self.apply_leaf_terminator(output, ctx);
                        self.emitter.exit_scope();
                        output.push_str("}\n");
                    }
                }
                _ => self.walk(output, &tests[test_idx].decision, ctx),
            }
        }
    }

    fn emit_chain_group_per_test(
        &mut self,
        output: &mut String,
        indices: &[usize],
        tests: &[EmitChainTest],
        ctx: &WalkCtx,
    ) {
        for (j, &test_idx) in indices.iter().enumerate() {
            let is_last_in_group = j == indices.len() - 1;
            let needs_wrapper =
                !is_last_in_group && decision_has_bindings(&tests[test_idx].decision);
            if needs_wrapper {
                output.push_str("{\n");
                self.emitter.enter_scope();
            }
            self.walk(output, &tests[test_idx].decision, ctx);
            if needs_wrapper {
                self.emitter.exit_scope();
                output.push_str("}\n");
            }
        }
    }

    fn emit_bindings(&mut self, output: &mut String, bindings: &[EmitBinding]) {
        for binding in bindings {
            let Some(ref go_name) = binding.go_name else {
                self.emitter.scope.bind(&binding.lisette_name, "");
                continue;
            };
            let access_expression = &binding.rendered_access;
            if self.emitter.scope.has_binding_for_go_name(go_name) {
                let fresh = self.emitter.fresh_var(Some(&binding.lisette_name));
                self.emitter.scope.bind(&binding.lisette_name, &fresh);
                self.emitter.try_declare(&fresh);
                write_line!(output, "{} := {}", fresh, access_expression);
            } else {
                let name = self
                    .emitter
                    .scope
                    .bind(&binding.lisette_name, go_name.clone());
                if self.emitter.try_declare(&name) {
                    write_line!(output, "{} := {}", name, access_expression);
                } else {
                    let fresh = self.emitter.fresh_var(Some(&binding.lisette_name));
                    self.emitter.scope.bind(&binding.lisette_name, &fresh);
                    self.emitter.try_declare(&fresh);
                    write_line!(output, "{} := {}", fresh, access_expression);
                }
            }
        }
    }

    fn emit_arm_body(&mut self, output: &mut String, arm_index: usize, place: &BodyPlace) {
        let arm = &self.arms[arm_index];
        self.emitter
            .emit_body_to_place(output, &arm.expression, place);
    }

    fn emit_guard_header(&mut self, output: &mut String, arm_index: usize) -> bool {
        let guard = &self.arms[arm_index].guard;
        if let Some(guard_expression) = guard {
            let guard_str = self.emitter.emit_operand(
                output,
                guard_expression,
                ExpressionContext::value().condition(),
            );
            let guard_str = wrap_if_struct_literal(guard_str);
            write_line!(output, "if {} {{", guard_str);
            self.emitter.enter_scope();
            true
        } else {
            false
        }
    }
}

fn decision_top_bindings(decision: &EmitDecision) -> &[EmitBinding] {
    match decision {
        EmitDecision::Guard { bindings, .. } | EmitDecision::Success { bindings, .. } => bindings,
        _ => &[],
    }
}

fn decision_has_bindings(decision: &EmitDecision) -> bool {
    !decision_top_bindings(decision).is_empty()
}

fn bindings_are_hoistable(tests: &[EmitChainTest], indices: &[usize]) -> bool {
    if indices.len() <= 1 {
        return false;
    }
    let reference = indices.iter().find_map(|&idx| {
        let b = decision_top_bindings(&tests[idx].decision);
        if !b.is_empty() { Some(b) } else { None }
    });
    let Some(reference) = reference else {
        return false;
    };
    indices.iter().all(|&idx| {
        let b = decision_top_bindings(&tests[idx].decision);
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
        let key = test.cond.as_deref().unwrap_or("");
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
