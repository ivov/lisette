use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;

use super::helpers::{expressions_equivalent, is_side_effect_free};

pub fn check_manual_compound_assignment(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Assignment {
        target,
        value,
        compound_operator: None,
        span,
    } = expression
    else {
        return;
    };

    let Expression::Binary { operator, left, .. } = value.unwrap_parens() else {
        return;
    };

    let Some(symbol) = operator.compound_assignment_symbol() else {
        return;
    };

    if !is_side_effect_free(target) || !expressions_equivalent(target, left) {
        return;
    }

    diagnostics.push(diagnostics::lint::manual_compound_assignment(span, symbol));
}
