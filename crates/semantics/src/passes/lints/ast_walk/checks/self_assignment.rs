use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;

pub fn check_self_assignment(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Assignment {
        target,
        value,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (
        Expression::Identifier {
            value: target_name, ..
        },
        Expression::Identifier {
            value: value_name, ..
        },
    ) = (target.unwrap_parens(), value.unwrap_parens())
    else {
        return;
    };

    if target_name != value_name {
        return;
    }

    diagnostics.push(diagnostics::lint::self_assignment(span));
}
