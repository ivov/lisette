use super::aliases::replace_identifier;
use super::name_uses::NameUses;
use crate::target::ir::go_expression::GoLiteral;

use crate::target::ir::bodies::{
    AssignForm, Definition, ElseArm, LoopKind, LoopTransfer, LoweredBlock, LoweredStatement,
    Statement, for_each_statements_mut,
};
use crate::target::ir::go_expression::{GoExpressionNode, GoUnaryOperator};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;

pub(super) fn return_found_elements_directly(statements: &mut Vec<Statement>) {
    for_each_statements_mut(statements, &mut |block| {
        let uses = NameUses::of(block);
        let Some(found) = search_loop_return(block) else {
            return;
        };
        if uses.reads(&found.flag) != 2 || uses.bindings(&found.flag) != 1 {
            return;
        }
        if let Some(value) = &found.value
            && (uses.reads(value) != 2 || uses.bindings(value) != 1)
        {
            return;
        }

        let SearchLoopReturn {
            declares_at,
            loop_at,
            value,
            element,
            mut success,
            failure,
            hit_body,
            ..
        } = found;
        if let (Some(value), Some(element)) = (&value, &element) {
            for returned in &mut success {
                replace_identifier(returned.node_mut(), value, element);
            }
        }
        *hit_body = LoweredBlock {
            statements: vec![LoweredStatement::Return(success).into()],
        };

        block.truncate(loop_at + 1);
        block.push(LoweredStatement::Return(failure).into());
        block.drain(declares_at..loop_at);
    });
}

struct SearchLoopReturn<'b> {
    declares_at: usize,
    loop_at: usize,
    flag: GoIdentifier,
    value: Option<GoIdentifier>,
    element: Option<GoIdentifier>,
    success: Vec<GoExpression>,
    failure: Vec<GoExpression>,
    hit_body: &'b mut LoweredBlock,
}

fn search_loop_return(block: &mut [Statement]) -> Option<SearchLoopReturn<'_>> {
    let loop_at = block.len().checked_sub(3)?;
    let define_at = loop_at.checked_sub(1)?;
    let flag = false_define(&block[define_at].kind)?;
    let (declares_at, value) = match define_at.checked_sub(1).map(|at| (at, &block[at].kind)) {
        Some((
            at,
            LoweredStatement::VarDeclaration {
                name, value: None, ..
            },
        )) => (at, Some(name.clone())),
        _ => (define_at, None),
    };

    let LoweredStatement::Loop(plan) = &block[loop_at].kind else {
        return None;
    };

    if !matches!(plan.kind, LoopKind::FoundFlagSearch) {
        return None;
    }
    let element = found_hit(&plan.body.statements.last()?.kind, &flag, value.as_ref())?;
    let failure = flag_guarded_return(&block[loop_at + 1].kind, &flag)?;
    let LoweredStatement::Return(success) = &block[loop_at + 2].kind else {
        return None;
    };
    let success = success.clone();
    let LoweredStatement::Loop(plan) = &mut block[loop_at].kind else {
        return None;
    };
    let LoweredStatement::If(hit) = &mut plan.body.statements.last_mut()?.kind else {
        return None;
    };
    Some(SearchLoopReturn {
        declares_at,
        loop_at,
        flag,
        value,
        element,
        success,
        failure,
        hit_body: &mut hit.then_body,
    })
}

fn false_define(statement: &LoweredStatement) -> Option<GoIdentifier> {
    let LoweredStatement::Define(Definition { names, value }) = statement else {
        return None;
    };
    let [name] = names.as_slice() else {
        return None;
    };
    matches!(
        value.node(),
        GoExpressionNode::Literal(GoLiteral::Bool(false))
    )
    .then(|| name.clone())
}

fn found_hit(
    statement: &LoweredStatement,
    flag: &GoIdentifier,
    value: Option<&GoIdentifier>,
) -> Option<Option<GoIdentifier>> {
    let LoweredStatement::If(plan) = statement else {
        return None;
    };
    if !matches!(plan.else_arm, ElseArm::None) || plan.initializer.is_some() {
        return None;
    }
    let body = plan.then_body.statements.as_slice();
    let (element, rest) = match (value, body) {
        (Some(value), [first, rest @ ..]) => (Some(assigned_name(&first.kind, value)?), rest),
        (None, rest) => (None, rest),
        _ => return None,
    };
    match rest {
        [raise, exit]
            if assigns_true(&raise.kind, flag)
                && matches!(exit.kind, LoweredStatement::Break(LoopTransfer::Unlabeled)) =>
        {
            Some(element)
        }
        _ => None,
    }
}

fn assigned_name(statement: &LoweredStatement, target: &GoIdentifier) -> Option<GoIdentifier> {
    let LoweredStatement::Assign(AssignForm::Simple {
        target: place,
        value,
    }) = statement
    else {
        return None;
    };

    if !matches!(place.node(), GoExpressionNode::Identifier(name) if name.refers_to_same(target)) {
        return None;
    }
    value.as_identifier().cloned()
}

fn assigns_true(statement: &LoweredStatement, target: &GoIdentifier) -> bool {
    let LoweredStatement::Assign(AssignForm::Simple {
        target: place,
        value,
    }) = statement
    else {
        return false;
    };

    matches!(place.node(), GoExpressionNode::Identifier(name) if name.refers_to_same(target))
        && matches!(
            value.node(),
            GoExpressionNode::Literal(GoLiteral::Bool(true))
        )
}

fn flag_guarded_return(
    statement: &LoweredStatement,
    flag: &GoIdentifier,
) -> Option<Vec<GoExpression>> {
    let LoweredStatement::If(plan) = statement else {
        return None;
    };
    if !matches!(plan.else_arm, ElseArm::None) || plan.initializer.is_some() {
        return None;
    }
    let GoExpressionNode::Unary {
        operator: GoUnaryOperator::Not,
        operand,
    } = plan.condition.node()
    else {
        return None;
    };
    let GoExpressionNode::Identifier(name) = operand.as_ref() else {
        return None;
    };
    if !name.refers_to_same(flag) {
        return None;
    }
    match plan.then_body.statements.as_slice() {
        [
            Statement {
                kind: LoweredStatement::Return(values),
                ..
            },
        ] => Some(values.clone()),
        _ => None,
    }
}
