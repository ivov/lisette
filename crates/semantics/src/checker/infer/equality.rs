use syntax::ast::{Expression, ExpressionId};
use syntax::program::{CallOperation, CheckedBodies, EqualityRecipe, NativeTypeKind, ValueUseKey};

use crate::checker::infer::expressions::comparison::select_equality;
use crate::store::Store;

/// The equality of each built-in comparison and each asserted `equals` call under `expression`, keyed by the comparison.
pub(super) fn select_equalities(
    store: &Store,
    package: &str,
    bodies: &CheckedBodies,
    expression: &Expression,
    selected: &mut Vec<(ExpressionId, EqualityRecipe)>,
) {
    let select = |ty| select_equality(store, ty, package, &[]);
    match expression {
        Expression::Call {
            expression: callee,
            args,
            ..
        } => {
            let callee = callee.unwrap_parens();
            if let Some(CallOperation::Native { kind, method }) = bodies
                .callee(&ValueUseKey::value(callee))
                .map(|callee| &callee.operation)
                && let Some(receiver) = match callee {
                    Expression::DotAccess { expression, .. } => Some(expression.as_ref()),
                    _ => args.first(),
                }
            {
                let receiver_type = store.peel_alias(&receiver.get_type().strip_refs());
                let compares = match (method.as_str(), kind) {
                    ("equals", NativeTypeKind::Slice | NativeTypeKind::Map) => {
                        receiver_type.is_slice() || receiver_type.is_map()
                    }
                    ("contains", NativeTypeKind::Slice) => receiver_type.is_slice(),
                    _ => false,
                };
                if compares {
                    selected.push((callee.id(), select(&receiver_type)));
                }
            }
        }
        Expression::Assert {
            expression: operand,
            id,
            ..
        } => {
            if let Expression::Call {
                expression: callee,
                args,
                ..
            } = operand.unwrap_parens()
                && let Expression::DotAccess {
                    expression: receiver,
                    member,
                    ..
                } = callee.unwrap_parens()
                && member == "equals"
                && args.len() == 1
            {
                let recipe = select(&receiver.get_type());
                if recipe != EqualityRecipe::Operator {
                    selected.push((*id, recipe));
                }
            }
        }
        _ => {}
    }
    expression.for_each_child(|child| {
        select_equalities(store, package, bodies, child, selected);
    });
}
