use super::name_uses::NameUses;

use crate::target::ir::bodies::{
    AssignForm, CompoundKind, Definition, LoweredStatement, SelectArmKind, Statement, SwitchKind,
    for_each_statements_mut,
};
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;

pub(super) fn inline_return_aliases(statements: &mut Vec<Statement>) {
    let uses = NameUses::of(statements);
    for_each_statements_mut(statements, &mut |list| {
        let mut index = 0;
        while index + 1 < list.len() {
            let Some(name) = returned_alias(&list[index].kind, &list[index + 1].kind) else {
                index += 1;
                continue;
            };
            if uses.reads(&name) != 1 || uses.bindings(&name) != 1 || uses.is_written(&name) {
                index += 1;
                continue;
            }
            let LoweredStatement::Define(Definition { value, .. }) = list.remove(index).kind else {
                unreachable!("returned_alias matched a definition");
            };
            let LoweredStatement::Return(values) = &mut list[index].kind else {
                unreachable!("returned_alias matched a return");
            };
            values[0] = value;
            index += 1;
        }
    });
}

fn returned_alias(definition: &LoweredStatement, next: &LoweredStatement) -> Option<GoIdentifier> {
    let LoweredStatement::Define(Definition { names, value }) = definition else {
        return None;
    };
    let [name] = names.as_slice() else {
        return None;
    };
    if value.effects().runs_code() {
        return None;
    }
    let LoweredStatement::Return(values) = next else {
        return None;
    };
    let [returned] = values.as_slice() else {
        return None;
    };
    matches!(returned.node(), GoExpressionNode::Identifier(read) if read.refers_to_same(name))
        .then(|| name.clone())
}

pub(super) fn inline_name_aliases(statements: &mut Vec<Statement>) {
    let uses = NameUses::of(statements);
    for_each_statements_mut(statements, &mut |list| {
        let mut index = 0;
        while index + 1 < list.len() {
            if let Some((temp, source)) = alias_define(&list[index].kind)
                && uses.reads(&temp) == 1
                && uses.bindings(&temp) == 1
                && !uses.is_written(&temp)
                && replace_only_read(&mut list[index + 1].kind, &temp, &source)
            {
                list.remove(index);
            } else {
                index += 1;
            }
        }
    });
}

fn alias_define(statement: &LoweredStatement) -> Option<(GoIdentifier, GoIdentifier)> {
    match statement {
        LoweredStatement::Define(Definition { names, value }) => {
            match (names.as_slice(), value.node()) {
                ([temp], GoExpressionNode::Identifier(source))
                    if go_name::is_plain_identifier(source) && !temp.refers_to_same(source) =>
                {
                    Some((temp.clone(), source.clone()))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn replace_only_read(
    statement: &mut LoweredStatement,
    temp: &GoIdentifier,
    source: &GoIdentifier,
) -> bool {
    match statement {
        LoweredStatement::Define(Definition { names, value }) => {
            !names
                .iter()
                .any(|name| name.spelling() == source.spelling())
                && rename_in(vec![value.node_mut()], None, temp, source)
        }
        LoweredStatement::Assign(AssignForm::Simple { target, value }) => rename_in(
            vec![target.node_mut(), value.node_mut()],
            Some(0),
            temp,
            source,
        ),
        LoweredStatement::Assign(AssignForm::Compound {
            target,
            kind: CompoundKind::OpAssign { rhs, .. },
        }) => rename_in(
            vec![target.node_mut(), rhs.node_mut()],
            Some(0),
            temp,
            source,
        ),
        LoweredStatement::Return(values) => rename_in(
            values.iter_mut().map(GoExpression::node_mut).collect(),
            None,
            temp,
            source,
        ),
        LoweredStatement::ExpressionStatement { expression, .. } => {
            rename_in(vec![expression.node_mut()], None, temp, source)
        }
        LoweredStatement::If(plan) if plan.initializer.is_none() => {
            rename_in(vec![plan.condition.node_mut()], None, temp, source)
        }
        LoweredStatement::Switch(plan) => match &mut plan.kind {
            SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                rename_in(vec![subject.node_mut()], None, temp, source)
            }
            SwitchKind::Conditional => false,
        },
        LoweredStatement::Select(plan) => {
            let siblings = plan
                .arms
                .iter_mut()
                .flat_map(|arm| match &mut arm.kind {
                    SelectArmKind::Receive { channel, .. } => vec![channel.node_mut()],
                    SelectArmKind::Send { channel, value, .. } => {
                        vec![channel.node_mut(), value.node_mut()]
                    }
                    SelectArmKind::Default { .. } => Vec::new(),
                })
                .collect();
            rename_in(siblings, None, temp, source)
        }
        _ => false,
    }
}

fn rename_in(
    siblings: Vec<&mut GoExpressionNode>,
    written: Option<usize>,
    temp: &GoIdentifier,
    source: &GoIdentifier,
) -> bool {
    let analyses: Vec<Analysis> = siblings
        .iter()
        .map(|sibling| analyze(sibling, temp))
        .collect();
    let reader = analyses.iter().position(|analysis| analysis.reads > 0);
    let Some(reader) = reader else {
        return false;
    };
    if written == Some(reader) || analyses[reader].reads != 1 || !analyses[reader].ordered {
        return false;
    }
    let other_work = analyses
        .iter()
        .enumerate()
        .any(|(index, analysis)| index != reader && analysis.works);
    if other_work {
        return false;
    }
    let mut siblings = siblings;
    replace_identifier(siblings[reader], temp, source);
    true
}

pub(super) fn replace_identifier(
    node: &mut GoExpressionNode,
    from: &GoIdentifier,
    to: &GoIdentifier,
) {
    match node {
        GoExpressionNode::Identifier(name) if name.refers_to_same(from) => *name = to.clone(),
        _ => node.visit_children_mut(&mut |child| replace_identifier(child, from, to)),
    }
}

struct Analysis {
    reads: usize,
    works: bool,
    ordered: bool,
}

fn analyze(node: &GoExpressionNode, temp: &GoIdentifier) -> Analysis {
    if let GoExpressionNode::Identifier(name) = node {
        return Analysis {
            reads: usize::from(name.refers_to_same(temp)),
            works: false,
            ordered: true,
        };
    }
    if let GoExpressionNode::FunctionLiteral { body, .. } = node {
        let mut reads = 0;
        body.visit_expressions(&mut |inner| {
            if let GoExpressionNode::Identifier(name) = inner
                && name.refers_to_same(temp)
            {
                reads += 1;
            }
        });
        return Analysis {
            reads,
            works: false,
            ordered: reads == 0,
        };
    }
    if let GoExpressionNode::Verbatim(_) = node {
        return Analysis {
            reads: 0,
            works: true,
            ordered: true,
        };
    }
    let identity_read = match node {
        GoExpressionNode::AddressOf(operand) | GoExpressionNode::Slice { base: operand, .. } => {
            operand.mentions_local(temp)
        }
        GoExpressionNode::Call { callee, .. } => {
            matches!(callee.as_ref(), GoExpressionNode::Selector { base, .. } if base.mentions_local(temp))
        }
        _ => false,
    };
    let mut children = Vec::new();
    node.visit_children(&mut |child| children.push(analyze(child, temp)));
    let reads = children.iter().map(|child| child.reads).sum();
    let reader = children.iter().position(|child| child.reads > 0);
    let other_work = children
        .iter()
        .enumerate()
        .any(|(index, child)| Some(index) != reader && child.works);
    let ordered = children.iter().all(|child| child.ordered)
        && !(reader.is_some() && other_work)
        && !identity_read;
    Analysis {
        reads,
        works: !Effects::local_read().can_move_across(node.effects()),
        ordered,
    }
}
