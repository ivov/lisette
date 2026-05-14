use crate::Emitter;
use crate::expressions::context::ExpressionContext;
use crate::placement::BodyPlace;
use crate::utils::output_ends_with_diverge;
use crate::write_line;
use syntax::ast::Expression;

impl Emitter<'_> {
    pub(crate) fn emit_if(
        &mut self,
        output: &mut String,
        condition: &Expression,
        consequence: &Expression,
        alternative: &Expression,
        place: &BodyPlace,
    ) {
        let condition_string =
            self.emit_operand(output, condition, ExpressionContext::value().condition());
        let condition_string = wrap_if_struct_literal(condition_string);
        write_line!(output, "if {} {{", condition_string);
        self.enter_scope();
        self.emit_body_to_place(output, consequence, place);
        self.exit_scope();
        self.emit_else_chain(output, alternative, place);
    }

    fn emit_else_chain(
        &mut self,
        output: &mut String,
        alternative: &Expression,
        place: &BodyPlace,
    ) {
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
            let (setup, condition_string) = self.capture_emission(output, |this, buf| {
                this.emit_operand(buf, condition, ExpressionContext::value().condition())
            });
            let condition_string = wrap_if_struct_literal(condition_string);
            if !setup.is_empty() {
                self.emit_else_if_with_setup(
                    output,
                    &setup,
                    &condition_string,
                    consequence,
                    next_alternative,
                    place,
                );
                return;
            }
            write_line!(output, "}} else if {} {{", condition_string);
            self.enter_scope();
            self.emit_body_to_place(output, consequence, place);
            self.exit_scope();
            self.emit_else_chain(output, next_alternative, place);
        } else if output_ends_with_diverge(output) {
            output.push_str("}\n");
            self.emit_body_to_place(output, alternative, place);
        } else {
            output.push_str("} else {\n");
            self.enter_scope();
            self.emit_body_to_place(output, alternative, place);
            self.exit_scope();
            output.push_str("}\n");
        }
    }

    fn emit_else_if_with_setup(
        &mut self,
        output: &mut String,
        condition_setup: &str,
        condition_string: &str,
        consequence: &Expression,
        next_alternative: &Expression,
        place: &BodyPlace,
    ) {
        output.push_str("} else {\n");
        self.enter_scope();
        output.push_str(condition_setup);
        write_line!(output, "if {} {{", condition_string);
        self.enter_scope();
        self.emit_body_to_place(output, consequence, place);
        self.exit_scope();
        self.emit_else_chain(output, next_alternative, place);
        self.exit_scope();
        output.push_str("}\n");
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

    pub(crate) fn emit_while_let_break_else(&mut self, output: &mut String) {
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

    pub(crate) fn emit_branching_directly(
        &mut self,
        output: &mut String,
        expression: &Expression,
        place: &BodyPlace,
    ) {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.emit_if(output, condition, consequence, alternative, place);
            }
            Expression::Match { subject, arms, .. } => {
                self.emit_match(output, subject, arms, place);
            }
            Expression::Select { arms, .. } => {
                self.emit_select(output, arms, place);
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
}

impl Emitter<'_> {
    pub(crate) fn emit_labeled_loop(
        &mut self,
        output: &mut String,
        header: &str,
        body: &Expression,
        needs_label: bool,
    ) {
        self.set_current_loop_label_if_needed(needs_label);
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
