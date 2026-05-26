use diagnostics::LisetteDiagnostic;
use syntax::ast::{Expression, Literal, Pattern};

pub fn check_unnecessary_raw_string_expression(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Literal {
        literal: Literal::String { value, raw: true },
        span,
        ..
    } = expression
    else {
        return;
    };
    if !value.contains('\\') {
        diagnostics.push(diagnostics::lint::unnecessary_raw_string(span));
    }
}

pub fn check_unnecessary_raw_string_pattern(
    pattern: &Pattern,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Pattern::Literal {
        literal: Literal::String { value, raw: true },
        span,
        ..
    } = pattern
    else {
        return;
    };
    if !value.contains('\\') {
        diagnostics.push(diagnostics::lint::unnecessary_raw_string(span));
    }
}
