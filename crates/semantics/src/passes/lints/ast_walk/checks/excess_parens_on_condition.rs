use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;

pub fn check_excess_parens_on_condition(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let (condition, keyword) = match expression {
        Expression::If { condition, .. } => (condition.as_ref(), "if"),
        Expression::While { condition, .. } => (condition.as_ref(), "while"),
        Expression::Match { subject, .. } => (subject.as_ref(), "match"),
        _ => return,
    };

    if let Expression::Paren { span, .. } = condition {
        diagnostics.push(diagnostics::lint::unnecessary_parens(span, keyword));
    }
}
