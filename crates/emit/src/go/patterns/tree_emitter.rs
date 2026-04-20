use syntax::ast::MatchArm;
use syntax::types::Type;

use crate::Emitter;
use crate::go::control_flow::branching::wrap_if_struct_literal;
use crate::go::patterns::decision_tree::{
    ChainTest, Decision, PatternBinding, SwitchBranch, SwitchKind, compile_expanded_arms,
    emit_tree_bindings, expand_or_patterns, render_condition,
};
use crate::go::types::emitter::Position;
use crate::go::utils::{inline_trivial_bindings, output_ends_with_diverge, output_references_var};
use crate::go::write_line;

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

        let switch_expression = match kind {
            SwitchKind::EnumTag => {
                let base = path.render(&subject_var);
                wrap_if_struct_literal(format!("{}.Tag", base))
            }
            SwitchKind::Value => {
                let base = path.render(&subject_var);
                wrap_if_struct_literal(base)
            }
            SwitchKind::TypeSwitch => unreachable!("handled above"),
        };

        if branches.len() == 2
            && fallback.is_none()
            && matches!(kind, SwitchKind::Value)
            && branches.iter().any(|b| b.case_label == "true")
            && branches.iter().any(|b| b.case_label == "false")
        {
            let (true_branch, false_branch) = if branches[0].case_label == "true" {
                (&branches[0], &branches[1])
            } else {
                (&branches[1], &branches[0])
            };
            if true_branch.needs_stdlib || false_branch.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            write_line!(output, "if {} {{", switch_expression);
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, &true_branch.decision, arm_position, &subject_var);
            self.emitter.exit_scope();
            self.emit_else_or_flat(output, &false_branch.decision, arm_position, &subject_var);
            return;
        }

        // Two-branch switch (common for Option/Result): emit as if/else.
        if branches.len() == 2 && fallback.is_none() {
            let first = &branches[0];
            let second = &branches[1];
            if first.needs_stdlib || second.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            write_line!(
                output,
                "if {} == {} {{",
                switch_expression,
                first.case_label
            );
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, &first.decision, arm_position, &subject_var);
            self.emitter.exit_scope();
            self.emit_else_or_flat(output, &second.decision, arm_position, &subject_var);
            return;
        }

        if branches.len() == 1 && self.is_empty_fallback(fallback) {
            let branch = &branches[0];
            if branch.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            write_line!(
                output,
                "if {} == {} {{",
                switch_expression,
                branch.case_label
            );
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, &branch.decision, arm_position, &subject_var);
            self.emitter.exit_scope();
            output.push_str("}\n");
            return;
        }

        if branches.len() == 1 && fallback.is_some() {
            let branch = &branches[0];
            if branch.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            write_line!(
                output,
                "if {} == {} {{",
                switch_expression,
                branch.case_label
            );
            self.emitter.enter_scope();
            self.emit_decision_in_case(output, &branch.decision, arm_position, &subject_var);
            self.emitter.exit_scope();
            self.emit_else_or_flat(
                output,
                fallback.as_ref().unwrap(),
                arm_position,
                &subject_var,
            );
            return;
        }

        // Single-branch exhaustive switch (single-variant enum): emit body directly.
        if branches.len() == 1 && fallback.is_none() {
            let branch = &branches[0];
            if branch.needs_stdlib {
                self.emitter.flags.needs_stdlib = true;
            }
            self.emit_decision_in_case(output, &branch.decision, arm_position, &subject_var);
            return;
        }

        write_line!(output, "switch {} {{", switch_expression);
        self.emit_switch_body(output, branches, fallback, arm_position, &subject_var, true);
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
        let label = if use_direct_return {
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
            use_direct_return,
        };
        self.emit_guarded_tree(output, tree, &ctx);

        if use_direct_return {
            if !Self::tree_has_unguarded_terminal(tree) {
                output.push_str("panic(\"unreachable\")\n");
            }
        } else {
            let last_arm_is_unguarded_catchall =
                self.arms.last().is_some_and(|arm| {
                    !arm.has_guard() && Emitter::is_catchall_pattern(&arm.pattern)
                }) || Self::tree_has_unguarded_terminal(tree);
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
            } => {
                output.push_str("{\n");
                self.emitter.enter_scope();
                self.emit_bindings(output, bindings, ctx.subject_var);
                self.emit_arm_body(output, *arm_index, Some(ctx.arm_position));
                if !ctx.use_direct_return && !output_ends_with_diverge(output) {
                    write_line!(output, "break {}", ctx.label);
                }
                self.emitter.exit_scope();
                output.push_str("}\n");
            }

            Decision::Guard {
                arm_index,
                bindings,
                success: _,
                failure,
            } => {
                let needs_scope = !bindings.is_empty();
                if needs_scope {
                    output.push_str("{\n");
                    self.emitter.enter_scope();
                }
                self.emit_bindings(output, bindings, ctx.subject_var);

                if self.emit_guard_header(output, *arm_index) {
                    self.emit_arm_body(output, *arm_index, Some(ctx.arm_position));
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

            Decision::Chain { tests, fallback } => {
                let last_is_catchall =
                    matches!(fallback.as_ref(), Decision::Unreachable) && tests.len() > 1;

                // Group consecutive tests with the same rendered condition
                // so that e.g. three `if tag == Some { ... }` blocks merge
                // into a single `if tag == Some { ...; ...; ... }`.
                let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
                for (i, test) in tests.iter().enumerate() {
                    let condition = render_condition(&test.checks, ctx.subject_var);
                    if let Some((last_cond, indices)) = groups.last_mut()
                        && *last_cond == condition
                    {
                        indices.push(i);
                        continue;
                    }
                    groups.push((condition, vec![i]));
                }

                for (g, (condition, indices)) in groups.iter().enumerate() {
                    let is_last_group = g == groups.len() - 1;

                    if is_last_group && last_is_catchall && indices.len() == 1 {
                        self.emit_guarded_tree_decision(
                            output,
                            &tests[indices[0]].decision,
                            ctx,
                            true,
                        );
                        continue;
                    }

                    if tests[indices[0]].checks.is_empty() {
                        output.push_str("{\n");
                    } else {
                        write_line!(output, "if {} {{", condition);
                    }
                    self.emitter.enter_scope();

                    let can_hoist = Self::bindings_are_hoistable(tests, indices, ctx.subject_var);
                    if can_hoist {
                        // Hoist shared bindings once at the top of the merged block
                        if let Some(&ref_idx) = indices.iter().find(|&&idx| {
                            !Self::decision_top_bindings(&tests[idx].decision).is_empty()
                        }) {
                            let bindings = Self::decision_top_bindings(&tests[ref_idx].decision);
                            self.emit_bindings(output, bindings, ctx.subject_var);
                        }
                        for &test_idx in indices.iter() {
                            self.emit_guarded_tree_decision(
                                output,
                                &tests[test_idx].decision,
                                ctx,
                                false,
                            );
                        }
                    } else {
                        for (j, &test_idx) in indices.iter().enumerate() {
                            let is_last_in_group = j == indices.len() - 1;
                            let needs_wrapper = !is_last_in_group
                                && Self::decision_has_bindings(&tests[test_idx].decision);
                            if needs_wrapper {
                                output.push_str("{\n");
                                self.emitter.enter_scope();
                            }
                            self.emit_guarded_tree_decision(
                                output,
                                &tests[test_idx].decision,
                                ctx,
                                true,
                            );
                            if needs_wrapper {
                                self.emitter.exit_scope();
                                output.push_str("}\n");
                            }
                        }
                    }

                    self.emitter.exit_scope();
                    output.push_str("}\n");
                }

                match fallback.as_ref() {
                    Decision::Unreachable => {}
                    _ => self.emit_guarded_tree(output, fallback, ctx),
                }
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
