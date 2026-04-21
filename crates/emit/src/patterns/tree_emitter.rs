use syntax::ast::MatchArm;
use syntax::types::Type;

use crate::Emitter;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::patterns::decision_tree::{
    AccessPath, ChainTest, Decision, PatternBinding, SwitchBranch, SwitchKind,
    compile_expanded_arms, emit_tree_bindings, expand_or_patterns, render_condition,
};
use crate::types::emitter::Position;
use crate::utils::{inline_trivial_bindings, output_ends_with_diverge, output_references_var};
use crate::write_line;

struct GuardedTreeContext<'a> {
    arm_position: &'a Position,
    subject_var: &'a str,
    label: &'a str,
    use_direct_return: bool,
}

pub(crate) struct TreeEmitter<'a, 'e> {
    emitter: &'a mut Emitter<'e>,
    arms: &'a [MatchArm],
    ty: &'a Type,
    subject_var: String,
    subject_ty: Type,
}

impl<'a, 'e> TreeEmitter<'a, 'e> {
    pub(crate) fn new(
        emitter: &'a mut Emitter<'e>,
        arms: &'a [MatchArm],
        ty: &'a Type,
        subject_var: String,
        subject_ty: Type,
    ) -> Self {
        Self {
            emitter,
            arms,
            ty,
            subject_var,
            subject_ty,
        }
    }

    pub(crate) fn emit(mut self, output: &mut String) {
        let pre_len = output.len();
        let expanded = expand_or_patterns(self.arms);
        let tree = compile_expanded_arms(self.emitter, &expanded, &self.subject_ty);

        let arm_position = self.emitter.compute_arm_position(Some(output), self.ty);
        let position = arm_position.position.clone();
        let needs_return = arm_position.needs_return;
        let result_var = arm_position.result_var().map(|s| s.to_string());

        if matches!(tree, Decision::Switch { .. }) {
            self.emit_switch(output, &tree, &position);
            if let Some(var) = &result_var {
                write_line!(output, "return {}", var);
            }
            inline_trivial_bindings(output, pre_len);
            return;
        }

        if let Decision::Success {
            arm_index,
            bindings,
        } = &tree
        {
            self.emit_single_catchall(output, *arm_index, bindings, &position, &result_var);
            inline_trivial_bindings(output, pre_len);
            return;
        }

        let has_guards = self.arms.iter().any(|arm| arm.has_guard());
        if has_guards {
            self.emit_with_loop(output, &tree, &position, needs_return);
        } else {
            self.emit_chain(output, &tree, &position, &result_var);
        }
        inline_trivial_bindings(output, pre_len);
    }

    fn emit_switch(&mut self, output: &mut String, tree: &Decision, arm_position: &Position) {
        let Decision::Switch {
            path,
            kind,
            branches,
            fallback,
        } = tree
        else {
            unreachable!("emit_switch called on non-Switch");
        };

        if matches!(kind, SwitchKind::TypeSwitch) {
            return self.emit_type_switch(output, tree, arm_position);
        }

        let subject_var = self.subject_var.clone();
        let switch_expression = self.render_switch_expression(path, kind, &subject_var);

        if self.try_emit_boolean_switch(
            output,
            kind,
            branches,
            fallback,
            &switch_expression,
            arm_position,
            &subject_var,
        ) {
            return;
        }

        if self.try_emit_two_branch_switch(
            output,
            branches,
            fallback,
            &switch_expression,
            arm_position,
            &subject_var,
        ) {
            return;
        }

        if self.try_emit_single_branch_switch(
            output,
            branches,
            fallback,
            &switch_expression,
            arm_position,
            &subject_var,
        ) {
            return;
        }

        write_line!(output, "switch {} {{", switch_expression);
        self.emit_switch_body(output, branches, fallback, arm_position, &subject_var, true);
    }

    /// Render the expression driving a non-type-switch dispatch.
    /// `EnumTag` reads `.Tag` off the subject; `Value` uses the subject path
    /// directly. Both wrap struct literals to keep gofmt from stripping parens.
    fn render_switch_expression(
        &self,
        path: &AccessPath,
        kind: &SwitchKind,
        subject_var: &str,
    ) -> String {
        let base = path.render(subject_var);
        match kind {
            SwitchKind::EnumTag => wrap_if_struct_literal(format!("{}.Tag", base)),
            SwitchKind::Value => wrap_if_struct_literal(base),
            SwitchKind::TypeSwitch => unreachable!("handled by emit_type_switch"),
        }
    }

    /// Propagate the stdlib-needed flag from any of the supplied branches.
    /// Collected in one place because every small-shape specialization
    /// otherwise repeats the same assignment.
    fn mark_stdlib_from_branches(&mut self, branches: &[&SwitchBranch]) {
        if branches.iter().any(|b| b.needs_stdlib) {
            self.emitter.flags.needs_stdlib = true;
        }
    }

    /// Emit `if <cond> { <then> }` with an optional else arm. The else branch
    /// may inline (e.g. a nested `if`) via `emit_else_or_flat`; `None` closes
    /// the if block with `}\n`.
    fn emit_if_else_arm(
        &mut self,
        output: &mut String,
        condition: &str,
        then: &Decision,
        otherwise: Option<&Decision>,
        arm_position: &Position,
        subject_var: &str,
    ) {
        write_line!(output, "if {} {{", condition);
        self.emitter.enter_scope();
        self.emit_decision_in_case(output, then, arm_position, subject_var);
        self.emitter.exit_scope();
        match otherwise {
            Some(d) => self.emit_else_or_flat(output, d, arm_position, subject_var),
            None => output.push_str("}\n"),
        }
    }

    /// Two-branch value switch with `true`/`false` labels: emit as a direct
    /// `if <expr> { true_branch } else { false_branch }` on the raw expression.
    #[allow(clippy::too_many_arguments)]
    fn try_emit_boolean_switch(
        &mut self,
        output: &mut String,
        kind: &SwitchKind,
        branches: &[SwitchBranch],
        fallback: &Option<Box<Decision>>,
        switch_expression: &str,
        arm_position: &Position,
        subject_var: &str,
    ) -> bool {
        if branches.len() != 2
            || fallback.is_some()
            || !matches!(kind, SwitchKind::Value)
            || !branches.iter().any(|b| b.case_label == "true")
            || !branches.iter().any(|b| b.case_label == "false")
        {
            return false;
        }
        let (true_branch, false_branch) = if branches[0].case_label == "true" {
            (&branches[0], &branches[1])
        } else {
            (&branches[1], &branches[0])
        };
        self.mark_stdlib_from_branches(&[true_branch, false_branch]);
        self.emit_if_else_arm(
            output,
            switch_expression,
            &true_branch.decision,
            Some(&false_branch.decision),
            arm_position,
            subject_var,
        );
        true
    }

    /// Two-branch non-boolean switch (common for Option/Result): emit as
    /// `if <expr> == <first.label> { first } else { second }`.
    fn try_emit_two_branch_switch(
        &mut self,
        output: &mut String,
        branches: &[SwitchBranch],
        fallback: &Option<Box<Decision>>,
        switch_expression: &str,
        arm_position: &Position,
        subject_var: &str,
    ) -> bool {
        if branches.len() != 2 || fallback.is_some() {
            return false;
        }
        let first = &branches[0];
        let second = &branches[1];
        self.mark_stdlib_from_branches(&[first, second]);
        let condition = format!("{} == {}", switch_expression, first.case_label);
        self.emit_if_else_arm(
            output,
            &condition,
            &first.decision,
            Some(&second.decision),
            arm_position,
            subject_var,
        );
        true
    }

    /// Single-branch shapes: one branch + no-or-empty fallback (plain `if`),
    /// one branch + real fallback (`if/else`), or one branch + missing
    /// fallback (inline the body, no condition wrapper — exhaustive enum).
    fn try_emit_single_branch_switch(
        &mut self,
        output: &mut String,
        branches: &[SwitchBranch],
        fallback: &Option<Box<Decision>>,
        switch_expression: &str,
        arm_position: &Position,
        subject_var: &str,
    ) -> bool {
        if branches.len() != 1 {
            return false;
        }
        let branch = &branches[0];
        self.mark_stdlib_from_branches(&[branch]);

        if self.is_empty_fallback(fallback) {
            let condition = format!("{} == {}", switch_expression, branch.case_label);
            self.emit_if_else_arm(
                output,
                &condition,
                &branch.decision,
                None,
                arm_position,
                subject_var,
            );
            return true;
        }
        if let Some(fb) = fallback.as_deref() {
            let condition = format!("{} == {}", switch_expression, branch.case_label);
            self.emit_if_else_arm(
                output,
                &condition,
                &branch.decision,
                Some(fb),
                arm_position,
                subject_var,
            );
            return true;
        }
        // Single-variant enum: emit the body directly, no wrapper.
        self.emit_decision_in_case(output, &branch.decision, arm_position, subject_var);
        true
    }

    /// Emit a Go type switch: `switch x := x.(type) { case T: ... default: ... }`.
    fn emit_type_switch(&mut self, output: &mut String, tree: &Decision, arm_position: &Position) {
        let Decision::Switch {
            path,
            branches,
            fallback,
            ..
        } = tree
        else {
            unreachable!("emit_type_switch called on non-Switch");
        };

        let subject_var = self.subject_var.clone();
        let base = path.render(&subject_var);

        let header_start = output.len();
        write_line!(output, "switch {} := {}.(type) {{", base, base);
        let body_start = output.len();
        self.emit_switch_body(output, branches, fallback, arm_position, &base, false);
        if !output_references_var(&output[body_start..], &base) {
            let new_header = format!("switch {}.(type) {{\n", base);
            output.replace_range(header_start..body_start, &new_header);
        }
    }

    /// Emit case branches, the default block, the closing brace, and the
    /// unreachable guard for a switch that has already emitted its header line.
    ///
    /// `track_stdlib`: when true, propagates `needs_stdlib` from branch labels
    /// (required for enum-tag switches; not needed for type switches).
    fn emit_switch_body(
        &mut self,
        output: &mut String,
        branches: &[SwitchBranch],
        fallback: &Option<Box<Decision>>,
        arm_position: &Position,
        subject_var: &str,
        track_stdlib: bool,
    ) {
        let use_last_as_default = fallback.is_none() && !branches.is_empty();
        let regular_branches = if use_last_as_default {
            &branches[..branches.len() - 1]
        } else {
            branches
        };

        for branch in regular_branches {
            if track_stdlib && branch.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            write_line!(output, "case {}:", branch.case_label);
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, &branch.decision, arm_position, subject_var);
            self.emitter.exit_scope();
        }

        let default_decision = if use_last_as_default {
            let last = branches.last().unwrap();
            if track_stdlib && last.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            Some(&last.decision)
        } else {
            fallback.as_deref()
        };
        if let Some(decision) = default_decision {
            let pre = output.len();
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, decision, arm_position, subject_var);
            self.emitter.exit_scope();
            if output.len() > pre {
                output.insert_str(pre, "default:\n");
            }
        }

        output.push_str("}\n");

        self.emitter
            .emit_unreachable_if_needed(output, fallback.is_some() || use_last_as_default);
    }

    /// Close an if-block and emit the alternative decision, either flat (when the
    /// if-branch diverges) or wrapped in `} else { ... }`.
    fn emit_else_or_flat(
        &mut self,
        output: &mut String,
        decision: &Decision,
        arm_position: &Position,
        subject_var: &str,
    ) {
        if self.is_empty_decision(decision) {
            output.push_str("}\n");
        } else if output_ends_with_diverge(output) {
            output.push_str("}\n");
            self.emit_decision_in_case(output, decision, arm_position, subject_var);
        } else {
            output.push_str("} else {\n");
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, decision, arm_position, subject_var);
            self.emitter.exit_scope();
            output.push_str("}\n");
        }
    }

    fn emit_else_or_flat_chain(
        &mut self,
        output: &mut String,
        decision: &Decision,
        arm_position: &Position,
        subject_var: &str,
    ) {
        if self.is_empty_decision(decision) {
            output.push_str("}\n");
        } else if output_ends_with_diverge(output) {
            output.push_str("}\n");
            self.emit_chain_decision_body(output, decision, arm_position, subject_var);
        } else {
            output.push_str("} else {\n");
            self.emitter.enter_scope();
            self.emit_chain_decision_body(output, decision, arm_position, subject_var);
            self.emitter.exit_scope();
            output.push_str("}\n");
        }
    }

    /// Emit a guard's `if <condition> {` header and enter scope.
    /// Returns `true` if the guard was emitted; `false` if no guard exists.
    fn emit_guard_header(&mut self, output: &mut String, arm_index: usize) -> bool {
        let guard = &self.arms[arm_index].guard;
        if let Some(guard_expression) = guard {
            let guard_str = self
                .emitter
                .emit_condition_operand(output, guard_expression);
            let guard_str = wrap_if_struct_literal(guard_str);
            write_line!(output, "if {} {{", guard_str);
            self.emitter.enter_scope();
            true
        } else {
            false
        }
    }

    /// Emit a decision node inside a switch case.
    fn emit_decision_in_case(
        &mut self,
        output: &mut String,
        decision: &Decision,
        arm_position: &Position,
        subject_var: &str,
    ) {
        match decision {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                self.emit_bindings(output, bindings, subject_var);
                self.emit_arm_body(output, *arm_index, Some(arm_position));
            }
            Decision::Guard {
                arm_index,
                bindings,
                success,
                failure,
            } => {
                self.emit_bindings(output, bindings, subject_var);
                if self.emit_guard_header(output, *arm_index) {
                    self.emit_decision_in_case(output, success, arm_position, subject_var);
                    self.emitter.exit_scope();
                    self.emit_else_or_flat(output, failure, arm_position, subject_var);
                }
            }
            _ => {
                // Nested chain/switch inside a case
                self.emit_chain_decisions(output, decision, arm_position, subject_var);
            }
        }
    }

    fn emit_single_catchall(
        &mut self,
        output: &mut String,
        arm_index: usize,
        bindings: &[PatternBinding],
        arm_position: &Position,
        result_var: &Option<String>,
    ) {
        let arm = &self.arms[arm_index];
        let emits_any_binding = bindings.iter().any(|b| b.go_name.is_some());
        let needs_block =
            emits_any_binding || self.emitter.pattern_has_binding_collisions(&arm.pattern);

        if needs_block {
            output.push_str("{\n");
            self.emitter.enter_scope();
        }

        let subject_var = self.subject_var.clone();
        self.emit_bindings(output, bindings, &subject_var);
        self.emit_arm_body(output, arm_index, Some(arm_position));

        if let Some(var) = result_var {
            write_line!(output, "return {}", var);
        }

        if needs_block {
            self.emitter.exit_scope();
            output.push_str("}\n");
        }
    }

    fn emit_chain(
        &mut self,
        output: &mut String,
        tree: &Decision,
        arm_position: &Position,
        result_var: &Option<String>,
    ) {
        let subject_var = self.subject_var.clone();
        self.emit_chain_decisions(output, tree, arm_position, &subject_var);

        if let Some(var) = result_var {
            write_line!(output, "return {}", var);
        }

        let has_catchall =
            self.arms.last().is_some_and(|arm| {
                Self::is_unconditional_catchall(&arm.pattern) && !arm.has_guard()
            }) || Self::decision_is_exhaustive(tree);
        self.emitter
            .emit_unreachable_if_needed(output, has_catchall);
    }

    fn emit_chain_decisions(
        &mut self,
        output: &mut String,
        tree: &Decision,
        arm_position: &Position,
        subject_var: &str,
    ) {
        match tree {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                self.emit_bindings(output, bindings, subject_var);
                self.emit_arm_body(output, *arm_index, Some(arm_position));
            }

            Decision::Chain { tests, fallback } => {
                let last_is_catchall =
                    matches!(fallback.as_ref(), Decision::Unreachable) && tests.len() > 1;

                let regular_tests = if last_is_catchall {
                    &tests[..tests.len() - 1]
                } else {
                    tests
                };

                for (i, test) in regular_tests.iter().enumerate() {
                    let condition = render_condition(&test.checks, subject_var);
                    let is_catchall = test.checks.is_empty();

                    self.emitter
                        .emit_branch_header(output, &condition, is_catchall, i == 0);

                    if matches!(test.decision, Decision::Guard { .. }) {
                        self.emit_decision_in_case(
                            output,
                            &test.decision,
                            arm_position,
                            subject_var,
                        );
                    } else {
                        self.emit_chain_decision_body(
                            output,
                            &test.decision,
                            arm_position,
                            subject_var,
                        );
                    }
                }

                self.emitter.exit_scope();
                if last_is_catchall {
                    let last_test = tests.last().unwrap();
                    self.emit_else_or_flat_chain(
                        output,
                        &last_test.decision,
                        arm_position,
                        subject_var,
                    );
                } else if matches!(fallback.as_ref(), Decision::Unreachable) {
                    output.push_str("}\n");
                } else {
                    self.emit_else_or_flat_chain(output, fallback, arm_position, subject_var);
                }
            }

            Decision::Switch { .. } => {
                self.emit_switch(output, tree, arm_position);
            }

            Decision::Unreachable => {}

            Decision::Guard { .. } => {
                self.emit_chain_decision_body(output, tree, arm_position, subject_var);
            }
        }
    }

    fn emit_chain_decision_body(
        &mut self,
        output: &mut String,
        decision: &Decision,
        arm_position: &Position,
        subject_var: &str,
    ) {
        match decision {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                self.emit_bindings(output, bindings, subject_var);
                self.emit_arm_body(output, *arm_index, Some(arm_position));
            }
            Decision::Guard {
                arm_index,
                bindings,
                success,
                ..
            } => {
                self.emit_bindings(output, bindings, subject_var);
                if self.emit_guard_header(output, *arm_index) {
                    self.emit_chain_decision_body(output, success, arm_position, subject_var);
                    self.emitter.exit_scope();
                    output.push_str("}\n");
                }
                // Failure falls through to the next branch
            }
            _ => {
                self.emit_chain_decisions(output, decision, arm_position, subject_var);
            }
        }
    }

    fn emit_with_loop(
        &mut self,
        output: &mut String,
        tree: &Decision,
        arm_position: &Position,
        needs_return: bool,
    ) {
        let use_direct_return = arm_position.is_tail();
        let tree_has_terminal = Self::tree_has_unguarded_terminal(tree);
        let last_arm_is_unguarded_catchall = tree_has_terminal
            || self
                .arms
                .last()
                .is_some_and(|arm| !arm.has_guard() && Emitter::is_catchall_pattern(&arm.pattern));

        let skip_wrapper = !use_direct_return
            && last_arm_is_unguarded_catchall
            && self
                .arms
                .iter()
                .all(|arm| arm.expression.diverges().is_some());

        let label = if use_direct_return || skip_wrapper {
            String::new()
        } else {
            let l = self.emitter.fresh_var(Some("match"));
            write_line!(output, "{}:\nfor {{", l);
            l
        };

        let subject_var = self.subject_var.clone();
        let ctx = GuardedTreeContext {
            arm_position,
            subject_var: &subject_var,
            label: &label,
            use_direct_return: use_direct_return || skip_wrapper,
        };
        self.emit_guarded_tree(output, tree, &ctx);

        if use_direct_return {
            if !tree_has_terminal {
                output.push_str("panic(\"unreachable\")\n");
            }
        } else if !skip_wrapper {
            if !last_arm_is_unguarded_catchall {
                write_line!(output, "break {}", label);
            }
            output.push_str("}\n");
            if needs_return && let Some(var) = arm_position.assign_target() {
                write_line!(output, "return {}", var);
            }
        }
    }

    fn emit_guarded_tree(
        &mut self,
        output: &mut String,
        tree: &Decision,
        ctx: &GuardedTreeContext,
    ) {
        match tree {
            Decision::Success {
                arm_index,
                bindings,
            } => self.emit_guarded_success(output, *arm_index, bindings, ctx),

            Decision::Guard {
                arm_index,
                bindings,
                failure,
                ..
            } => self.emit_guarded_guard(output, *arm_index, bindings, failure, ctx),

            Decision::Chain { tests, fallback } => {
                self.emit_guarded_chain(output, tests, fallback, ctx);
            }

            Decision::Switch { .. } => {
                self.emit_switch(output, tree, ctx.arm_position);
                if !ctx.use_direct_return && !output_ends_with_diverge(output) {
                    write_line!(output, "break {}", ctx.label);
                }
            }

            Decision::Unreachable => {}
        }
    }

    /// Successful leaf: emit the arm body in its own scope and terminate
    /// either by the arm's own divergence or by breaking out of the labeled
    /// retry loop the guarded tree lives inside.
    fn emit_guarded_success(
        &mut self,
        output: &mut String,
        arm_index: usize,
        bindings: &[PatternBinding],
        ctx: &GuardedTreeContext,
    ) {
        output.push_str("{\n");
        self.emitter.enter_scope();
        self.emit_bindings(output, bindings, ctx.subject_var);
        self.emit_arm_body(output, arm_index, Some(ctx.arm_position));
        if !ctx.use_direct_return && !output_ends_with_diverge(output) {
            write_line!(output, "break {}", ctx.label);
        }
        self.emitter.exit_scope();
        output.push_str("}\n");
    }

    /// Guard arm: emit bindings (possibly scoped), then an `if <guard>`
    /// header. On guard success emit the arm body; on guard failure recurse
    /// into the failure branch.
    fn emit_guarded_guard(
        &mut self,
        output: &mut String,
        arm_index: usize,
        bindings: &[PatternBinding],
        failure: &Decision,
        ctx: &GuardedTreeContext,
    ) {
        let needs_scope = !bindings.is_empty();
        if needs_scope {
            output.push_str("{\n");
            self.emitter.enter_scope();
        }
        self.emit_bindings(output, bindings, ctx.subject_var);

        if self.emit_guard_header(output, arm_index) {
            self.emit_arm_body(output, arm_index, Some(ctx.arm_position));
            if !ctx.use_direct_return && !output_ends_with_diverge(output) {
                write_line!(output, "break {}", ctx.label);
            }
            self.emitter.exit_scope();
            output.push_str("}\n");
        }

        if needs_scope {
            self.emitter.exit_scope();
            output.push_str("}\n");
        }

        self.emit_guarded_tree(output, failure, ctx);
    }

    /// Chain of tests: collapse consecutive tests sharing the same rendered
    /// condition into a single `if` block, then emit each group. When the
    /// fallback is unreachable and the last group is a singleton, unwrap it
    /// as an exhaustive catchall rather than emitting a dead condition.
    fn emit_guarded_chain(
        &mut self,
        output: &mut String,
        tests: &[ChainTest],
        fallback: &Decision,
        ctx: &GuardedTreeContext,
    ) {
        let last_is_catchall = matches!(fallback, Decision::Unreachable) && tests.len() > 1;
        let groups = self.group_chain_tests_by_condition(tests, ctx.subject_var);
        let group_count = groups.len();

        for (g, (condition, indices)) in groups.iter().enumerate() {
            let is_last_group = g == group_count - 1;
            let collapse_as_catchall = is_last_group && last_is_catchall && indices.len() == 1;
            self.emit_chain_group(output, condition, indices, tests, ctx, collapse_as_catchall);
        }

        if !matches!(fallback, Decision::Unreachable) {
            self.emit_guarded_tree(output, fallback, ctx);
        }
    }

    /// Group consecutive chain tests that render to the same condition, so
    /// e.g. three `if tag == Some { ... }` blocks collapse into one.
    fn group_chain_tests_by_condition(
        &self,
        tests: &[ChainTest],
        subject_var: &str,
    ) -> Vec<(String, Vec<usize>)> {
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, test) in tests.iter().enumerate() {
            let condition = render_condition(&test.checks, subject_var);
            if let Some((last_cond, indices)) = groups.last_mut()
                && *last_cond == condition
            {
                indices.push(i);
                continue;
            }
            groups.push((condition, vec![i]));
        }
        groups
    }

    /// Emit one group of merged-condition chain tests. `collapse_as_catchall`
    /// drops the condition wrapper entirely when the final group is an
    /// exhaustive singleton and the fallback is unreachable.
    fn emit_chain_group(
        &mut self,
        output: &mut String,
        condition: &str,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &GuardedTreeContext,
        collapse_as_catchall: bool,
    ) {
        if collapse_as_catchall {
            self.emit_guarded_tree_decision(output, &tests[indices[0]].decision, ctx, true);
            return;
        }

        if tests[indices[0]].checks.is_empty() {
            output.push_str("{\n");
        } else {
            write_line!(output, "if {} {{", condition);
        }
        self.emitter.enter_scope();

        if Self::bindings_are_hoistable(tests, indices, ctx.subject_var) {
            self.emit_chain_group_hoisted(output, indices, tests, ctx);
        } else {
            self.emit_chain_group_per_test(output, indices, tests, ctx);
        }

        self.emitter.exit_scope();
        output.push_str("}\n");
    }

    /// Hoist shared pattern bindings to the top of the merged block and emit
    /// each test's body without its own binding prelude. Caller pre-checked
    /// that the bindings are hoist-safe.
    fn emit_chain_group_hoisted(
        &mut self,
        output: &mut String,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &GuardedTreeContext,
    ) {
        if let Some(&ref_idx) = indices
            .iter()
            .find(|&&idx| !Self::decision_top_bindings(&tests[idx].decision).is_empty())
        {
            let bindings = Self::decision_top_bindings(&tests[ref_idx].decision);
            self.emit_bindings(output, bindings, ctx.subject_var);
        }
        for &test_idx in indices {
            self.emit_guarded_tree_decision(output, &tests[test_idx].decision, ctx, false);
        }
    }

    /// Emit each test in the group with its own binding prelude. Non-last
    /// tests that declare bindings are wrapped in their own block so the
    /// bindings stay scoped to that test.
    fn emit_chain_group_per_test(
        &mut self,
        output: &mut String,
        indices: &[usize],
        tests: &[ChainTest],
        ctx: &GuardedTreeContext,
    ) {
        for (j, &test_idx) in indices.iter().enumerate() {
            let is_last_in_group = j == indices.len() - 1;
            let needs_wrapper =
                !is_last_in_group && Self::decision_has_bindings(&tests[test_idx].decision);
            if needs_wrapper {
                output.push_str("{\n");
                self.emitter.enter_scope();
            }
            self.emit_guarded_tree_decision(output, &tests[test_idx].decision, ctx, true);
            if needs_wrapper {
                self.emitter.exit_scope();
                output.push_str("}\n");
            }
        }
    }

    fn emit_guarded_tree_decision(
        &mut self,
        output: &mut String,
        decision: &Decision,
        ctx: &GuardedTreeContext,
        emit_bindings: bool,
    ) {
        match decision {
            Decision::Success {
                arm_index,
                bindings,
            } => {
                if emit_bindings {
                    self.emit_bindings(output, bindings, ctx.subject_var);
                }
                self.emit_arm_body(output, *arm_index, Some(ctx.arm_position));
                if !ctx.use_direct_return && !output_ends_with_diverge(output) {
                    write_line!(output, "break {}", ctx.label);
                }
            }
            Decision::Guard {
                arm_index,
                bindings,
                ..
            } => {
                if emit_bindings {
                    self.emit_bindings(output, bindings, ctx.subject_var);
                }
                if self.emit_guard_header(output, *arm_index) {
                    self.emit_arm_body(output, *arm_index, Some(ctx.arm_position));
                    if !ctx.use_direct_return && !output_ends_with_diverge(output) {
                        write_line!(output, "break {}", ctx.label);
                    }
                    self.emitter.exit_scope();
                    output.push_str("}\n");
                }
                // Failure falls through
            }
            _ => {
                self.emit_guarded_tree(output, decision, ctx);
            }
        }
    }

    fn emit_bindings(
        &mut self,
        output: &mut String,
        bindings: &[PatternBinding],
        subject_var: &str,
    ) {
        emit_tree_bindings(self.emitter, output, bindings, subject_var);
    }

    fn emit_arm_body(
        &mut self,
        output: &mut String,
        arm_index: usize,
        position: Option<&Position>,
    ) {
        let arm = &self.arms[arm_index];
        match position {
            Some(position) => self.emitter.with_position(position.clone(), |e| {
                e.emit_in_position(output, &arm.expression);
            }),
            None => self
                .emitter
                .emit_in_position(output, &self.arms[arm_index].expression),
        }
    }

    /// Extract top-level bindings from a decision node.
    fn decision_top_bindings(decision: &Decision) -> &[PatternBinding] {
        match decision {
            Decision::Guard { bindings, .. } | Decision::Success { bindings, .. } => bindings,
            _ => &[],
        }
    }

    /// Whether a decision node's own bindings are non-empty (used to decide
    /// if a subscope wrapper `{ }` is needed inside merged guard groups).
    fn decision_has_bindings(decision: &Decision) -> bool {
        !Self::decision_top_bindings(decision).is_empty()
    }

    /// Check if all decisions in a merged group have identical bindings
    /// (same names, same paths), so they can be hoisted once at the top.
    fn bindings_are_hoistable(tests: &[ChainTest], indices: &[usize], subject_var: &str) -> bool {
        if indices.len() <= 1 {
            return false;
        }
        let reference = indices.iter().find_map(|&idx| {
            let b = Self::decision_top_bindings(&tests[idx].decision);
            if !b.is_empty() { Some(b) } else { None }
        });
        let Some(reference) = reference else {
            return false;
        };
        indices.iter().all(|&idx| {
            let b = Self::decision_top_bindings(&tests[idx].decision);
            b.is_empty()
                || (b.len() == reference.len()
                    && b.iter().zip(reference.iter()).all(|(a, r)| {
                        a.lisette_name == r.lisette_name
                            && a.go_name == r.go_name
                            && a.path.render(subject_var) == r.path.render(subject_var)
                    }))
        })
    }

    fn is_unconditional_catchall(pattern: &syntax::ast::Pattern) -> bool {
        match pattern {
            syntax::ast::Pattern::Or { patterns, .. } => {
                patterns.iter().all(Emitter::is_catchall_pattern)
            }
            other => Emitter::is_catchall_pattern(other),
        }
    }

    /// Check if a decision tree is structurally exhaustive (has an unconditional success path).
    fn decision_is_exhaustive(tree: &Decision) -> bool {
        match tree {
            Decision::Success { .. } => true,
            Decision::Chain {
                fallback, tests, ..
            } => {
                // When fallback is Unreachable with 2+ tests, the last test's
                // else branch is exhaustive (emitted as `} else {`).
                (matches!(fallback.as_ref(), Decision::Unreachable) && tests.len() > 1)
                    || Self::decision_is_exhaustive(fallback)
            }
            Decision::Switch {
                fallback, branches, ..
            } => fallback.is_some() || !branches.is_empty(),
            _ => false,
        }
    }

    /// Check if a guard tree has an unguarded terminal (i.e., the final fallback is
    /// an unconditional Success, not an Unreachable or a guarded arm).
    fn tree_has_unguarded_terminal(tree: &Decision) -> bool {
        match tree {
            Decision::Success { .. } => true,
            Decision::Guard { failure, .. } => Self::tree_has_unguarded_terminal(failure),
            Decision::Chain { tests, fallback } => {
                Self::tree_has_unguarded_terminal(fallback)
                    || (matches!(fallback.as_ref(), Decision::Unreachable)
                        && tests
                            .last()
                            .is_some_and(|t| Self::tree_has_unguarded_terminal(&t.decision)))
            }
            Decision::Switch {
                fallback, branches, ..
            } => fallback.as_ref().map_or(!branches.is_empty(), |fb| {
                Self::tree_has_unguarded_terminal(fb)
            }),
            Decision::Unreachable => false,
        }
    }

    /// Check if a decision would produce no output (unit body, no bindings).
    fn is_empty_decision(&self, decision: &Decision) -> bool {
        if let Decision::Success {
            arm_index,
            bindings,
        } = decision
        {
            if !bindings.is_empty() {
                return false;
            }
            let expression = &*self.arms[*arm_index].expression;
            return matches!(expression, syntax::ast::Expression::Unit { .. })
                || matches!(expression, syntax::ast::Expression::Block { items, .. } if items.is_empty());
        }
        false
    }

    /// Check if the switch fallback is an empty arm (unit body, no bindings).
    fn is_empty_fallback(&self, fallback: &Option<Box<Decision>>) -> bool {
        fallback
            .as_deref()
            .is_some_and(|fb| self.is_empty_decision(fb))
    }
}
