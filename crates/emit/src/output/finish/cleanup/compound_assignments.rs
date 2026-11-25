use crate::target::ir::bodies::{
    AssignForm, CompoundKind, LoweredStatement, Statement, for_each_statements_mut,
};
use crate::target::ir::go_expression::GoLiteral;
use crate::target::ir::go_expression::{GoBinaryOperator, GoExpressionNode};
use crate::target::ir::values::GoExpression;

pub(super) fn fold_compound_assignments(statements: &mut Vec<Statement>) {
    for_each_statements_mut(statements, &mut |block| {
        for statement in block.iter_mut() {
            if let Some(folded) = folded_compound_assignment(&statement.kind) {
                statement.kind = folded;
            }
        }
    });
}

fn folded_compound_assignment(statement: &LoweredStatement) -> Option<LoweredStatement> {
    let LoweredStatement::Assign(AssignForm::Simple { target, value }) = statement else {
        return None;
    };
    let GoExpressionNode::Identifier(name) = target.node() else {
        return None;
    };
    let GoExpressionNode::Binary {
        operator,
        left,
        right,
        ..
    } = value.node()
    else {
        return None;
    };

    if !matches!(left.as_ref(), GoExpressionNode::Identifier(read) if read.refers_to_same(name))
        || !operator.has_compound_form()
    {
        return None;
    }

    let kind = match (operator, right.as_ref()) {
        (GoBinaryOperator::Addition, GoExpressionNode::Literal(GoLiteral::Int(1))) => {
            CompoundKind::Increment
        }
        (GoBinaryOperator::Subtraction, GoExpressionNode::Literal(GoLiteral::Int(1))) => {
            CompoundKind::Decrement
        }
        _ => CompoundKind::OperatorAssign {
            operator: *operator,
            rhs: GoExpression::from_operand(right.clone()),
        },
    };

    Some(LoweredStatement::Assign(AssignForm::Compound {
        target: target.clone(),
        kind,
    }))
}
