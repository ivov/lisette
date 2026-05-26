use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;

use super::helpers::{expressions_equivalent, is_empty_block};

pub fn check_identical_if_branches(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::If {
        consequence,
        alternative,
        span,
        ..
    } = expression
    else {
        return;
    };

    // `else if` chains: each arm is checked independently; comparing the
    // chain tail against the head produces noisy false positives.
    if matches!(
        alternative.as_ref(),
        Expression::If { .. } | Expression::IfLet { .. }
    ) {
        return;
    }

    // Empty blocks are usually in-progress stubs; do not add noise on top of
    // other lints that already cover that case.
    if is_empty_block(consequence) || is_empty_block(alternative) {
        return;
    }

    if !expressions_equivalent(consequence, alternative) {
        return;
    }

    diagnostics.push(diagnostics::lint::identical_if_branches(span));
}
