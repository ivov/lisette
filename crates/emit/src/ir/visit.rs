use super::bodies::{
    AssignForm, CompoundKind, Definition, ElseArm, IfPlan, LoopHeader, LoweredStatement,
    SelectArmKind, Statement, SwitchCasePlan, SwitchKind,
};
use super::go_expression::GoExpressionNode;
use super::local::GoIdentifier;
use super::local::LocalId;
use crate::state::scope::ScopeState;
use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

pub(crate) trait VisitorMut {
    fn expression(&mut self, node: &mut GoExpressionNode);

    fn statements(&mut self, _statements: &mut Vec<Statement>) {}

    fn enter_scope(&mut self) {}

    fn exit_scope(&mut self) {}

    fn local_binding(&mut self, _name: &mut GoIdentifier) {}

    fn assignment_target(&mut self, _target: &GoExpressionNode) {}
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
            visitor.local_binding(&mut parameter.name);
        }
        visit_statements_mut(&mut body.statements, visitor);
        visitor.exit_scope();
    } else {
        node.visit_children_mut(&mut |child| visit_expression(child, visitor));
    }
}

fn visit_definition(definition: &mut Definition, visitor: &mut impl VisitorMut) {
    visit_expression(definition.value.node_mut(), visitor);
    for name in &mut definition.names {
        visitor.local_binding(name);
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
            inline: false,
        } => {
            visitor.enter_scope();
            visit_statements_mut(&mut body.statements, visitor);
            visitor.exit_scope();
        }
        ElseArm::Else { body, inline: true } => {
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

fn visit_statement(statement: &mut LoweredStatement, visitor: &mut impl VisitorMut) {
    match statement {
        LoweredStatement::If(plan) => visit_if(plan, visitor),
        LoweredStatement::Loop(plan) => {
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
                        visitor.local_binding(name);
                    }
                }
                LoopHeader::Counted {
                    variable,
                    start,
                    condition,
                } => {
                    visit_expression(start.node_mut(), visitor);
                    visitor.local_binding(variable);
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
            visitor.local_binding(&mut plan.name);
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
                    CompoundKind::OpAssign { rhs, .. } => {
                        visit_expression(rhs.node_mut(), visitor);
                    }
                    CompoundKind::Increment | CompoundKind::Decrement => {}
                }
            }
            AssignForm::Simple { target, value } => {
                visitor.assignment_target(target.node());
                visit_expression(target.node_mut(), visitor);
                visit_expression(value.node_mut(), visitor);
            }
        },
        LoweredStatement::Async { call, .. } => visit_expression(call.node_mut(), visitor),
        LoweredStatement::Select(plan) => {
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
                            visitor.local_binding(name);
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
        LoweredStatement::Switch(plan) => {
            visitor.enter_scope();
            match &mut plan.kind {
                SwitchKind::Conditional => {}
                SwitchKind::Value { subject } => visit_expression(subject.node_mut(), visitor),
                SwitchKind::Type { subject, binding } => {
                    visit_expression(subject.node_mut(), visitor);
                    if let Some(name) = binding {
                        visitor.local_binding(name);
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
        LoweredStatement::Define(definition) => visit_definition(definition, visitor),
        LoweredStatement::VarDecl { name, value, .. } => {
            if let Some(value) = value {
                visit_expression(value.node_mut(), visitor);
            }
            visitor.local_binding(name);
        }
        LoweredStatement::Discard {
            value: expression, ..
        }
        | LoweredStatement::ExpressionStatement { expression, .. } => {
            visit_expression(expression.node_mut(), visitor)
        }
    }
}

pub(crate) fn identify_body_locals(
    statements: &mut Vec<Statement>,
    parameters: &[GoIdentifier],
    scope: &mut ScopeState,
) -> HashSet<LocalId> {
    struct Resolve<'a> {
        scope: &'a mut ScopeState,
        frames: Vec<HashMap<String, LocalId>>,
        shadowing: HashSet<LocalId>,
    }

    impl VisitorMut for Resolve<'_> {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            let GoExpressionNode::Identifier(name) = node else {
                return;
            };
            if let Some(id) = name.id() {
                assert!(
                    self.frames
                        .iter()
                        .any(|frame| frame.values().any(|local| *local == id)),
                    "identified read of `{}` has no visible declaration",
                    name.spelling()
                );
                return;
            }
            assert!(
                !name.is_pending(),
                "read has no producer-assigned identity: {}",
                name.spelling()
            );
        }

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            if name.spelling() == "_" {
                name.finish();
                return;
            }
            let current = self
                .frames
                .last()
                .expect("a local scope exists")
                .get(name.spelling())
                .copied();
            let id =
                current.unwrap_or_else(|| name.id().unwrap_or_else(|| self.scope.new_local_id()));
            name.identify(id);
            if self.frames[..self.frames.len() - 1]
                .iter()
                .any(|frame| frame.get(name.spelling()).is_some_and(|outer| *outer != id))
            {
                self.shadowing.insert(id);
            }
            self.frames
                .last_mut()
                .expect("a local scope exists")
                .insert(name.spelling().to_string(), id);
        }

        fn enter_scope(&mut self) {
            self.frames.push(HashMap::default());
        }

        fn exit_scope(&mut self) {
            self.frames.pop().expect("a local scope exists");
        }
    }

    let mut resolver = Resolve {
        scope,
        frames: vec![HashMap::default()],
        shadowing: HashSet::default(),
    };
    for name in parameters {
        let mut name = name.clone();
        resolver.local_binding(&mut name);
    }
    visit_statements_mut(statements, &mut resolver);
    resolver.shadowing
}
