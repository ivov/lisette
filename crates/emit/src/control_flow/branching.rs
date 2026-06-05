use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::write_line;
use syntax::ast::Expression;

impl Planner<'_> {
    /// `if {`, `} else if {`, or `} else {` for the next link in a pattern
    /// chain; enters a new scope after.
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

    /// String-context bridge over `lower_block_as_body`.
    pub(crate) fn emit_block(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) {
        let body = self.lower_block_as_body(expression, fx);
        Renderer.render_lowered_block(output, &body);
    }
}

impl Planner<'_> {
    pub(crate) fn emit_labeled_loop(
        &mut self,
        output: &mut String,
        header: &str,
        body: &Expression,
        needs_label: bool,
        fx: &mut EmitEffects,
    ) {
        self.set_current_loop_label_if_needed(needs_label);
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }
        output.push_str(header);
        self.enter_scope();
        self.emit_block(output, body, fx);
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
