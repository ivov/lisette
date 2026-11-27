use crate::output::render::types::go_type_text;
use crate::output::render::writer::GoWriter;
use crate::target::ir::bodies::{
    AssignForm, CompoundKind, ConstPlan, Definition, ElseArm, ElseLayout, IfPlan, LoopHeader,
    LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement, SelectArmKind, SelectStatementPlan,
    Statement, SwitchKind, SwitchStatementPlan,
};
use crate::target::ir::go_expression::GoLiteral;
use crate::target::ir::go_expression::Position;
use crate::target::ir::go_expression::{
    CompositeLayout, FunctionLiteralLayout, GoExpressionNode, TreeVisitor,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;
use std::fmt::Write;

macro_rules! write_line {
    ($dst:expr, $($arg:tt)*) => {
        { use std::fmt::Write as _; writeln!($dst, $($arg)*).unwrap() }
    };
}

fn write_header_line(output: &mut GoWriter, line: Option<&Position>) {
    if let Some(line) = line {
        output.push_line_directive(line);
    }
}

fn names<T: ToString>(names: &[T]) -> String {
    names
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn render_lowered_block(output: &mut GoWriter, block: &LoweredBlock) {
    for statement in &block.statements {
        render_statement(output, statement);
    }
}

fn render_select(output: &mut GoWriter, plan: &SelectStatementPlan) {
    output.push_str("select {\n");
    for arm in &plan.arms {
        write_header_line(output, arm.line.as_ref());
        render_select_arm(output, &arm.kind);
    }
    output.push_str("}\n");
}

fn render_switch(output: &mut GoWriter, plan: &SwitchStatementPlan) {
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
        render_lowered_block(output, &case.body);
    }
    if let Some(default_body) = &plan.default
        && !default_body.renders_empty()
    {
        write_header_line(output, plan.default_line.as_ref());
        output.push_str("default:\n");
        render_lowered_block(output, default_body);
    }
    output.push_str("}\n");
}

fn render_select_arm(output: &mut GoWriter, arm: &SelectArmKind) {
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
            render_lowered_block(output, body);
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
            render_lowered_block(output, body);
        }
        SelectArmKind::Default { body } => {
            output.push_str("default:\n");
            render_lowered_block(output, body);
        }
    }
}

fn render_definition(output: &mut GoWriter, definition: &Definition) {
    output.push_str(&names(&definition.names));
    output.push_str(" := ");
    definition.value.write(output);
}

fn write_statement_line(output: &mut GoWriter, statement: &Statement) {
    if let Some(line) = &statement.line
        && statement.kind.emits_output()
        && !matches!(&statement.kind, LoweredStatement::Body(body)
            if body.statements.first().is_some_and(|first| first.line.is_some()))
    {
        output.push_line_directive(line);
    }
}

fn render_statement(output: &mut GoWriter, statement: &Statement) {
    write_statement_line(output, statement);
    match &statement.kind {
        LoweredStatement::If(plan) => render_if(output, plan),
        LoweredStatement::Loop(plan) => render_loop(output, plan, statement.line.as_ref()),
        LoweredStatement::Block(body) => {
            output.push_str("{\n");
            render_lowered_block(output, body);
            output.push_str("}\n");
        }
        LoweredStatement::Body(body) => render_lowered_block(output, body),
        LoweredStatement::Break(target) => render_transfer(output, "break", target),
        LoweredStatement::Continue(target) => render_transfer(output, "continue", target),
        LoweredStatement::Const(plan) => {
            render_const_declaration(output, plan);
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
            render_assign_statement(output, plan);
        }
        LoweredStatement::Async { kind, call } => {
            output.push_str(kind.keyword());
            output.push(' ');
            call.write(output);
            output.push('\n');
        }
        LoweredStatement::Select(plan) => render_select(output, plan),
        LoweredStatement::Switch(plan) => render_switch(output, plan),
        LoweredStatement::Define(definition) => {
            render_definition(output, definition);
            output.push('\n');
        }
        LoweredStatement::VarDeclaration {
            name,
            go_type,
            value,
        } => render_var_declaration(output, name, go_type, value.as_ref()),
        LoweredStatement::Discard { arity, value } => {
            output.push_str(&vec!["_"; *arity].join(", "));
            output.push_str(" = ");
            value.write(output);
            output.push('\n');
        }
        LoweredStatement::ExpressionStatement { expression, .. } => {
            expression.write(output);
            output.push('\n');
        }
        LoweredStatement::UnreachablePanic => output.push_str("panic(\"unreachable\")\n"),
    }
}

fn render_var_declaration(
    output: &mut GoWriter,
    name: &GoIdentifier,
    go_type: &GoType,
    value: Option<&GoExpression>,
) {
    match value {
        Some(value) => {
            write!(output, "var {} {} = ", name, go_type_text(go_type)).unwrap();
            value.write(output);
            output.push('\n');
        }
        None => write_line!(output, "var {} {}", name, go_type_text(go_type)),
    }
}

fn render_assign_statement(output: &mut GoWriter, plan: &AssignForm) {
    match plan {
        AssignForm::Compound { target, kind } => {
            target.write(output);
            match kind {
                CompoundKind::Increment => output.push_str("++"),
                CompoundKind::Decrement => output.push_str("--"),
                CompoundKind::OperatorAssign { operator, rhs } => {
                    write!(output, " {}= ", operator.symbol()).unwrap();
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

fn render_transfer(output: &mut GoWriter, keyword: &str, target: &LoopTransfer) {
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

fn render_const_declaration(output: &mut GoWriter, plan: &ConstPlan) {
    output.push_str("const ");
    output.push_str(&plan.name);
    if let Some(go_type) = &plan.go_type {
        output.push(' ');
        output.push_str(&go_type_text(go_type));
    }
    output.push_str(" = ");
    plan.value.write(output);
    output.push('\n');
}

fn render_loop(output: &mut GoWriter, plan: &LoopPlan, line: Option<&Position>) {
    if let Some(label) = plan.kind.label() {
        write_line!(output, "{}:", label);
        write_header_line(output, line);
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
    render_lowered_block(output, &plan.body);
    output.push_str("}\n");
}

fn render_if_header(output: &mut GoWriter, plan: &IfPlan) {
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

fn render_if(output: &mut GoWriter, plan: &IfPlan) {
    render_if_header(output, plan);
    render_lowered_block(output, &plan.then_body);
    render_else_arm(output, &plan.else_arm);
}

fn render_else_arm(output: &mut GoWriter, arm: &ElseArm) {
    match arm {
        ElseArm::None => output.push_str("}\n"),
        ElseArm::ElseIf(plan) => {
            write_header_line(output, plan.else_if_line.as_ref());
            output.push_str("} else ");
            render_if_header(output, plan);
            render_lowered_block(output, &plan.then_body);
            render_else_arm(output, &plan.else_arm);
        }
        ElseArm::Else { body, layout } => {
            debug_assert!(!body.renders_empty(), "else body must render output");
            match layout {
                ElseLayout::Inline => {
                    output.push_str("}\n");
                    render_lowered_block(output, body);
                }
                ElseLayout::Block => {
                    output.push_str("} else {\n");
                    render_lowered_block(output, body);
                    output.push_str("}\n");
                }
            }
        }
    }
}

pub(crate) fn can_inline_block(block: &LoweredBlock) -> bool {
    struct InlineEligibility {
        inline: bool,
    }

    impl TreeVisitor for InlineEligibility {
        fn statement(&mut self, statement: &LoweredStatement) {
            match statement {
                LoweredStatement::If(_)
                | LoweredStatement::Loop(_)
                | LoweredStatement::Block(_)
                | LoweredStatement::Select(_)
                | LoweredStatement::Switch(_) => self.inline = false,
                LoweredStatement::Body(body) if body.statements.len() > 1 => self.inline = false,
                LoweredStatement::Body(_)
                | LoweredStatement::Break(_)
                | LoweredStatement::Continue(_)
                | LoweredStatement::Const(_)
                | LoweredStatement::Return(_)
                | LoweredStatement::Assign(_)
                | LoweredStatement::Async { .. }
                | LoweredStatement::Define(_)
                | LoweredStatement::VarDeclaration { .. }
                | LoweredStatement::Discard { .. }
                | LoweredStatement::ExpressionStatement { .. }
                | LoweredStatement::UnreachablePanic => {}
            }
        }

        fn expression(&mut self, expression: &GoExpressionNode) {
            match expression {
                GoExpressionNode::CompositeLiteral {
                    layout: CompositeLayout::MultiLine { .. },
                    ..
                }
                | GoExpressionNode::FunctionLiteral {
                    layout: FunctionLiteralLayout::MultiLine,
                    ..
                } => self.inline = false,
                GoExpressionNode::Literal(GoLiteral::Token(text))
                | GoExpressionNode::Verbatim(text)
                    if text.contains('\n') =>
                {
                    self.inline = false
                }
                GoExpressionNode::Identifier(_)
                | GoExpressionNode::Qualified { .. }
                | GoExpressionNode::Literal(_)
                | GoExpressionNode::Type(_)
                | GoExpressionNode::CompositeLiteral { .. }
                | GoExpressionNode::Call { .. }
                | GoExpressionNode::Instantiation { .. }
                | GoExpressionNode::Selector { .. }
                | GoExpressionNode::Index { .. }
                | GoExpressionNode::Slice { .. }
                | GoExpressionNode::TypeAssertion { .. }
                | GoExpressionNode::Unary { .. }
                | GoExpressionNode::AddressOf(_)
                | GoExpressionNode::Dereference(_)
                | GoExpressionNode::Binary { .. }
                | GoExpressionNode::Conversion { .. }
                | GoExpressionNode::Spread(_)
                | GoExpressionNode::FunctionLiteral { .. }
                | GoExpressionNode::Verbatim(_)
                | GoExpressionNode::Unreachable => {}
            }
        }
    }
    let mut eligibility = InlineEligibility { inline: true };
    block.walk(&mut eligibility);
    eligibility.inline
}
