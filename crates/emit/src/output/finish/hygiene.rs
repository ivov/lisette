use std::mem;
use std::slice;

use rustc_hash::FxHashSet;

use crate::output::finish::cleanup::name_uses::NameUses;
use crate::output::finish::cleanup::terminal_else::{drop_empty_else, flip_branches};
use crate::output::finish::verify::BodyResult;
use crate::target::ir::bodies::{
    AssignForm, Definition, ElseArm, IfPlan, LoopHeader, LoopPlan, LoweredBlock, LoweredStatement,
    SelectArmKind, Statement, SwitchKind, SwitchStatementPlan, direct_first, effect_statement,
    for_each_statements_mut, scope_statements,
};
use crate::target::ir::go_expression::{CallPurity, GoExpressionNode, Position};
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use crate::target::ir::visit::{BindingSite, VisitorMut, visit_statements_mut};

pub(crate) fn apply_go_hygiene(body: &mut LoweredBlock, result: BodyResult) {
    drop_unused_locals(&mut body.statements);
    blank_unused_literal_parameters(&mut body.statements);
    drop_empty_else_arms(&mut body.statements);
    terminate_function_literals(&mut body.statements);
    if result == BodyResult::Value {
        body.ensure_go_termination();
    }
}

fn terminate_function_literals(statements: &mut Vec<Statement>) {
    struct Literals;

    impl VisitorMut for Literals {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            if let GoExpressionNode::FunctionLiteral {
                result: Some(_),
                body,
                ..
            } = node
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
                        parameter.name = GoIdentifier::blank();
                    }
                }
            }
        }
    }
    let uses = NameUses::of(statements);
    visit_statements_mut(statements, &mut Literals(&uses));
}

fn is_unreferenced(name: &GoIdentifier, uses: &NameUses) -> bool {
    !name.is_blank() && uses.references(name) == 0
}

fn keep_effects(value: GoExpression, line: Option<Position>) -> Option<Statement> {
    let empty_literal = matches!(value.node(),
        GoExpressionNode::CompositeLiteral { elements, .. } if elements.is_empty());
    if empty_literal || value.effects().can_erase() {
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
    compound: FxHashSet<LocalId>,
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
            && !name.is_blank()
            && self.uses.value_reads(name) == 0
            && self.uses.bindings(name) == 1
    }
}

struct UnreadBindings<'a> {
    liveness: &'a Liveness,
    blanked: Vec<GoIdentifier>,
}

impl VisitorMut for UnreadBindings<'_> {
    fn expression(&mut self, _node: &mut GoExpressionNode) {}

    fn local_binding(&mut self, name: &mut GoIdentifier, site: BindingSite) {
        if site.must_be_read() && self.liveness.unused(name) {
            self.blanked.push(mem::replace(name, GoIdentifier::blank()));
        }
    }
}

pub(crate) fn drop_unused_locals(statements: &mut Vec<Statement>) {
    loop {
        let liveness = Liveness::of(statements);
        for_each_statements_mut(statements, &mut |list| {
            for statement in mem::take(list) {
                list.extend(unscoped(statement, &liveness));
            }
        });
        let mut unread = UnreadBindings {
            liveness: &liveness,
            blanked: Vec::new(),
        };
        visit_statements_mut(statements, &mut unread);
        drop_writes_to(statements, &unread.blanked);
        settle_blank_bindings(statements);
        let dropped_ifs = drop_empty_ifs(statements);
        if unread.blanked.is_empty() && !dropped_ifs {
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
        kind @ LoweredStatement::If(_)
        | kind @ LoweredStatement::Loop(_)
        | kind @ LoweredStatement::Block(_)
        | kind @ LoweredStatement::Body(_)
        | kind @ LoweredStatement::Break(_)
        | kind @ LoweredStatement::Continue(_)
        | kind @ LoweredStatement::Const(_)
        | kind @ LoweredStatement::Return(_)
        | kind @ LoweredStatement::Assign(_)
        | kind @ LoweredStatement::Async { .. }
        | kind @ LoweredStatement::Select(_)
        | kind @ LoweredStatement::Switch(_)
        | kind @ LoweredStatement::Define(_)
        | kind @ LoweredStatement::VarDeclaration { .. }
        | kind @ LoweredStatement::Discard { .. }
        | kind @ LoweredStatement::ExpressionStatement { .. }
        | kind @ LoweredStatement::UnreachablePanic => vec![Statement {
            line: statement.line,
            kind,
        }],
    }
}

fn unused_declarations(statements: &[Statement], liveness: &Liveness) -> Option<usize> {
    let mut count = 0;
    for statement in scope_statements(statements) {
        let names: &[GoIdentifier] = match statement {
            LoweredStatement::Define(Definition { names, .. }) => names,
            LoweredStatement::VarDeclaration { name, .. } => slice::from_ref(name),
            LoweredStatement::Const(_) => return None,
            LoweredStatement::If(_)
            | LoweredStatement::Loop(_)
            | LoweredStatement::Block(_)
            | LoweredStatement::Body(_)
            | LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Return(_)
            | LoweredStatement::Assign(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Select(_)
            | LoweredStatement::Switch(_)
            | LoweredStatement::Discard { .. }
            | LoweredStatement::ExpressionStatement { .. }
            | LoweredStatement::UnreachablePanic => continue,
        };

        if !names.iter().all(|name| liveness.unused(name)) {
            return None;
        }
        count += 1;
    }
    Some(count)
}

fn discard_values(arity: usize, value: GoExpression, line: Option<Position>) -> Option<Statement> {
    if arity == 1 {
        return keep_effects(value, line);
    }
    if value.effects().can_erase() {
        return None;
    }
    let kind = if value.stands_as_a_statement() {
        effect_statement(value).kind
    } else {
        LoweredStatement::Discard { arity, value }
    };
    Some(Statement { line, kind })
}

fn hoist_blank_initializers(plan: &mut IfPlan, line: Option<Position>) -> Vec<Statement> {
    plan.else_arm = match mem::replace(&mut plan.else_arm, ElseArm::None) {
        ElseArm::ElseIf(mut link) => {
            let link_line = link.else_if_line.clone();
            let mut statements = hoist_blank_initializers(&mut link, link_line.clone());
            if statements.is_empty() {
                ElseArm::ElseIf(link)
            } else {
                statements.push(Statement {
                    line: link_line,
                    kind: LoweredStatement::If(*link),
                });
                ElseArm::block(LoweredBlock { statements })
            }
        }
        other @ ElseArm::None | other @ ElseArm::Else { .. } => other,
    };
    match plan
        .initializer
        .take_if(|definition| definition.names.iter().all(GoIdentifier::is_blank))
    {
        Some(Definition { names, value }) => discard_values(names.len(), value, line)
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

fn settle_blank_bindings(statements: &mut Vec<Statement>) {
    for_each_statements_mut(statements, &mut |list| {
        for statement in mem::take(list) {
            let line = statement.line;
            match statement.kind {
                LoweredStatement::Define(Definition { names, value })
                    if names.iter().all(GoIdentifier::is_blank) =>
                {
                    list.extend(discard_values(names.len(), value, line));
                }
                LoweredStatement::VarDeclaration { name, value, .. } if name.is_blank() => {
                    list.extend(value.and_then(|value| keep_effects(value, line)));
                }
                LoweredStatement::If(mut plan) => {
                    list.extend(hoist_blank_initializers(&mut plan, line.clone()));
                    list.push(Statement {
                        line,
                        kind: LoweredStatement::If(plan),
                    });
                }
                kind @ LoweredStatement::Loop(_)
                | kind @ LoweredStatement::Block(_)
                | kind @ LoweredStatement::Body(_)
                | kind @ LoweredStatement::Break(_)
                | kind @ LoweredStatement::Continue(_)
                | kind @ LoweredStatement::Const(_)
                | kind @ LoweredStatement::Return(_)
                | kind @ LoweredStatement::Assign(_)
                | kind @ LoweredStatement::Async { .. }
                | kind @ LoweredStatement::Select(_)
                | kind @ LoweredStatement::Switch(_)
                | kind @ LoweredStatement::Define(_)
                | kind @ LoweredStatement::VarDeclaration { .. }
                | kind @ LoweredStatement::Discard { .. }
                | kind @ LoweredStatement::ExpressionStatement { .. }
                | kind @ LoweredStatement::UnreachablePanic => {
                    let mut kind = kind;
                    settle_blank_header(&mut kind);
                    list.push(Statement { line, kind });
                }
            }
        }
    });
}

fn settle_blank_header(statement: &mut LoweredStatement) {
    match statement {
        LoweredStatement::Loop(LoopPlan {
            header: LoopHeader::Range { key, value, .. },
            ..
        }) => {
            for slot in [value, key] {
                if slot.as_ref().is_some_and(GoIdentifier::is_blank) {
                    *slot = None;
                }
            }
        }
        LoweredStatement::Switch(SwitchStatementPlan {
            kind: SwitchKind::Type { binding, .. },
            ..
        }) if binding.as_ref().is_some_and(GoIdentifier::is_blank) => *binding = None,
        LoweredStatement::Select(plan) => {
            for arm in &mut plan.arms {
                if let SelectArmKind::Receive { receive_vars, .. } = &mut arm.kind
                    && receive_vars.iter().all(GoIdentifier::is_blank)
                {
                    receive_vars.clear();
                }
            }
        }
        LoweredStatement::If(_)
        | LoweredStatement::Loop(_)
        | LoweredStatement::Block(_)
        | LoweredStatement::Body(_)
        | LoweredStatement::Break(_)
        | LoweredStatement::Continue(_)
        | LoweredStatement::Const(_)
        | LoweredStatement::Return(_)
        | LoweredStatement::Assign(_)
        | LoweredStatement::Async { .. }
        | LoweredStatement::Switch(_)
        | LoweredStatement::Define(_)
        | LoweredStatement::VarDeclaration { .. }
        | LoweredStatement::Discard { .. }
        | LoweredStatement::ExpressionStatement { .. }
        | LoweredStatement::UnreachablePanic => {}
    }
}

fn compound_targets(statements: &mut Vec<Statement>) -> FxHashSet<LocalId> {
    let mut targets = FxHashSet::default();
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
                kind @ LoweredStatement::If(_)
                | kind @ LoweredStatement::Loop(_)
                | kind @ LoweredStatement::Block(_)
                | kind @ LoweredStatement::Body(_)
                | kind @ LoweredStatement::Break(_)
                | kind @ LoweredStatement::Continue(_)
                | kind @ LoweredStatement::Const(_)
                | kind @ LoweredStatement::Return(_)
                | kind @ LoweredStatement::Assign(_)
                | kind @ LoweredStatement::Async { .. }
                | kind @ LoweredStatement::Select(_)
                | kind @ LoweredStatement::Switch(_)
                | kind @ LoweredStatement::Define(_)
                | kind @ LoweredStatement::VarDeclaration { .. }
                | kind @ LoweredStatement::Discard { .. }
                | kind @ LoweredStatement::ExpressionStatement { .. }
                | kind @ LoweredStatement::UnreachablePanic => list.push(Statement { line, kind }),
            }
        }
    });
}

fn drop_empty_ifs(statements: &mut Vec<Statement>) -> bool {
    let mut changed = false;
    for_each_statements_mut(statements, &mut |list| {
        for statement in mem::take(list) {
            let line = statement.line;
            match statement.kind {
                LoweredStatement::If(mut plan) if plan.condition.effects().can_erase() => {
                    drop_empty_else(&mut plan);
                    if plan.then_body.renders_empty() && matches!(plan.else_arm, ElseArm::None) {
                        changed = true;
                        if let Some(Definition { names, value }) = plan.initializer {
                            list.extend(discard_values(names.len(), value, line));
                        }
                    } else {
                        list.push(Statement {
                            line,
                            kind: LoweredStatement::If(plan),
                        });
                    }
                }
                kind @ LoweredStatement::If(_)
                | kind @ LoweredStatement::Loop(_)
                | kind @ LoweredStatement::Block(_)
                | kind @ LoweredStatement::Body(_)
                | kind @ LoweredStatement::Break(_)
                | kind @ LoweredStatement::Continue(_)
                | kind @ LoweredStatement::Const(_)
                | kind @ LoweredStatement::Return(_)
                | kind @ LoweredStatement::Assign(_)
                | kind @ LoweredStatement::Async { .. }
                | kind @ LoweredStatement::Select(_)
                | kind @ LoweredStatement::Switch(_)
                | kind @ LoweredStatement::Define(_)
                | kind @ LoweredStatement::VarDeclaration { .. }
                | kind @ LoweredStatement::Discard { .. }
                | kind @ LoweredStatement::ExpressionStatement { .. }
                | kind @ LoweredStatement::UnreachablePanic => list.push(Statement { line, kind }),
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
