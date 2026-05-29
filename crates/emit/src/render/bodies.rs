use crate::plan::bodies::{
    AssignForm, AssignPlan, BreakValueDisposition, BreakValuePlan, CompoundKind, ConstPlan,
    ElseArm, ExpressionStatementForm, ExpressionStatementPlan, IfPlan, LetForm, LetPlan, LoopPlan,
    LoweredBlock, LoweredStatement, ReturnForm, ReturnStatementPlan, SelectArmPlan,
    SelectStatementPlan, SwitchKind, SwitchStatementPlan,
};
#[cfg(debug_assertions)]
use crate::plan::invariants;
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
        #[cfg(debug_assertions)]
        {
            let issues = invariants::validate(block);
            debug_assert!(
                issues.is_empty(),
                "LoweredBlock invariant violations: {:#?}",
                issues
            );
        }
        for statement in &block.statements {
            self.render_statement(output, statement);
        }
    }

    /// True when `block` renders to no output. Some structurally non-empty
    /// statements (e.g. a side-effect-free discard `let _`) render empty, so
    /// callers that gate scaffolding on emptiness query this rather than
    /// `LoweredBlock::is_empty`.
    pub(crate) fn renders_empty(&self, block: &LoweredBlock) -> bool {
        let mut buffer = String::new();
        self.render_lowered_block(&mut buffer, block);
        buffer.is_empty()
    }

    /// Render a `select` statement: optional retry-loop framing around the
    /// `select { ... }`, its arms, and any trailing postlude.
    fn render_select(&self, output: &mut String, plan: &SelectStatementPlan) {
        output.push_str(&plan.directive);
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
        output.push_str(&plan.directive);
        match &plan.kind {
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
                write_line!(output, "case {}:", operation);
                self.render_lowered_block(output, body);
            }
            SelectArmPlan::Default { body } => {
                output.push_str("default:\n");
                self.render_lowered_block(output, body);
            }
        }
    }

    pub(super) fn render_statement(&self, output: &mut String, statement: &LoweredStatement) {
        match statement {
            LoweredStatement::If(plan) => self.render_if(output, plan),
            LoweredStatement::Loop(plan) => self.render_loop(output, plan),
            LoweredStatement::Block(body) => {
                output.push_str("{\n");
                self.render_lowered_block(output, body);
                output.push_str("}\n");
            }
            LoweredStatement::Break { directive, label } => {
                output.push_str(directive);
                match label {
                    Some(label) => write_line!(output, "break {}", label),
                    None => output.push_str("break\n"),
                }
            }
            LoweredStatement::Continue { directive, label } => {
                output.push_str(directive);
                match label {
                    Some(label) => write_line!(output, "continue {}", label),
                    None => output.push_str("continue\n"),
                }
            }
            LoweredStatement::Const(plan) => {
                output.push_str(&plan.directive);
                self.render_const_declaration(output, plan);
            }
            LoweredStatement::Return(plan) => {
                output.push_str(&plan.directive);
                self.render_return_statement(output, plan);
            }
            LoweredStatement::BreakValue(plan) => {
                output.push_str(&plan.directive);
                self.render_break_value(output, plan);
            }
            LoweredStatement::Let(plan) => {
                output.push_str(&plan.directive);
                self.render_let_statement(output, plan);
            }
            LoweredStatement::Assign(plan) => {
                output.push_str(&plan.directive);
                self.render_assign_statement(output, plan);
            }
            LoweredStatement::Expression(plan) => {
                output.push_str(&plan.directive);
                self.render_expression_statement(output, plan);
            }
            LoweredStatement::Match(plan) => {
                output.push_str(&plan.directive);
                self.render_lowered_block(output, &plan.body);
            }
            LoweredStatement::Select(plan) => self.render_select(output, plan),
            LoweredStatement::Switch(plan) => self.render_switch(output, plan),
            LoweredStatement::WhileLet(plan) => {
                output.push_str(&plan.directive);
                self.render_lowered_block(output, &plan.body);
            }
            LoweredStatement::TempBind { name, value } => {
                write_line!(output, "{} := {}", name, value);
            }
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
            LoweredStatement::RawGo(code) => output.push_str(code),
        }
    }

    /// Render an `ExpressionStatementPlan`. The `Async` form flushes the value's
    /// setup, then emits the value as its own statement line (skipped when
    /// the value text is empty); the `Other` form is dumped via
    /// `render_lowered_block`.
    pub(crate) fn render_expression_statement(
        &self,
        output: &mut String,
        plan: &ExpressionStatementPlan,
    ) {
        match &plan.form {
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
            ExpressionStatementForm::Propagate { body }
            | ExpressionStatementForm::Discard { body } => {
                self.render_lowered_block(output, body);
            }
        }
    }

    /// Render a `LetPlan`. The `Never` form emits the optional `var X T`
    /// declaration line first; every form then renders its body block.
    pub(crate) fn render_let_statement(&self, output: &mut String, plan: &LetPlan) {
        if let LetForm::Never {
            declaration: Some(declaration),
            ..
        } = &plan.form
        {
            output.push_str(declaration);
        }
        self.render_lowered_block(output, plan.form.body());
    }

    /// Render an `AssignPlan`. The `Compound` form composes `target++`,
    /// `target--`, or `target op= rhs` after flushing target capture and
    /// RHS setup; the `Other` form is dumped via `render_lowered_block`.
    pub(crate) fn render_assign_statement(&self, output: &mut String, plan: &AssignPlan) {
        match &plan.form {
            AssignForm::Compound {
                target_capture,
                target_str,
                kind,
            } => {
                self.render_capture_statements(output, target_capture);
                match kind {
                    CompoundKind::Increment => write_line!(output, "{}++", target_str),
                    CompoundKind::Decrement => write_line!(output, "{}--", target_str),
                    CompoundKind::OpAssign { op_text, rhs } => {
                        let rhs_text = self.render_value(output, rhs);
                        write_line!(output, "{} {}= {}", target_str, op_text, rhs_text);
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
            AssignForm::NilClear {
                target_capture,
                target_str,
            } => {
                self.render_capture_statements(output, target_capture);
                write_line!(output, "{} = nil", target_str);
            }
            AssignForm::Discard { body } | AssignForm::NeverTyped { body } => {
                self.render_lowered_block(output, body);
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

    /// Render a `ReturnStatementPlan`. The `Plain` form flushes the value's
    /// setup, then writes `return <value>`. The `Other` form (unit return,
    /// lowered-ABI tail return, fallible-wrapped return, propagate-as-
    /// return) is dumped via `render_lowered_block`.
    pub(crate) fn render_return_statement(&self, output: &mut String, plan: &ReturnStatementPlan) {
        match &plan.form {
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
            ReturnForm::LoweredAbi { body } | ReturnForm::Wrapped { body } => {
                self.render_lowered_block(output, body);
            }
        }
    }

    /// Render a `BreakValuePlan`: flush the value's setup, apply the
    /// disposition (assign to result slot / unit-call side effect + unit
    /// assign / discard / no-op when diverged), then emit `break [label]`
    /// (skipped when diverged).
    pub(crate) fn render_break_value(&self, output: &mut String, plan: &BreakValuePlan) {
        let value_text = self.render_value(output, &plan.value);
        match &plan.disposition {
            BreakValueDisposition::Diverged => return,
            BreakValueDisposition::UnitCallIntoResult { result_var } => {
                if !value_text.is_empty() {
                    write_line!(output, "{}", value_text);
                }
                write_line!(output, "{} = struct{{}}{{}}", result_var);
            }
            BreakValueDisposition::AssignToResult { result_var } => {
                if !value_text.is_empty() {
                    write_line!(output, "{} = {}", result_var, value_text);
                }
            }
            BreakValueDisposition::Discard => {
                if !value_text.is_empty() {
                    write_line!(output, "_ = {}", value_text);
                }
            }
        }
        match &plan.label {
            Some(label) => write_line!(output, "break {}", label),
            None => output.push_str("break\n"),
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
        output.push_str(&plan.directive);
        output.push_str(&plan.prologue);
        if let Some(label) = &plan.label {
            write_line!(output, "{}:", label);
        }
        output.push_str(&plan.header);
        self.render_lowered_block(output, &plan.body);
        output.push_str("}\n");
    }

    fn render_if(&self, output: &mut String, plan: &IfPlan) {
        output.push_str(&plan.directive);
        output.push_str(&plan.condition_setup);
        write_line!(output, "if {} {{", plan.condition);
        self.render_lowered_block(output, &plan.then_body);
        self.render_else_arm(output, &plan.else_arm);
    }

    fn render_else_arm(&self, output: &mut String, arm: &ElseArm) {
        match arm {
            ElseArm::None => output.push_str("}\n"),
            ElseArm::ElseIf(plan) => {
                if !plan.condition_setup.is_empty() {
                    output.push_str("} else {\n");
                    output.push_str(&plan.condition_setup);
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
    /// value text. Data-driven counterpart of `emit_value`.
    pub(crate) fn render_value(&self, output: &mut String, plan: &ValuePlan) -> String {
        match plan {
            ValuePlan::Operand(value) => value.clone(),
            ValuePlan::Composite { setup, value } => {
                for statement in setup {
                    self.render_statement(output, statement);
                }
                value.clone()
            }
            ValuePlan::Paren(inner) => {
                let inner_text = self.render_value(output, inner);
                format!("({})", inner_text)
            }
            ValuePlan::Cast { go_type, inner } => {
                let inner_text = self.render_value(output, inner);
                format!("{}({})", go_type, inner_text)
            }
            ValuePlan::Unary { op, inner } => {
                let inner_text = self.render_value(output, inner);
                format!("{}{}", op, inner_text)
            }
        }
    }
}
