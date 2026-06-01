use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;

use super::helpers::bool_literal;

pub fn check_unnecessary_bool(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::If {
        consequence,
        alternative,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (Some(then_value), Some(else_value)) = (
        block_single_bool(consequence),
        block_single_bool(alternative),
    ) else {
        return;
    };

    // Equal literals are `identical_if_branches`, not a condition rewrite.
    if then_value == else_value {
        return;
    }

    diagnostics.push(diagnostics::lint::unnecessary_bool(span, then_value));
}

fn block_single_bool(expression: &Expression) -> Option<bool> {
    let Expression::Block { items, .. } = expression else {
        return None;
    };
    let [only] = items.as_slice() else {
        return None;
    };
    bool_literal(only.unwrap_parens())
}
