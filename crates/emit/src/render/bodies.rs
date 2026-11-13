use crate::plan::bodies::{
    AssignForm, CompoundKind, ConstPlan, Definition, ElseArm, IfPlan, LoopHeader, LoopPlan,
    LoopTransfer, LoweredBlock, LoweredStatement, SelectArmKind, SelectStatementPlan, Statement,
    SwitchKind, SwitchStatementPlan,
};
use crate::plan::values::GoExpression;
use crate::render::Renderer;
use crate::write_line;

fn write_header_line(output: &mut String, line: Option<&String>) {
    if let Some(line) = line
        && !output.ends_with(line.as_str())
    {
        output.push_str(line);
    }
}

fn names<T: ToString>(names: &[T]) -> String {
    names
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl Renderer {
    /// Render a slice of setup statements to a fresh `String`.
    pub(crate) fn render_setup(&self, setup: &[Statement]) -> String {
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

    fn render_select(&self, output: &mut String, plan: &SelectStatementPlan) {
        output.push_str("select {\n");
        for arm in &plan.arms {
            write_header_line(output, arm.line.as_ref());
            self.render_select_arm(output, &arm.kind);
        }
        output.push_str("}\n");
    }

    /// Render a `switch` statement: the value/type-switch header, each
    /// `case`/`default:` plus body, and the closing brace.
    fn render_switch(&self, output: &mut String, plan: &SwitchStatementPlan) {
        match &plan.kind {
            SwitchKind::Conditional => output.push_str("switch {\n"),
            SwitchKind::Value { subject } => {
                output.push_str("switch ");
                subject.write_header(output);
                output.push_str(" {\n");
            }
            SwitchKind::Type { subject, binding } => {
                output.push_str("switch ");
                if let Some(binding) = binding {
                    output.push_str(binding);
                    output.push_str(" := ");
                }
                subject.write_header(output);
                output.push_str(".(type) {\n");
            }
        }
        for case in &plan.cases {
            write_header_line(output, case.line.as_ref());
            output.push_str("case ");
            GoExpression::write_list(&case.labels, output);
            output.push_str(":\n");
            self.render_lowered_block(output, &case.body);
        }
        if let Some(default_body) = &plan.default
            && !default_body.renders_empty()
        {
            write_header_line(output, plan.default_line.as_ref());
            output.push_str("default:\n");
            self.render_lowered_block(output, default_body);
        }
        output.push_str("}\n");
    }

    fn render_select_arm(&self, output: &mut String, arm: &SelectArmKind) {
        match arm {
            SelectArmKind::Receive {
                receive_vars,
                channel,
                body,
            } => {
                output.push_str("case ");
                if !receive_vars.is_empty() {
                    output.push_str(&names(receive_vars));
                    output.push_str(" := ");
                }
                output.push_str("<-");
                channel.write(output);
                output.push_str(":\n");
                self.render_lowered_block(output, body);
            }
            SelectArmKind::Send {
                channel,
                value,
                body,
            } => {
                output.push_str("case ");
                channel.write(output);
                output.push_str(" <- ");
                value.write(output);
                output.push_str(":\n");
                self.render_lowered_block(output, body);
            }
            SelectArmKind::Default { body } => {
                output.push_str("default:\n");
                self.render_lowered_block(output, body);
            }
        }
    }

    fn render_definition(&self, output: &mut String, definition: &Definition) {
        output.push_str(&names(&definition.names));
        output.push_str(" := ");
        definition.value.write(output);
    }

    fn render_statement(&self, output: &mut String, statement: &Statement) {
        if let Some(line) = &statement.line
            && !matches!(&statement.kind, LoweredStatement::Body(body)
                if body.statements.first().is_some_and(|first| first.line.is_some()))
            && !output.ends_with(line.as_str())
        {
            output.push_str(line);
        }
        match &statement.kind {
            LoweredStatement::If(plan) => self.render_if(output, plan),
            LoweredStatement::Loop(plan) => self.render_loop(output, plan, statement.line.as_ref()),
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
            LoweredStatement::Return(values) => {
                output.push_str("return");
                if !values.is_empty() {
                    output.push(' ');
                    GoExpression::write_list(values, output);
                }
                output.push('\n');
            }
            LoweredStatement::Assign(plan) => {
                self.render_assign_statement(output, plan);
            }
            LoweredStatement::Async { kind, call } => {
                output.push_str(kind.keyword());
                output.push(' ');
                call.write(output);
                output.push('\n');
            }
            LoweredStatement::Select(plan) => self.render_select(output, plan),
            LoweredStatement::Switch(plan) => self.render_switch(output, plan),
            LoweredStatement::Define(definition) => {
                self.render_definition(output, definition);
                output.push('\n');
            }
            LoweredStatement::VarDecl {
                name,
                go_type,
                value,
            } => match value {
                Some(value) => {
                    output.push_str(&format!("var {} {} = ", name, go_type.code));
                    value.write(output);
                    output.push('\n');
                }
                None => write_line!(output, "var {} {}", name, go_type.code),
            },
            LoweredStatement::Discard { arity, value } => {
                output.push_str(&vec!["_"; *arity].join(", "));
                output.push_str(" = ");
                value.write(output);
                output.push('\n');
            }
            LoweredStatement::ExpressionStatement { expression, .. } => {
                if !expression.is_empty() {
                    expression.write(output);
                    output.push('\n');
                }
            }
            LoweredStatement::UnreachablePanic => output.push_str("panic(\"unreachable\")\n"),
        }
    }

    fn render_assign_statement(&self, output: &mut String, plan: &AssignForm) {
        match plan {
            AssignForm::Compound { target, kind } => {
                target.write(output);
                match kind {
                    CompoundKind::Increment => output.push_str("++"),
                    CompoundKind::Decrement => output.push_str("--"),
                    CompoundKind::OpAssign { operator, rhs } => {
                        output.push_str(&format!(" {}= ", operator.symbol()));
                        rhs.write(output);
                    }
                }
            }
            AssignForm::Simple { target, value } => {
                target.write(output);
                output.push_str(" = ");
                value.write(output);
            }
        }
        output.push('\n');
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

    fn render_const_declaration(&self, output: &mut String, plan: &ConstPlan) {
        output.push_str("const ");
        output.push_str(&plan.name);
        if let Some(go_type) = &plan.go_type {
            output.push(' ');
            output.push_str(&go_type.code);
        }
        output.push_str(" = ");
        plan.value.write(output);
        output.push('\n');
    }

    fn render_loop(&self, output: &mut String, plan: &LoopPlan, line: Option<&String>) {
        if let Some(label) = plan.kind.label() {
            write_line!(output, "{}:", label);
            if let Some(line) = line {
                output.push_str(line);
            }
        }
        match &plan.header {
            LoopHeader::Infinite => output.push_str("for {\n"),
            LoopHeader::While(condition) => {
                output.push_str("for ");
                condition.write_header(output);
                output.push_str(" {\n");
            }
            LoopHeader::Range {
                key,
                value,
                iterable,
            } => {
                match (key, value) {
                    (None, None) => output.push_str("for range "),
                    (Some(key), None) => output.push_str(&format!("for {} := range ", key)),
                    (key, Some(value)) => output.push_str(&format!(
                        "for {}, {} := range ",
                        key.as_deref().unwrap_or("_"),
                        value
                    )),
                }
                iterable.write_header(output);
                output.push_str(" {\n");
            }
            LoopHeader::Counted {
                variable,
                start,
                condition,
            } => {
                output.push_str(&format!("for {} := ", variable));
                start.write_header(output);
                output.push_str("; ");
                if let Some(condition) = condition {
                    condition.write_header(output);
                }
                output.push_str(&format!("; {}++ {{\n", variable));
            }
        }
        self.render_lowered_block(output, &plan.body);
        output.push_str("}\n");
    }

    fn render_if_header(&self, output: &mut String, plan: &IfPlan) {
        debug_assert!(!plan.condition.is_empty(), "if condition must not be empty");
        output.push_str("if ");
        if let Some(initializer) = &plan.initializer {
            output.push_str(&names(&initializer.names));
            output.push_str(" := ");
            initializer.value.write_header(output);
            output.push_str("; ");
        }
        plan.condition.write_header(output);
        output.push_str(" {\n");
    }

    fn render_if(&self, output: &mut String, plan: &IfPlan) {
        self.render_if_header(output, plan);
        self.render_lowered_block(output, &plan.then_body);
        self.render_else_arm(output, &plan.else_arm);
    }

    fn render_else_arm(&self, output: &mut String, arm: &ElseArm) {
        match arm {
            ElseArm::None => output.push_str("}\n"),
            ElseArm::ElseIf(plan) => {
                if let Some(line) = &plan.else_if_line {
                    output.push_str(line);
                }
                output.push_str("} else ");
                self.render_if_header(output, plan);
                self.render_lowered_block(output, &plan.then_body);
                self.render_else_arm(output, &plan.else_arm);
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
}
