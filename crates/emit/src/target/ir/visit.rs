use super::bodies::{
    AssignForm, CompoundKind, Definition, ElseArm, ElseLayout, IfPlan, LoopHeader, LoopPlan,
    LoweredStatement, SelectArmKind, SelectStatementPlan, Statement, SwitchCasePlan, SwitchKind,
    SwitchStatementPlan,
};
use super::go_expression::GoExpressionNode;
use super::local::GoIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingSite {
    ShortVariable,
    Variable,
    Constant,
    RangeVariable,
    CountedVariable,
    TypeSwitchVariable,
    ReceiveVariable,
    Parameter,
}

impl BindingSite {
    pub(crate) fn must_be_read(self) -> bool {
        match self {
            Self::ShortVariable
            | Self::Variable
            | Self::RangeVariable
            | Self::TypeSwitchVariable
            | Self::ReceiveVariable => true,
            Self::Constant | Self::Parameter | Self::CountedVariable => false,
        }
    }
}

pub(crate) trait VisitorMut {
    fn expression(&mut self, node: &mut GoExpressionNode);

    fn statements(&mut self, _statements: &mut Vec<Statement>) {}

    fn enter_scope(&mut self) {}

    fn exit_scope(&mut self) {}

    fn local_binding(&mut self, _name: &mut GoIdentifier, _site: BindingSite) {}

    fn short_variable_declaration(&mut self, _names: &[GoIdentifier]) {}

    fn assignment_target(&mut self, _target: &GoExpressionNode) {}

    fn assigned_identifier(&mut self, node: &mut GoExpressionNode) {
        self.expression(node);
    }
}

pub(crate) fn visit_statements_mut(statements: &mut Vec<Statement>, visitor: &mut impl VisitorMut) {
    visitor.statements(statements);
    for statement in statements {
        visit_statement(&mut statement.kind, visitor);
    }
}

fn visit_expression(node: &mut GoExpressionNode, visitor: &mut impl VisitorMut) {
    visitor.expression(node);
    if let GoExpressionNode::FunctionLiteral {
        parameters, body, ..
    } = node
    {
        visitor.enter_scope();
        for parameter in parameters {
            visitor.local_binding(&mut parameter.name, BindingSite::Parameter);
        }
        visit_statements_mut(&mut body.statements, visitor);
        visitor.exit_scope();
    } else {
        node.visit_children_mut(&mut |child| visit_expression(child, visitor));
    }
}

fn visit_definition(definition: &mut Definition, visitor: &mut impl VisitorMut) {
    visit_expression(definition.value.node_mut(), visitor);
    visitor.short_variable_declaration(&definition.names);
    for name in &mut definition.names {
        visitor.local_binding(name, BindingSite::ShortVariable);
    }
}

fn visit_if(plan: &mut IfPlan, visitor: &mut impl VisitorMut) {
    visit_if_link(plan, visitor, 1);
}

fn visit_if_link(plan: &mut IfPlan, visitor: &mut impl VisitorMut, open: usize) {
    visitor.enter_scope();
    if let Some(initializer) = &mut plan.initializer {
        visit_definition(initializer, visitor);
    }
    visit_expression(plan.condition.node_mut(), visitor);
    visitor.enter_scope();
    visit_statements_mut(&mut plan.then_body.statements, visitor);
    visitor.exit_scope();
    match &mut plan.else_arm {
        ElseArm::None => {}
        ElseArm::ElseIf(plan) => return visit_if_link(plan, visitor, open + 1),
        ElseArm::Else {
            body,
            layout: ElseLayout::Block,
        } => {
            visitor.enter_scope();
            visit_statements_mut(&mut body.statements, visitor);
            visitor.exit_scope();
        }
        ElseArm::Else {
            body,
            layout: ElseLayout::Inline,
        } => {
            for _ in 0..open {
                visitor.exit_scope();
            }
            visit_statements_mut(&mut body.statements, visitor);
            return;
        }
    }
    for _ in 0..open {
        visitor.exit_scope();
    }
}

fn visit_case(case: &mut SwitchCasePlan, visitor: &mut impl VisitorMut) {
    for label in &mut case.labels {
        visit_expression(label.node_mut(), visitor);
    }
    visit_statements_mut(&mut case.body.statements, visitor);
}

fn visit_loop(plan: &mut LoopPlan, visitor: &mut impl VisitorMut) {
    visitor.enter_scope();
    match &mut plan.header {
        LoopHeader::Infinite => {}
        LoopHeader::While(condition) => visit_expression(condition.node_mut(), visitor),
        LoopHeader::Range {
            key,
            value,
            iterable,
        } => {
            visit_expression(iterable.node_mut(), visitor);
            for name in key.iter_mut().chain(value.iter_mut()) {
                visitor.local_binding(name, BindingSite::RangeVariable);
            }
        }
        LoopHeader::Counted {
            variable,
            start,
            condition,
        } => {
            visit_expression(start.node_mut(), visitor);
            visitor.local_binding(variable, BindingSite::CountedVariable);
            if let Some(condition) = condition {
                visit_expression(condition.node_mut(), visitor);
            }
        }
    }
    visitor.enter_scope();
    visit_statements_mut(&mut plan.body.statements, visitor);
    visitor.exit_scope();
    visitor.exit_scope();
}

fn visit_select(plan: &mut SelectStatementPlan, visitor: &mut impl VisitorMut) {
    for arm in &mut plan.arms {
        match &mut arm.kind {
            SelectArmKind::Receive {
                receive_vars,
                channel,
                body,
            } => {
                visit_expression(channel.node_mut(), visitor);
                visitor.enter_scope();
                for name in receive_vars {
                    visitor.local_binding(name, BindingSite::ReceiveVariable);
                }
                visit_statements_mut(&mut body.statements, visitor);
                visitor.exit_scope();
            }
            SelectArmKind::Send {
                channel,
                value,
                body,
            } => {
                visit_expression(channel.node_mut(), visitor);
                visit_expression(value.node_mut(), visitor);
                visitor.enter_scope();
                visit_statements_mut(&mut body.statements, visitor);
                visitor.exit_scope();
            }
            SelectArmKind::Default { body } => {
                visitor.enter_scope();
                visit_statements_mut(&mut body.statements, visitor);
                visitor.exit_scope();
            }
        }
    }
}

fn visit_switch(plan: &mut SwitchStatementPlan, visitor: &mut impl VisitorMut) {
    visitor.enter_scope();
    match &mut plan.kind {
        SwitchKind::Conditional => {}
        SwitchKind::Value { subject } => visit_expression(subject.node_mut(), visitor),
        SwitchKind::Type { subject, binding } => {
            visit_expression(subject.node_mut(), visitor);
            if let Some(name) = binding {
                visitor.local_binding(name, BindingSite::TypeSwitchVariable);
            }
        }
    }
    for case in &mut plan.cases {
        visitor.enter_scope();
        visit_case(case, visitor);
        visitor.exit_scope();
    }
    if let Some(default) = &mut plan.default {
        visitor.enter_scope();
        visit_statements_mut(&mut default.statements, visitor);
        visitor.exit_scope();
    }
    visitor.exit_scope();
}

fn visit_statement(statement: &mut LoweredStatement, visitor: &mut impl VisitorMut) {
    match statement {
        LoweredStatement::If(plan) => visit_if(plan, visitor),
        LoweredStatement::Loop(plan) => visit_loop(plan, visitor),
        LoweredStatement::Block(body) => {
            visitor.enter_scope();
            visit_statements_mut(&mut body.statements, visitor);
            visitor.exit_scope();
        }
        LoweredStatement::Body(body) => visit_statements_mut(&mut body.statements, visitor),
        LoweredStatement::Break(_)
        | LoweredStatement::Continue(_)
        | LoweredStatement::UnreachablePanic => {}
        LoweredStatement::Const(plan) => {
            visit_expression(plan.value.node_mut(), visitor);
            visitor.local_binding(&mut plan.name, BindingSite::Constant);
        }
        LoweredStatement::Return(values) => {
            for value in values {
                visit_expression(value.node_mut(), visitor);
            }
        }
        LoweredStatement::Assign(form) => match form {
            AssignForm::Compound { target, kind } => {
                visitor.assignment_target(target.node());
                visit_expression(target.node_mut(), visitor);
                match kind {
                    CompoundKind::OperatorAssign { rhs, .. } => {
                        visit_expression(rhs.node_mut(), visitor);
                    }
                    CompoundKind::Increment | CompoundKind::Decrement => {}
                }
            }
            AssignForm::Simple { target, value } => {
                visitor.assignment_target(target.node());
                match target.node_mut() {
                    node @ GoExpressionNode::Identifier(_) => visitor.assigned_identifier(node),
                    node => visit_expression(node, visitor),
                }
                visit_expression(value.node_mut(), visitor);
            }
        },
        LoweredStatement::Async { call, .. } => visit_expression(call.node_mut(), visitor),
        LoweredStatement::Select(plan) => visit_select(plan, visitor),
        LoweredStatement::Switch(plan) => visit_switch(plan, visitor),
        LoweredStatement::Define(definition) => visit_definition(definition, visitor),
        LoweredStatement::VarDeclaration { name, value, .. } => {
            if let Some(value) = value {
                visit_expression(value.node_mut(), visitor);
            }
            visitor.local_binding(name, BindingSite::Variable);
        }
        LoweredStatement::Discard {
            value: expression, ..
        }
        | LoweredStatement::ExpressionStatement { expression, .. } => {
            visit_expression(expression.node_mut(), visitor)
        }
    }
}
