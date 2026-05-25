use crate::plan::bodies::{ElseArm, IfPlan, LoopPlan, LoweredBlock, LoweredStatement};

/// Walk the block and return any structural problems found. An empty `Vec`
/// means the IR is well-formed by these rules.
pub(crate) fn validate(block: &LoweredBlock) -> Vec<String> {
    let mut issues = Vec::new();
    walk_block(block, &mut issues, "block");
    issues
}

fn walk_block(block: &LoweredBlock, issues: &mut Vec<String>, path: &str) {
    for (i, statement) in block.statements.iter().enumerate() {
        walk_statement(statement, issues, &format!("{}.statements[{}]", path, i));
    }
}

fn walk_statement(statement: &LoweredStatement, issues: &mut Vec<String>, path: &str) {
    match statement {
        LoweredStatement::If(plan) => walk_if(plan, issues, &format!("{}/If", path)),
        LoweredStatement::Loop(plan) => walk_loop(plan, issues, &format!("{}/Loop", path)),
        LoweredStatement::Block(body) => walk_block(body, issues, &format!("{}/Block", path)),
        LoweredStatement::Break { .. } | LoweredStatement::Continue { .. } => {}
        LoweredStatement::Const(_)
        | LoweredStatement::Return(_)
        | LoweredStatement::BreakValue(_)
        | LoweredStatement::Let(_)
        | LoweredStatement::Assign(_)
        | LoweredStatement::Expression(_)
        | LoweredStatement::TempBind { .. }
        | LoweredStatement::RawGo(_) => {}
        LoweredStatement::ClosureBind { body, .. } => {
            walk_block(body, issues, &format!("{}/ClosureBind", path))
        }
        LoweredStatement::Match(plan) => walk_block(&plan.body, issues, &format!("{}/Match", path)),
        LoweredStatement::Select(plan) => {
            for (i, statement) in plan.setup.iter().enumerate() {
                walk_statement(statement, issues, &format!("{}/Select.setup[{}]", path, i));
            }
            for (i, arm) in plan.arms.iter().enumerate() {
                walk_block(arm.body(), issues, &format!("{}/Select.arms[{}]", path, i));
            }
            for (i, statement) in plan.postlude.iter().enumerate() {
                walk_statement(
                    statement,
                    issues,
                    &format!("{}/Select.postlude[{}]", path, i),
                );
            }
        }
        LoweredStatement::Switch(plan) => {
            for (i, case) in plan.cases.iter().enumerate() {
                walk_block(&case.body, issues, &format!("{}/Switch.cases[{}]", path, i));
            }
            if let Some(default_body) = &plan.default {
                walk_block(default_body, issues, &format!("{}/Switch.default", path));
            }
            for (i, statement) in plan.postlude.iter().enumerate() {
                walk_statement(
                    statement,
                    issues,
                    &format!("{}/Switch.postlude[{}]", path, i),
                );
            }
        }
        LoweredStatement::WhileLet(plan) => {
            walk_block(&plan.body, issues, &format!("{}/WhileLet", path))
        }
    }
}

fn walk_if(plan: &IfPlan, issues: &mut Vec<String>, path: &str) {
    if plan.condition.is_empty() {
        issues.push(format!("{}: empty condition", path));
    }
    walk_block(&plan.then_body, issues, &format!("{}.then", path));
    walk_else(&plan.else_arm, issues, &format!("{}.else", path));
}

fn walk_else(arm: &ElseArm, issues: &mut Vec<String>, path: &str) {
    match arm {
        ElseArm::None => {}
        ElseArm::ElseIf(plan) => walk_if(plan, issues, path),
        ElseArm::Else { body, .. } => walk_block(body, issues, path),
    }
}

fn walk_loop(plan: &LoopPlan, issues: &mut Vec<String>, path: &str) {
    if plan.header.is_empty() {
        issues.push(format!("{}: empty header", path));
    }
    walk_block(&plan.body, issues, &format!("{}.body", path));
}
