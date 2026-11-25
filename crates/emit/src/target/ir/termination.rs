use crate::target::ir::bodies::{
    ElseArm, ElseLayout, FlowSummary, IfPlan, LoopHeader, LoopTransfer, LoweredBlock,
    LoweredStatement, Statement, SwitchStatementPlan,
};
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::values::GoExpression;

pub(crate) fn list_terminates(statements: &[Statement]) -> bool {
    match last_rendered(statements) {
        Some(Last::Statement(statement)) => statement_terminates(statement),
        Some(Last::InlineElse(body)) => list_terminates(&body.statements),
        None => false,
    }
}

enum Last<'a> {
    Statement(&'a LoweredStatement),
    InlineElse(&'a LoweredBlock),
}

fn last_rendered(statements: &[Statement]) -> Option<Last<'_>> {
    for statement in statements.iter().rev() {
        if !statement.kind.emits_output() {
            continue;
        }
        return match &statement.kind {
            LoweredStatement::Body(body) => last_rendered(&body.statements),
            LoweredStatement::If(plan) => match inline_else(plan) {
                Some(body) if !body.renders_empty() => Some(Last::InlineElse(body)),
                _ => Some(Last::Statement(&statement.kind)),
            },
            kind => Some(Last::Statement(kind)),
        };
    }
    None
}

fn inline_else(plan: &IfPlan) -> Option<&LoweredBlock> {
    match &plan.else_arm {
        ElseArm::Else {
            body,
            layout: ElseLayout::Inline,
        } => Some(body),
        ElseArm::ElseIf(next) => inline_else(next),
        _ => None,
    }
}

fn statement_terminates(statement: &LoweredStatement) -> bool {
    match statement {
        LoweredStatement::Return(_) | LoweredStatement::UnreachablePanic => true,
        LoweredStatement::ExpressionStatement { expression, .. } => is_panic_call(expression),
        LoweredStatement::Block(body) | LoweredStatement::Body(body) => {
            list_terminates(&body.statements)
        }
        LoweredStatement::If(plan) => if_terminates(plan),
        LoweredStatement::Loop(plan) => {
            let unconditional = match &plan.header {
                LoopHeader::Infinite => true,
                LoopHeader::Counted { condition, .. } => condition.is_none(),
                LoopHeader::While(_) | LoopHeader::Range { .. } => false,
            };
            unconditional
                && !breaks_out(
                    &plan.body.statements,
                    plan.kind.label(),
                    UnlabeledBreak::Exits,
                )
        }
        LoweredStatement::Switch(plan) => switch_terminates(plan),
        LoweredStatement::Select(plan) => plan.arms.iter().all(|arm| {
            let body = arm.body();
            list_terminates(&body.statements)
                && !breaks_out(&body.statements, None, UnlabeledBreak::Exits)
        }),
        _ => false,
    }
}

fn if_terminates(plan: &IfPlan) -> bool {
    list_terminates(&plan.then_body.statements)
        && match &plan.else_arm {
            ElseArm::None
            | ElseArm::Else {
                layout: ElseLayout::Inline,
                ..
            } => false,
            ElseArm::ElseIf(next) => if_terminates(next),
            ElseArm::Else { body, .. } => list_terminates(&body.statements),
        }
}

fn switch_terminates(plan: &SwitchStatementPlan) -> bool {
    let Some(default) = &plan.default else {
        return false;
    };
    plan.cases
        .iter()
        .map(|case| &case.body)
        .chain([default])
        .all(|body| {
            list_terminates(&body.statements)
                && !breaks_out(&body.statements, None, UnlabeledBreak::Exits)
        })
}

fn is_panic_call(expression: &GoExpression) -> bool {
    matches!(expression.node(), GoExpressionNode::Call { callee, .. }
        if matches!(callee.as_ref(), GoExpressionNode::Identifier(name)
            if name.is_external() && name.spelling() == "panic"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnlabeledBreak {
    Exits,
    Captured,
}

fn breaks_out(statements: &[Statement], label: Option<&str>, unlabeled: UnlabeledBreak) -> bool {
    statements
        .iter()
        .any(|statement| statement_breaks_out(&statement.kind, label, unlabeled))
}

fn statement_breaks_out(
    statement: &LoweredStatement,
    label: Option<&str>,
    unlabeled: UnlabeledBreak,
) -> bool {
    let nested =
        |body: &LoweredBlock| breaks_out(&body.statements, label, UnlabeledBreak::Captured);
    match statement {
        LoweredStatement::Break(LoopTransfer::Unlabeled) => unlabeled == UnlabeledBreak::Exits,
        LoweredStatement::Break(LoopTransfer::Labeled(target)) => label == Some(target.as_str()),
        LoweredStatement::Block(body) | LoweredStatement::Body(body) => {
            breaks_out(&body.statements, label, unlabeled)
        }
        LoweredStatement::If(plan) => if_breaks_out(plan, label, unlabeled),
        LoweredStatement::Loop(plan) => nested(&plan.body),
        LoweredStatement::Switch(plan) => {
            plan.cases.iter().any(|case| nested(&case.body))
                || plan.default.as_ref().is_some_and(nested)
        }
        LoweredStatement::Select(plan) => plan.arms.iter().any(|arm| nested(arm.body())),
        _ => false,
    }
}

fn if_breaks_out(plan: &IfPlan, label: Option<&str>, unlabeled: UnlabeledBreak) -> bool {
    breaks_out(&plan.then_body.statements, label, unlabeled)
        || match &plan.else_arm {
            ElseArm::None => false,
            ElseArm::ElseIf(next) => if_breaks_out(next, label, unlabeled),
            ElseArm::Else { body, .. } => breaks_out(&body.statements, label, unlabeled),
        }
}

impl LoweredBlock {
    pub(crate) fn ensure_go_termination(&mut self) {
        terminate_go_path(&mut self.statements);
    }

    pub(crate) fn go_terminates(&self) -> bool {
        list_terminates(&self.statements)
    }
}

impl LoweredStatement {
    fn terminate_go_tail(&mut self) {
        match self {
            Self::If(plan) => plan.terminate_go_tail(),
            Self::Body(body) => terminate_go_path(&mut body.statements),
            _ => {}
        }
    }
}

impl IfPlan {
    fn terminate_go_tail(&mut self) {
        terminate_go_path(&mut self.then_body.statements);
        match &mut self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(inner) => inner.terminate_go_tail(),
            ElseArm::Else { body, .. } => terminate_go_path(&mut body.statements),
        }
    }
}

fn terminate_go_path(statements: &mut Vec<Statement>) {
    if list_terminates(statements) || FlowSummary::statements(statements).falls_through {
        return;
    }
    if let Some(last) = statements.last_mut()
        && !last.kind.flow().falls_through
    {
        last.kind.terminate_go_tail();
        if list_terminates(statements) {
            return;
        }
    }
    statements.push(LoweredStatement::UnreachablePanic.into());
}
