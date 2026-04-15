use crate::Emitter;
use crate::go::names::go_name;
use crate::go::patterns::decision_tree;
use crate::go::statements::assignments::is_lvalue_chain;
use crate::go::types::emitter::Position;
use crate::go::utils::output_ends_with_diverge;
use crate::go::write_line;
use syntax::ast::{Expression, Pattern, TypedPattern};

impl Emitter<'_> {
    pub(crate) fn emit_if(
        &mut self,
        output: &mut String,
        condition: &Expression,
        consequence: &Expression,
        alternative: &Expression,
    ) {
        let condition_string = self.emit_condition_operand(output, condition);
        let condition_string = wrap_if_struct_literal(condition_string);
        write_line!(output, "if {} {{", condition_string);
        self.enter_scope();
        self.emit_in_position(output, consequence);
        self.exit_scope();
        self.emit_else_chain(output, alternative);
    }

    fn emit_else_chain(&mut self, output: &mut String, alternative: &Expression) {
        let is_empty_alternative = match alternative {
            Expression::Unit { .. } => true,
            Expression::Block { items, .. } => items.is_empty(),
            _ => false,
        };
        if is_empty_alternative {
            output.push_str("}\n");
            return;
        }

        if let Expression::If {
            condition,
            consequence,
            alternative: next_alternative,
            ..
        } = alternative
        {
            let condition_string = self.emit_condition_operand(output, condition);
            let condition_string = wrap_if_struct_literal(condition_string);
            write_line!(output, "}} else if {} {{", condition_string);
            self.enter_scope();
            self.emit_in_position(output, consequence);
            self.exit_scope();
            self.emit_else_chain(output, next_alternative);
        } else if output_ends_with_diverge(output) {
            output.push_str("}\n");
            self.emit_in_position(output, alternative);
        } else {
            output.push_str("} else {\n");
            self.enter_scope();
            self.emit_in_position(output, alternative);
            self.exit_scope();
            output.push_str("}\n");
        }
    }

    /// Emit an if/else-if branch header for pattern matching chains.
    ///
    /// When `is_first` is true, emits `if <condition> {`. Otherwise, exits the
    /// previous scope and emits `} else if <condition> {` (or `} else {` for catchalls).
    /// Always enters a new scope after the header.
    pub(crate) fn emit_branch_header(
        &mut self,
        output: &mut String,
        condition: &str,
        is_catchall: bool,
        is_first: bool,
    ) {
        if is_first {
            if is_catchall {
                output.push_str("if true {\n");
            } else {
                write_line!(output, "if {} {{", condition);
            }
        } else {
            self.exit_scope();
            if is_catchall {
                output.push_str("} else {\n");
            } else {
                write_line!(output, "}} else if {} {{", condition);
            }
        }
        self.enter_scope();
    }

    pub(crate) fn emit_while_let(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        typed_pattern: Option<&TypedPattern>,
        scrutinee: &Expression,
        body: &Expression,
        needs_label: bool,
    ) {
        self.maybe_set_loop_label(needs_label);
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }
        output.push_str("for {\n");

        let inlined = if let Expression::Identifier { value, .. } = scrutinee {
            let name = value.to_string();
            let has_collision = Self::pattern_binds_name(pattern, &name);
            if !has_collision && !name.contains('.') {
                Some(
                    self.scope
                        .bindings
                        .get(&name)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| go_name::escape_reserved(&name).into_owned()),
                )
            } else {
                None
            }
        } else {
            None
        };
        let subject_var = inlined.unwrap_or_else(|| {
            let var = self.fresh_var(Some("subject"));
            let expression = self.emit_operand(output, scrutinee);
            write_line!(output, "{} := {}", var, expression);
            var
        });

        if let Pattern::Or { patterns, .. } = pattern
            && Self::pattern_has_bindings(pattern)
        {
            for (i, alternative_pattern) in patterns.iter().enumerate() {
                let (checks, bindings) =
                    decision_tree::collect_pattern_info(self, alternative_pattern, None);
                let condition = decision_tree::render_condition(&checks, &subject_var);

                self.emit_branch_header(output, &condition, false, i == 0);

                decision_tree::emit_tree_bindings(self, output, &bindings, &subject_var);
                self.emit_block(output, body);
            }

            self.emit_while_let_break_else(output);
            return;
        }

        let (checks, bindings) = decision_tree::collect_pattern_info(self, pattern, typed_pattern);
        let condition = decision_tree::render_condition(&checks, &subject_var);
        write_line!(output, "if {} {{", condition);
        self.enter_scope();

        if !matches!(pattern, Pattern::Or { .. }) {
            decision_tree::emit_tree_bindings(self, output, &bindings, &subject_var);
        }

        self.emit_block(output, body);

        self.emit_while_let_break_else(output);
    }

    fn emit_while_let_break_else(&mut self, output: &mut String) {
        self.exit_scope();
        output.push_str("} else {\n");
        self.enter_scope();
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "break {}", label);
        } else {
            output.push_str("break\n");
        }
        self.exit_scope();
        output.push_str("}\n");
        output.push_str("}\n");
    }

    pub(crate) fn emit_branching_directly(&mut self, output: &mut String, expression: &Expression) {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.emit_if(output, condition, consequence, alternative);
            }
            Expression::Match {
                subject, arms, ty, ..
            } => {
                self.emit_match(output, subject, arms, ty);
            }
            Expression::Select { arms, .. } => {
                self.emit_select(output, arms);
            }
            _ => unreachable!("expected if/match/select"),
        }
    }

    pub(crate) fn emit_block(&mut self, output: &mut String, expression: &Expression) {
        let Expression::Block { items, .. } = expression else {
            self.emit_statement(output, expression);
            return;
        };

        for item in items {
            self.emit_statement(output, item);
        }
    }

    pub(crate) fn emit_block_to_var_with_braces(
        &mut self,
        output: &mut String,
        expression: &Expression,
        var: &str,
        has_go_braces: bool,
    ) {
        let is_block = matches!(expression, Expression::Block { .. });
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };

        if is_block {
            if has_go_braces {
                self.enter_scope();
            } else {
                self.scope.bindings.save();
            }
        }

        let Some((last, rest)) = items.split_last() else {
            if is_block {
                if has_go_braces {
                    self.exit_scope();
                } else {
                    self.scope.bindings.restore();
                }
            }
            return;
        };

        let is_new_target = self.scope.assign_targets.insert(var.to_string());

        for item in rest {
            self.emit_statement(output, item);
        }

        if matches!(
            last,
            Expression::Return { .. }
                | Expression::Break { .. }
                | Expression::Continue { .. }
                | Expression::Let { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::For { .. }
                | Expression::Const { .. }
        ) {
            self.emit_statement(output, last);
        } else if last.get_type().is_never() {
            // Never-typed expressions (e.g. panic(), blocks ending in
            // break/continue/return) don't produce a value. Emit as a
            // statement to avoid creating unused temp variables.
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
        } else if last.get_type().resolve().is_unit()
            && matches!(last.unwrap_parens(), Expression::Call { .. })
        {
            // Emit as statement and assign struct{}{} to the block result var.
            let call_str = self.emit_value(output, last);
            if !call_str.is_empty() {
                write_line!(output, "{call_str}");
            }
            write_line!(output, "{var} = struct{{}}{{}}");
        } else if !self.emit_append_to_var(output, var, last) {
            match last {
                Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                    self.with_position(Position::Assign(var.to_string()), |this| {
                        this.emit_branching_directly(output, last);
                    });
                }
                _ => {
                    let expression_string = self.emit_value(output, last);
                    let expression_string = self.adapt_to_assign_target(last, expression_string);
                    write_line!(output, "{} = {}", var, expression_string);
                }
            }
        }

        if is_new_target {
            self.scope.assign_targets.remove(var);
        }

        if is_block {
            if has_go_braces {
                self.exit_scope();
            } else {
                self.scope.bindings.restore();
            }
        }
    }

    fn emit_append_to_var(&mut self, output: &mut String, var: &str, last: &Expression) -> bool {
        let Expression::Call {
            expression: func,
            args,
            ..
        } = last
        else {
            return false;
        };
        if !self.is_slice_append_or_extend(func) {
            return false;
        }

        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = func.as_ref()
        else {
            return true;
        };

        let is_extend = member == "extend";
        let unwrapped = receiver.unwrap_parens();
        let receiver_is_lvalue =
            is_lvalue_chain(unwrapped) && !self.contains_newtype_access(unwrapped);

        if receiver_is_lvalue {
            // false: append args never produce RHS temp statements (if/match/block).
            let receiver_lv = self.emit_left_value_capturing(output, unwrapped, false);
            let args_str = self.emit_append_args(output, args, is_extend);
            write_line!(output, "{} = append({}, {})", var, receiver_lv, args_str);
        } else {
            let value_str = self.emit_value(output, last);
            write_line!(output, "{} = {}", var, value_str);
        }

        true
    }

    pub(crate) fn emit_block_to_tail(&mut self, output: &mut String, expression: &Expression) {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };

        let Some((last, rest)) = items.split_last() else {
            return;
        };

        for item in rest {
            self.emit_statement(output, item);
        }

        let return_span = last.get_span();

        let last = if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        };

        if last.get_type().is_unit() {
            if !matches!(last, Expression::Unit { .. }) {
                self.emit_statement(output, last);
            }
            return;
        }

        if last.get_type().is_never() {
            let directive = self.maybe_line_directive(&return_span);
            output.push_str(&directive);
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }

        let directive = self.maybe_line_directive(&return_span);
        match last {
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                output.push_str(&directive);
                self.emit_branching_directly(output, last);
            }
            _ => {
                let expression_string = self.emit_value(output, last);
                let expression_string = self.adapt_return_to_context(last, expression_string);
                write_line!(output, "{}return {}", directive, expression_string);
            }
        }
    }

    pub(crate) fn adapt_to_assign_target(
        &mut self,
        expression: &Expression,
        emitted: String,
    ) -> String {
        let Some(target) = self.assign_target_ty.clone() else {
            return emitted;
        };
        self.maybe_wrap_as_go_interface(emitted, &expression.get_type(), &target)
    }

    pub(crate) fn emit_in_position(&mut self, output: &mut String, expression: &Expression) {
        match &self.position {
            Position::Statement | Position::Expression => {
                self.emit_block(output, expression);
            }
            Position::Assign(var) => {
                let var = var.clone();
                if expression.get_type().is_result() || expression.get_type().is_option() {
                    self.emit_option_result_assignment(output, &var, expression);
                } else {
                    self.emit_block_to_var_with_braces(output, expression, &var, false);
                }
            }
            Position::Tail => self.emit_block_to_tail(output, expression),
        }
    }
}

impl Emitter<'_> {
    pub(crate) fn maybe_set_loop_label(&mut self, needs_label: bool) {
        if needs_label {
            let label = self.fresh_var(Some("loop"));
            if let Some(ctx) = self.scope.loop_stack.last_mut() {
                ctx.label = Some(label);
            }
        }
    }

    pub(crate) fn emit_labeled_loop(
        &mut self,
        output: &mut String,
        header: &str,
        body: &Expression,
        needs_label: bool,
    ) {
        self.maybe_set_loop_label(needs_label);
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }
        output.push_str(header);
        self.enter_scope();
        self.emit_block(output, body);
        self.exit_scope();
        output.push_str("}\n");
    }
}

pub(crate) fn wrap_if_struct_literal(condition: String) -> String {
    if condition.contains('{') {
        format!("({})", condition)
    } else {
        condition
    }
}
