use std::mem;
use std::slice;

use rustc_hash::FxHashSet as HashSet;

use crate::output::finish::cleanup::name_uses::NameUses;
use crate::output::finish::cleanup::terminal_else::{drop_empty_else, flip_branches};
use crate::target::ir::bodies::{
    AssignForm, Definition, ElseArm, IfPlan, LoopHeader, LoopPlan, LoweredBlock, LoweredStatement,
    SelectArmKind, Statement, SwitchKind, SwitchStatementPlan, direct_first, effect_statement,
    for_each_statements_mut,
};
use crate::target::ir::go_expression::{CallPurity, GoExpressionNode, Position};
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use crate::target::ir::visit::{VisitorMut, visit_statements_mut};

pub(crate) fn apply_go_hygiene(body: &mut LoweredBlock, has_result: bool) {
    drop_unused_locals(&mut body.statements);
    blank_unused_literal_parameters(&mut body.statements);
    drop_empty_else_arms(&mut body.statements);
    terminate_function_literals(&mut body.statements);
    if has_result {
        body.ensure_go_termination();
    }
}

fn terminate_function_literals(statements: &mut Vec<Statement>) {
    struct Literals;
    impl VisitorMut for Literals {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            if let GoExpressionNode::FunctionLiteral { result, body, .. } = node
                && !result.code.is_empty()
            {
                body.ensure_go_termination();
            }
        }
    }
    visit_statements_mut(statements, &mut Literals);
}

pub(crate) fn blank_unused_parameters(
    body: &mut LoweredBlock,
    parameters: &mut [GoIdentifier],
    receiver: Option<&GoIdentifier>,
) {
    let uses = NameUses::of(&mut body.statements);
    for parameter in parameters {
        let is_receiver = receiver.is_some_and(|receiver| receiver.refers_to_same(parameter));
        if !is_receiver && is_unreferenced(parameter, &uses) {
            *parameter.spelling_mut() = "_".to_string();
        }
    }
}

fn blank_unused_literal_parameters(statements: &mut Vec<Statement>) {
    struct Literals<'a>(&'a NameUses);
    impl VisitorMut for Literals<'_> {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            if let GoExpressionNode::FunctionLiteral { parameters, .. } = node {
                for parameter in parameters.iter_mut() {
                    if is_unreferenced(&parameter.name, self.0) {
                        parameter.name = blank();
                    }
                }
            }
        }
    }
    let uses = NameUses::of(statements);
    visit_statements_mut(statements, &mut Literals(&uses));
}

fn is_unreferenced(name: &GoIdentifier, uses: &NameUses) -> bool {
    name.spelling() != "_" && uses.references(name) == 0
}

fn blank() -> GoIdentifier {
    GoIdentifier::external("_".to_string())
}

fn keep_effects(value: GoExpression, line: Option<Position>) -> Option<Statement> {
    let empty_literal = matches!(value.node(),
        GoExpressionNode::CompositeLiteral { elements, .. } if elements.is_empty());
    if value.is_empty() || empty_literal || value.effects().can_erase() {
        return None;
    }
    if let GoExpressionNode::Call {
        arguments,
        purity: CallPurity::Constructor | CallPurity::Conversion,
        ..
    } = value.node()
    {
        let mut effectful = arguments
            .iter()
            .map(|argument| GoExpression::from_operand(argument.clone()))
            .filter(|argument| !argument.effects().can_erase());
        match (effectful.next(), effectful.next()) {
            (None, _) => return None,
            (Some(argument), None) => return keep_effects(argument, line),
            _ => {}
        }
    }
    let mut statement = effect_statement(value);
    statement.line = line;
    Some(statement)
}

struct Liveness {
    uses: NameUses,
    compound: HashSet<LocalId>,
}

impl Liveness {
    fn of(statements: &mut Vec<Statement>) -> Self {
        Self {
            uses: NameUses::of(statements),
            compound: compound_targets(statements),
        }
    }

    fn unused(&self, name: &GoIdentifier) -> bool {
        name.id().is_some_and(|id| !self.compound.contains(&id))
            && name.spelling() != "_"
            && self.uses.value_reads(name) == 0
            && self.uses.bindings(name) == 1
    }

    fn removable(&self, name: &GoIdentifier) -> bool {
        self.unused(name) && !self.uses.is_written(name)
    }
}

pub(crate) fn drop_unused_locals(statements: &mut Vec<Statement>) {
    loop {
        let liveness = Liveness::of(statements);
        let mut dead_vars: Vec<GoIdentifier> = Vec::new();
        let mut changed = false;
        for_each_statements_mut(statements, &mut |list| {
            for statement in mem::take(list) {
                for statement in unscoped(statement, &liveness) {
                    list.extend(without_unused(
                        statement,
                        &liveness,
                        &mut dead_vars,
                        &mut changed,
                    ));
                }
            }
        });
        changed |= !dead_vars.is_empty();
        drop_writes_to(statements, &dead_vars);
        changed |= drop_empty_ifs(statements);
        for_each_statements_mut(statements, &mut |list| {
            for statement in list.iter_mut() {
                changed |= blank_unused_header_names(&mut statement.kind, &liveness);
            }
        });
        if !changed {
            return;
        }
    }
}

fn unscoped(statement: Statement, liveness: &Liveness) -> Vec<Statement> {
    match statement.kind {
        LoweredStatement::Block(body)
            if unused_declarations(&body.statements, liveness).is_some_and(|count| count > 0) =>
        {
            let mut statements = body.statements;
            direct_first(&mut statements, statement.line);
            statements
        }
        kind => vec![Statement {
            line: statement.line,
            kind,
        }],
    }
}

fn unused_declarations(statements: &[Statement], liveness: &Liveness) -> Option<usize> {
    let mut count = 0;
    for statement in statements {
        let names: &[GoIdentifier] = match &statement.kind {
            LoweredStatement::Define(Definition { names, .. }) => names,
            LoweredStatement::VarDecl { name, .. } => slice::from_ref(name),
            LoweredStatement::Const(_) => return None,
            LoweredStatement::Body(body) => {
                count += unused_declarations(&body.statements, liveness)?;
                continue;
            }
            _ => continue,
        };
        if !names.iter().all(|name| liveness.removable(name)) {
            return None;
        }
        count += 1;
    }
    Some(count)
}

fn without_unused(
    statement: Statement,
    liveness: &Liveness,
    dead_vars: &mut Vec<GoIdentifier>,
    changed: &mut bool,
) -> Option<Statement> {
    let line = statement.line;
    match statement.kind {
        LoweredStatement::Define(Definition { names, value })
            if names.len() == 1 && liveness.unused(&names[0]) =>
        {
            dead_vars.extend(names);
            keep_effects(value, line)
        }
        LoweredStatement::VarDecl { name, value, .. } if liveness.unused(&name) => {
            dead_vars.push(name);
            value.and_then(|value| keep_effects(value, line))
        }
        LoweredStatement::Define(Definition { mut names, value })
            if names.len() > 1 && names.iter().any(|name| liveness.removable(name)) =>
        {
            for name in names.iter_mut().filter(|name| liveness.removable(name)) {
                *name = blank();
            }
            let kind = if names.iter().all(|name| name.spelling() == "_") {
                *changed = true;
                if value.stands_as_a_statement() {
                    return Some(Statement {
                        line,
                        kind: effect_statement(value).kind,
                    });
                }
                LoweredStatement::Discard {
                    arity: names.len(),
                    value,
                }
            } else {
                LoweredStatement::Define(Definition { names, value })
            };
            *changed = true;
            Some(Statement { line, kind })
        }
        kind => Some(Statement { line, kind }),
    }
}

fn blank_unused_header_names(statement: &mut LoweredStatement, liveness: &Liveness) -> bool {
    let removable = |name: &GoIdentifier| liveness.removable(name);
    match statement {
        LoweredStatement::Loop(LoopPlan {
            header: LoopHeader::Range { key, value, .. },
            ..
        }) => {
            let mut changed = false;
            for slot in [&mut *value, &mut *key] {
                if slot.as_ref().is_some_and(removable) {
                    *slot = None;
                    changed = true;
                }
            }
            if value.is_none() && key.as_ref().is_some_and(|key| key.spelling() == "_") {
                *key = None;
                changed = true;
            }
            changed
        }
        LoweredStatement::Switch(SwitchStatementPlan {
            kind: SwitchKind::Type { binding, .. },
            ..
        }) if binding.as_ref().is_some_and(removable) => {
            *binding = None;
            true
        }
        LoweredStatement::Select(plan) => {
            let mut changed = false;
            for arm in &mut plan.arms {
                let SelectArmKind::Receive { receive_vars, .. } = &mut arm.kind else {
                    continue;
                };
                for name in receive_vars.iter_mut().filter(|name| removable(name)) {
                    *name = blank();
                    changed = true;
                }
                if receive_vars.iter().all(|name| name.spelling() == "_") {
                    changed |= !receive_vars.is_empty();
                    receive_vars.clear();
                }
            }
            changed
        }
        _ => false,
    }
}

fn compound_targets(statements: &mut Vec<Statement>) -> HashSet<LocalId> {
    let mut targets = HashSet::default();
    for_each_statements_mut(statements, &mut |list| {
        for statement in list.iter() {
            if let LoweredStatement::Assign(AssignForm::Compound { target, .. }) = &statement.kind
                && let Some(id) = target.as_identifier().and_then(GoIdentifier::id)
            {
                targets.insert(id);
            }
        }
    });
    targets
}

fn drop_writes_to(statements: &mut Vec<Statement>, names: &[GoIdentifier]) {
    if names.is_empty() {
        return;
    }
    for_each_statements_mut(statements, &mut |list| {
        for statement in mem::take(list) {
            let line = statement.line.clone();
            match statement.kind {
                LoweredStatement::Assign(AssignForm::Simple { target, value })
                    if target.as_identifier().is_some_and(|target| {
                        names.iter().any(|name| name.refers_to_same(target))
                    }) =>
                {
                    list.extend(keep_effects(value, line));
                }
                kind => list.push(Statement { line, kind }),
            }
        }
    });
}

fn drop_empty_ifs(statements: &mut Vec<Statement>) -> bool {
    let mut changed = false;
    for_each_statements_mut(statements, &mut |list| {
        for statement in mem::take(list) {
            match statement.kind {
                LoweredStatement::If(mut plan) if tests_a_flag(&plan) => {
                    drop_empty_else(&mut plan);
                    if plan.then_body.renders_empty() && matches!(plan.else_arm, ElseArm::None) {
                        changed = true;
                    } else {
                        list.push(Statement {
                            line: statement.line,
                            kind: LoweredStatement::If(plan),
                        });
                    }
                }
                kind => list.push(Statement {
                    line: statement.line,
                    kind,
                }),
            }
        }
    });
    changed
}

fn tests_a_flag(plan: &IfPlan) -> bool {
    plan.initializer.is_none() && plan.condition.as_identifier().is_some()
}

fn drop_empty_else_arms(statements: &mut Vec<Statement>) {
    for_each_statements_mut(statements, &mut |list| {
        for statement in list.iter_mut() {
            if let LoweredStatement::If(plan) = &mut statement.kind {
                drop_empty_else(plan);
                if tests_a_flag(plan)
                    && plan.then_body.renders_empty()
                    && matches!(plan.else_arm, ElseArm::Else { .. })
                {
                    flip_branches(plan);
                    plan.else_arm = ElseArm::None;
                }
            }
        }
    });
}
