use super::name_uses::NameUses;
use super::terminal_else::drop_empty_else;
use std::mem;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::target::ir::bodies::{
    Definition, LoweredStatement, Statement, direct_first, for_each_statements_mut,
};
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;

pub(super) fn drop_unread_temps(statements: &mut Vec<Statement>, shadowing: &HashSet<LocalId>) {
    while let Some(name) = find_unread_temp(statements, shadowing) {
        remove_temp(statements, &name);
    }
}

fn find_unread_temp(
    statements: &mut Vec<Statement>,
    shadowing: &HashSet<LocalId>,
) -> Option<GoIdentifier> {
    let uses = NameUses::of(statements);
    let discards = discard_counts(statements);
    let mut found = None;
    for_each_statements_mut(statements, &mut |list| {
        if found.is_none() {
            found = list
                .iter()
                .find_map(|statement| unread_temp(statement, &uses, &discards, shadowing));
        }
    });
    found
}

fn discard_counts(statements: &mut Vec<Statement>) -> HashMap<LocalId, usize> {
    let mut discards: HashMap<LocalId, usize> = HashMap::default();
    for_each_statements_mut(statements, &mut |list| {
        for statement in list.iter() {
            if let Some(id) = discarded_name(&statement.kind).and_then(GoIdentifier::id) {
                *discards.entry(id).or_default() += 1;
            }
        }
    });
    discards
}

fn unread_temp(
    statement: &Statement,
    uses: &NameUses,
    discards: &HashMap<LocalId, usize>,
    shadowing: &HashSet<LocalId>,
) -> Option<GoIdentifier> {
    let (name, value) = pure_define(&statement.kind)?;
    if name.id().is_some_and(|id| shadowing.contains(&id)) {
        return None;
    }
    let discard_count = name
        .id()
        .and_then(|id| discards.get(&id))
        .copied()
        .unwrap_or_default();
    let unread = uses.value_reads(name) == discard_count
        && uses.bindings(name) == 1
        && !uses.is_written(name);
    let mut sources = Vec::new();
    value.node().visit(&mut |node| {
        if let GoExpressionNode::Identifier(source) = node {
            sources.push(source.clone());
        }
    });
    let read_elsewhere = |source: &GoIdentifier| {
        let reads_in_value = sources.iter().filter(|s| s.refers_to_same(source)).count();
        uses.value_reads(source) > reads_in_value
    };
    (unread && sources.iter().all(read_elsewhere)).then(|| name.clone())
}

fn remove_temp(statements: &mut Vec<Statement>, name: &GoIdentifier) {
    let defines = |statement: &Statement| {
        pure_define(&statement.kind).is_some_and(|(defined, _)| defined.refers_to_same(name))
    };
    for_each_statements_mut(statements, &mut |list| {
        for statement in mem::take(list) {
            match statement.kind {
                LoweredStatement::Block(mut body)
                    if body.statements.iter().any(defines)
                        && body.statements.iter().filter(|s| declares(s)).count() == 1 =>
                {
                    body.statements.retain(|statement| !defines(statement));
                    direct_first(&mut body.statements, statement.line);
                    list.extend(body.statements);
                }
                kind => list.push(Statement {
                    line: statement.line,
                    kind,
                }),
            }
        }
    });
    for_each_statements_mut(statements, &mut |list| {
        list.retain(|statement| {
            !defines(statement)
                && discarded_name(&statement.kind)
                    .is_none_or(|discarded| !discarded.refers_to_same(name))
        });
    });
    for_each_statements_mut(statements, &mut |list| {
        for statement in list {
            if let LoweredStatement::If(plan) = &mut statement.kind {
                drop_empty_else(plan);
            }
        }
    });
}

fn declares(statement: &Statement) -> bool {
    matches!(
        statement.kind,
        LoweredStatement::Define(_) | LoweredStatement::VarDecl { .. } | LoweredStatement::Const(_)
    )
}

fn pure_define(statement: &LoweredStatement) -> Option<(&GoIdentifier, &GoExpression)> {
    match statement {
        LoweredStatement::Define(Definition { names, value }) => match names.as_slice() {
            [name] if value.effects().can_erase() => Some((name, value)),
            _ => None,
        },
        _ => None,
    }
}

fn discarded_name(statement: &LoweredStatement) -> Option<&GoIdentifier> {
    match statement {
        LoweredStatement::Discard {
            arity: 1,
            value: expression,
        } => match expression.node() {
            GoExpressionNode::Identifier(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}
