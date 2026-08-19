use crate::plan::bodies::{
    AssignForm, BreakValueAction, BreakValuePlan, CompoundKind, ConstPlan, ElseArm,
    ExpressionStatementForm, IfPlan, LetPlan, LoopPlan, LoopTransfer, LoweredBlock,
    LoweredStatement, ReturnForm, SelectArmPlan, SelectStatementPlan, SwitchKind,
    SwitchStatementPlan,
};
use crate::plan::values::ValuePlan;
use crate::render::Renderer;
use crate::write_line;

impl Renderer {
    /// Render a slice of setup statements to a fresh `String`.
    pub(crate) fn render_setup(&self, setup: &[LoweredStatement]) -> String {
        let mut buffer = String::new();
        for statement in setup {
            self.render_statement(&mut buffer, statement);
        }
        buffer
    }

    pub(crate) fn render_lowered_block(&self, output: &mut String, block: &LoweredBlock) {
        for statement in &block.statements {
            self.render_statement(output, statement);
        }
    }

    /// Render a `select` statement: optional retry-loop framing around the
    /// `select { ... }`, its arms, and any trailing postlude.
    fn render_select(&self, output: &mut String, plan: &SelectStatementPlan) {
        for statement in &plan.setup {
            self.render_statement(output, statement);
        }
        if plan.retry_loop {
            output.push_str("for {\n");
        }
        output.push_str("select {\n");
        for arm in &plan.arms {
            self.render_select_arm(output, arm);
        }
        output.push_str("}\n");
        if plan.retry_loop {
            if plan.all_arms_diverge() {
                output.push_str("}\n");
            } else {
                output.push_str("break\n}\n");
            }
        }
        for statement in &plan.postlude {
            self.render_statement(output, statement);
        }
    }

    /// Render a `switch` statement: the value/type-switch header, each
    /// `case`/`default:` plus body, the closing brace, and any postlude.
    fn render_switch(&self, output: &mut String, plan: &SwitchStatementPlan) {
        match &plan.kind {
            SwitchKind::Conditional => output.push_str("switch {\n"),
            SwitchKind::Value { subject } => write_line!(output, "switch {} {{", subject),
            SwitchKind::Type {
                subject,
                binding: Some(binding),
            } => write_line!(output, "switch {} := {}.(type) {{", binding, subject),
            SwitchKind::Type {
                subject,
                binding: None,
            } => write_line!(output, "switch {}.(type) {{", subject),
        }
        for case in &plan.cases {
            write_line!(output, "case {}:", case.labels);
            self.render_lowered_block(output, &case.body);
        }
        if let Some(default_body) = &plan.default {
            output.push_str("default:\n");
            self.render_lowered_block(output, default_body);
        }
        output.push_str("}\n");
        for statement in &plan.postlude {
            self.render_statement(output, statement);
        }
    }

    fn render_select_arm(&self, output: &mut String, arm: &SelectArmPlan) {
        match arm {
            SelectArmPlan::Receive {
                receive_vars,
                channel,
                body,
            } => {
                match receive_vars {
                    Some(vars) => write_line!(output, "case {} := <-{}:", vars, channel),
                    None => write_line!(output, "case <-{}:", channel),
                }
                self.render_lowered_block(output, body);
            }
            SelectArmPlan::Send { operation, body } => {
                write_line!(output, "case {}:", operation.rendered());
                self.render_lowered_block(output, body);
            }
            SelectArmPlan::Default { body } => {
                output.push_str("default:\n");
                self.render_lowered_block(output, body);
            }
        }
    }

    fn render_statement(&self, output: &mut String, statement: &LoweredStatement) {
        match statement {
            LoweredStatement::If(plan) => self.render_if(output, plan),
            LoweredStatement::Loop(plan) => self.render_loop(output, plan),
            LoweredStatement::Block(body) => {
                output.push_str("{\n");
                self.render_lowered_block(output, body);
                output.push_str("}\n");
            }
            LoweredStatement::Body(body) => self.render_lowered_block(output, body),
            LoweredStatement::Break(target) => self.render_transfer(output, "break", target),
            LoweredStatement::Continue(target) => self.render_transfer(output, "continue", target),
            LoweredStatement::Const(plan) => {
                self.render_const_declaration(output, plan);
            }
            LoweredStatement::Return(plan) => {
                self.render_return_statement(output, plan);
            }
            LoweredStatement::BreakValue(plan) => {
                self.render_break_value(output, plan);
            }
            LoweredStatement::Let(plan) => {
                self.render_let_statement(output, plan);
            }
            LoweredStatement::Assign(plan) => {
                self.render_assign_statement(output, plan);
            }
            LoweredStatement::Expression(plan) => {
                self.render_expression_statement(output, plan);
            }
            LoweredStatement::Select(plan) => self.render_select(output, plan),
            LoweredStatement::Switch(plan) => self.render_switch(output, plan),
            LoweredStatement::WhileLet(body) => {
                self.render_lowered_block(output, body);
            }
            LoweredStatement::TempBind { name, value } => {
                write_line!(output, "{} := {}", name, value);
            }
            LoweredStatement::VarDecl {
                name,
                go_type,
                value,
            } => match value {
                Some(value) => write_line!(output, "var {} {} = {}", name, go_type, value),
                None => write_line!(output, "var {} {}", name, go_type),
            },
            LoweredStatement::ClosureBind {
                name,
                closure_open,
                body,
                closure_close,
            } => {
                write_line!(
                    output,
                    "{} := {}",
                    name,
                    closure_open.trim_end_matches('\n')
                );
                self.render_lowered_block(output, body);
                output.push_str(closure_close);
            }
            LoweredStatement::Directed { directive, inner } => {
                output.push_str(directive);
                self.render_statement(output, inner);
            }
            LoweredStatement::RawGo(code) | LoweredStatement::DivergingRawGo(code) => {
                output.push_str(code)
            }
            LoweredStatement::UnreachablePanic => output.push_str("panic(\"unreachable\")\n"),
        }
    }

    fn render_expression_statement(&self, output: &mut String, plan: &ExpressionStatementForm) {
        match plan {
            ExpressionStatementForm::Async { value } => {
                let value_text = self.render_value(output, value);
                if !value_text.is_empty() {
                    write_line!(output, "{}", value_text);
                }
            }
            ExpressionStatementForm::AsyncBlock { keyword, body } => {
                write_line!(output, "{} func() {{", keyword);
                self.render_lowered_block(output, body);
                output.push_str("}()\n");
            }
        }
    }

    fn render_let_statement(&self, output: &mut String, plan: &LetPlan) {
        if let Some(declaration) = &plan.declaration {
            self.render_statement(output, declaration);
        }
        self.render_lowered_block(output, &plan.body);
    }

    fn render_assign_statement(&self, output: &mut String, plan: &AssignForm) {
        match plan {
            AssignForm::Compound {
                target_capture,
                target_str,
                kind,
            } => {
                self.render_capture_statements(output, target_capture);
                match kind {
                    CompoundKind::Increment => write_line!(output, "{}++", target_str),
                    CompoundKind::Decrement => write_line!(output, "{}--", target_str),
                    CompoundKind::OpAssign {
                        op_text,
                        rhs,
                        pinned_left,
                    } => {
                        let rhs_text = self.render_value(output, rhs);
                        match pinned_left {
                            Some(left) => write_line!(
                                output,
                                "{} = {} {} {}",
                                target_str,
                                left,
                                op_text,
                                rhs_text
                            ),
                            None => {
                                write_line!(output, "{} {}= {}", target_str, op_text, rhs_text)
                            }
                        }
                    }
                }
            }
            AssignForm::Simple {
                target_capture,
                target_str,
                value,
            } => {
                self.render_capture_statements(output, target_capture);
                let value_text = self.render_value(output, value);
                write_line!(output, "{} = {}", target_str, value_text);
            }
        }
    }

    /// Render a sequence of capture statements (order-sensitive lvalue
    /// setup). The statements are `RawGo` today.
    fn render_capture_statements(&self, output: &mut String, statements: &[LoweredStatement]) {
        for statement in statements {
            self.render_statement(output, statement);
        }
    }

    fn render_return_statement(&self, output: &mut String, plan: &ReturnForm) {
        match plan {
            ReturnForm::Plain { value } => {
                let value_text = self.render_value(output, value);
                write_line!(output, "return {}", value_text);
            }
            ReturnForm::Unit { side_effect } => {
                if let Some(body) = side_effect {
                    self.render_lowered_block(output, body);
                }
                output.push_str("return\n");
            }
            ReturnForm::Multi { values } => {
                write_line!(output, "return {}", values.join(", "));
            }
            ReturnForm::Body { body } => {
                self.render_lowered_block(output, body);
            }
        }
    }

    fn render_transfer(&self, output: &mut String, keyword: &str, target: &LoopTransfer) {
        match target {
            LoopTransfer::Unlabeled => write_line!(output, "{}", keyword),
            LoopTransfer::Labeled(label) => write_line!(output, "{} {}", keyword, label),
            LoopTransfer::Source(target) => {
                unreachable!(
                    "source loop transfer must be legalized before rendering: {keyword} {target:?}"
                )
            }
        }
    }

    fn render_break_value(&self, output: &mut String, plan: &BreakValuePlan) {
        match plan {
            BreakValuePlan::Diverged { value } => {
                self.render_value(output, value);
            }
            BreakValuePlan::Transfer {
                value,
                action,
                target,
            } => {
                let value_text = self.render_value(output, value);
                match action {
                    BreakValueAction::UnitCallIntoResult { result_var } => {
                        if !value_text.is_empty() {
                            write_line!(output, "{}", value_text);
                        }
                        write_line!(output, "{} = struct{{}}{{}}", result_var);
                    }
                    BreakValueAction::AssignToResult { result_var } => {
                        if !value_text.is_empty() {
                            write_line!(output, "{} = {}", result_var, value_text);
                        }
                    }
                    BreakValueAction::Discard => {
                        if !value_text.is_empty() {
                            write_line!(output, "_ = {}", value_text);
                        }
                    }
                }
                self.render_transfer(output, "break", target);
            }
        }
    }

    /// Render a `ConstPlan` as `const|var name ty = value` plus a trailing
    /// newline. The directive (if any) is emitted by the caller before this
    /// call. Setup statements that the value plan carries are flushed before
    /// the declaration line.
    pub(crate) fn render_const_declaration(&self, output: &mut String, plan: &ConstPlan) {
        let value_text = self.render_value(output, &plan.value);
        let keyword = if plan.is_const { "const" } else { "var" };
        write_line!(
            output,
            "{} {} {} = {}",
            keyword,
            plan.name,
            plan.ty_str,
            value_text
        );
    }

    fn render_loop(&self, output: &mut String, plan: &LoopPlan) {
        debug_assert!(!plan.header.is_empty(), "loop header must not be empty");
        output.push_str(&self.render_setup(&plan.prologue));
        if let Some(label) = plan.kind.label() {
            write_line!(output, "{}:", label);
        }
        output.push_str(&plan.header);
        self.render_lowered_block(output, &plan.body);
        output.push_str("}\n");
    }

    fn render_if(&self, output: &mut String, plan: &IfPlan) {
        debug_assert!(!plan.condition.is_empty(), "if condition must not be empty");
        output.push_str(&self.render_setup(&plan.condition_setup));
        write_line!(output, "if {} {{", plan.condition);
        self.render_lowered_block(output, &plan.then_body);
        self.render_else_arm(output, &plan.else_arm);
    }

    fn render_else_arm(&self, output: &mut String, arm: &ElseArm) {
        match arm {
            ElseArm::None => output.push_str("}\n"),
            ElseArm::ElseIf(plan) => {
                debug_assert!(
                    !plan.condition.is_empty(),
                    "else-if condition must not be empty"
                );
                if !plan.condition_setup.is_empty() {
                    output.push_str("} else {\n");
                    output.push_str(&self.render_setup(&plan.condition_setup));
                    write_line!(output, "if {} {{", plan.condition);
                    self.render_lowered_block(output, &plan.then_body);
                    self.render_else_arm(output, &plan.else_arm);
                    output.push_str("}\n");
                } else {
                    write_line!(output, "}} else if {} {{", plan.condition);
                    self.render_lowered_block(output, &plan.then_body);
                    self.render_else_arm(output, &plan.else_arm);
                }
            }
            ElseArm::Else { body, inline } => {
                debug_assert!(!body.renders_empty(), "else body must render output");
                if *inline {
                    output.push_str("}\n");
                    self.render_lowered_block(output, body);
                } else {
                    output.push_str("} else {\n");
                    self.render_lowered_block(output, body);
                    output.push_str("}\n");
                }
            }
        }
    }

    /// Render a value plan: emit its setup statements (if any), then return the
    /// value text.
    fn render_value(&self, output: &mut String, plan: &ValuePlan) -> String {
        for statement in &plan.setup {
            self.render_statement(output, statement);
        }
        plan.expression.rendered()
    }
}
