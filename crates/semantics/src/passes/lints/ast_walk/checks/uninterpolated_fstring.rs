use diagnostics::LisetteDiagnostic;
use syntax::ast::{Expression, FormatStringPart, Literal};

pub fn check_uninterpolated_fstring(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Literal {
        literal: Literal::FormatString(parts),
        span,
        ..
    } = expression
    else {
        return;
    };

    let has_interpolation = parts
        .iter()
        .any(|p| matches!(p, FormatStringPart::Expression(_)));

    if !has_interpolation {
        diagnostics.push(diagnostics::lint::uninterpolated_fstring(span));
    }
}
