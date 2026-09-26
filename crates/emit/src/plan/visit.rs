use super::bodies::{
    AssignForm, CompoundKind, Definition, ElseArm, IfPlan, LoopHeader, LoweredStatement,
    SelectArmPlan, SwitchCasePlan, SwitchKind,
};
use super::go_expression::GoExpressionNode;
use super::values::ValuePlan;

pub(crate) trait VisitorMut {
    fn expression(&mut self, node: &mut GoExpressionNode);
    fn binding(&mut self, name: &mut String);
}

pub(crate) fn visit_statements_mut(
    statements: &mut [LoweredStatement],
    visitor: &mut impl VisitorMut,
) {
    for statement in statements {
        visit_statement(statement, visitor);
    }
}

fn visit_expression(node: &mut GoExpressionNode, visitor: &mut impl VisitorMut) {
    visitor.expression(node);
    if let GoExpressionNode::FunctionLiteral {
        parameters, body, ..
    } = node
    {
        for parameter in parameters {
            visitor.binding(&mut parameter.name);
        }
        visit_statements_mut(&mut body.statements, visitor);
    } else {
        node.visit_children_mut(&mut |child| visit_expression(child, visitor));
    }
}

fn visit_definition(definition: &mut Definition, visitor: &mut impl VisitorMut) {
    for name in &mut definition.names {
        visitor.binding(name);
    }
    visit_expression(definition.value.node_mut(), visitor);
}

fn visit_value(value: &mut ValuePlan, visitor: &mut impl VisitorMut) {
    visit_statements_mut(&mut value.setup, visitor);
    visit_expression(value.expression.node_mut(), visitor);
}

fn visit_if(plan: &mut IfPlan, visitor: &mut impl VisitorMut) {
    visit_statements_mut(&mut plan.condition_setup, visitor);
    if let Some(initializer) = &mut plan.initializer {
        visit_definition(initializer, visitor);
    }
    visit_expression(plan.condition.node_mut(), visitor);
    visit_statements_mut(&mut plan.then_body.statements, visitor);
    match &mut plan.else_arm {
        ElseArm::None => {}
        ElseArm::ElseIf(plan) => visit_if(plan, visitor),
        ElseArm::Else { body, .. } => visit_statements_mut(&mut body.statements, visitor),
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
            visit_statements_mut(&mut plan.prologue, visitor);
            match &mut plan.header {
                LoopHeader::Infinite => {}
                LoopHeader::While(condition) => visit_expression(condition.node_mut(), visitor),
                LoopHeader::Range {
                    key,
                    value,
                    iterable,
                } => {
                    for name in key.iter_mut().chain(value.iter_mut()) {
                        visitor.binding(name);
                    }
                    visit_expression(iterable.node_mut(), visitor);
                }
                LoopHeader::Counted {
                    variable,
                    start,
                    condition,
                } => {
                    visitor.binding(variable);
                    visit_expression(start.node_mut(), visitor);
                    if let Some(condition) = condition {
                        visit_expression(condition.node_mut(), visitor);
                    }
                }
            }
            visit_statements_mut(&mut plan.body.statements, visitor);
        }
        LoweredStatement::Block(body)
        | LoweredStatement::Body(body)
        | LoweredStatement::WhileLet(body) => visit_statements_mut(&mut body.statements, visitor),
        LoweredStatement::Break(_)
        | LoweredStatement::Continue(_)
        | LoweredStatement::UnreachablePanic => {}
        LoweredStatement::Const(plan) => {
            visitor.binding(&mut plan.name);
            visit_expression(plan.value.node_mut(), visitor);
        }
        LoweredStatement::Return(values) => {
            for value in values {
                visit_expression(value.node_mut(), visitor);
            }
        }
        LoweredStatement::Assign(form) => match form {
            AssignForm::Compound {
                target_capture,
                target,
                kind,
            } => {
                visit_statements_mut(target_capture, visitor);
                visit_expression(target.node_mut(), visitor);
                match kind {
                    CompoundKind::OpAssign {
                        rhs, pinned_left, ..
                    } => {
                        visit_value(rhs, visitor);
                        if let Some(left) = pinned_left {
                            visit_expression(left.node_mut(), visitor);
                        }
                    }
                    CompoundKind::Increment | CompoundKind::Decrement => {}
                }
            }
            AssignForm::Simple {
                target_capture,
                target,
                value,
            } => {
                visit_statements_mut(target_capture, visitor);
                visit_expression(target.node_mut(), visitor);
                visit_value(value, visitor);
            }
        },
        LoweredStatement::Async { call, .. } => visit_expression(call.node_mut(), visitor),
        LoweredStatement::Select(plan) => {
            visit_statements_mut(&mut plan.setup, visitor);
            for arm in &mut plan.arms {
                match arm {
                    SelectArmPlan::Receive {
                        receive_vars,
                        channel,
                        body,
                    } => {
                        for name in receive_vars {
                            visitor.binding(name);
                        }
                        visit_expression(channel.node_mut(), visitor);
                        visit_statements_mut(&mut body.statements, visitor);
                    }
                    SelectArmPlan::Send {
                        channel,
                        value,
                        body,
                    } => {
                        visit_expression(channel.node_mut(), visitor);
                        visit_expression(value.node_mut(), visitor);
                        visit_statements_mut(&mut body.statements, visitor);
                    }
                    SelectArmPlan::Default { body } => {
                        visit_statements_mut(&mut body.statements, visitor)
                    }
                }
            }
            visit_statements_mut(&mut plan.postlude, visitor);
        }
        LoweredStatement::Switch(plan) => {
            match &mut plan.kind {
                SwitchKind::Conditional => {}
                SwitchKind::Value { subject } => visit_expression(subject.node_mut(), visitor),
                SwitchKind::Type { subject, binding } => {
                    if let Some(name) = binding {
                        visitor.binding(name);
                    }
                    visit_expression(subject.node_mut(), visitor);
                }
            }
            for case in &mut plan.cases {
                visit_case(case, visitor);
            }
            if let Some(default) = &mut plan.default {
                visit_statements_mut(&mut default.statements, visitor);
            }
            visit_statements_mut(&mut plan.postlude, visitor);
        }
        LoweredStatement::Define(definition) => visit_definition(definition, visitor),
        LoweredStatement::AssignMany { targets, value } => {
            for target in targets {
                visit_expression(target.node_mut(), visitor);
            }
            visit_expression(value.node_mut(), visitor);
        }
        LoweredStatement::VarDecl { name, value, .. } => {
            visitor.binding(name);
            if let Some(value) = value {
                visit_expression(value.node_mut(), visitor);
            }
        }
        LoweredStatement::Discard(expression)
        | LoweredStatement::ExpressionStatement { expression, .. } => {
            visit_expression(expression.node_mut(), visitor)
        }
        LoweredStatement::Directed { inner, .. } => visit_statement(inner, visitor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::bodies::{LoweredBlock, define, rename_generated_names};
    use crate::plan::go_expression::{FunctionLiteralLayout, GoParameter};
    use crate::plan::values::GoExpression;

    fn name(value: &str) -> GoExpression {
        GoExpression::name(value.to_string())
    }

    fn function(parameter: &str, local: &str) -> GoExpression {
        GoExpression::function_literal(
            vec![GoParameter::new(parameter, "int")],
            "int".to_string(),
            LoweredBlock {
                statements: vec![
                    define(local.to_string(), name(parameter)),
                    LoweredStatement::Return(vec![name(local)]),
                ],
            },
            FunctionLiteralLayout::MultiLine,
        )
    }

    #[test]
    fn renaming_keeps_nested_function_declarations_and_reads_together() {
        let mut statements = vec![define(
            "callback_1".to_string(),
            function("arg_1", "local_1"),
        )];
        rename_generated_names(&mut statements, &|name| {
            name.strip_suffix("_1").map(str::to_string)
        });
        assert_eq!(
            statements,
            vec![define("callback".to_string(), function("arg", "local"))]
        );
    }

    #[test]
    fn visitation_reaches_nested_function_names_once() {
        #[derive(Default)]
        struct Names {
            bindings: Vec<String>,
            reads: Vec<String>,
        }
        impl VisitorMut for Names {
            fn expression(&mut self, node: &mut GoExpressionNode) {
                if let GoExpressionNode::Identifier(name) = node {
                    self.reads.push(name.clone());
                }
            }
            fn binding(&mut self, name: &mut String) {
                self.bindings.push(name.clone());
            }
        }
        let mut statements = vec![define("callback".to_string(), function("arg", "local"))];
        let mut names = Names::default();
        visit_statements_mut(&mut statements, &mut names);
        assert_eq!(names.bindings, ["callback", "arg", "local"]);
        assert_eq!(names.reads, ["arg", "local"]);
    }

    #[test]
    fn renaming_reaches_each_else_if_initializer() {
        let mut statements = vec![LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: Some(Definition::single("first_1".to_string(), name("source"))),
            condition: name("first_1"),
            then_body: LoweredBlock {
                statements: Vec::new(),
            },
            else_arm: ElseArm::ElseIf(Box::new(IfPlan {
                condition_setup: Vec::new(),
                initializer: Some(Definition::single("second_1".to_string(), name("source"))),
                condition: name("second_1"),
                then_body: LoweredBlock {
                    statements: Vec::new(),
                },
                else_arm: ElseArm::None,
            })),
        })];
        rename_generated_names(&mut statements, &|name| {
            name.strip_suffix("_1").map(str::to_string)
        });
        let LoweredStatement::If(first) = &statements[0] else {
            panic!("expected if");
        };
        assert_eq!(first.initializer.as_ref().unwrap().names, ["first"]);
        assert_eq!(first.condition, name("first"));
        let ElseArm::ElseIf(second) = &first.else_arm else {
            panic!("expected else-if");
        };
        assert_eq!(second.initializer.as_ref().unwrap().names, ["second"]);
        assert_eq!(second.condition, name("second"));
    }
}
