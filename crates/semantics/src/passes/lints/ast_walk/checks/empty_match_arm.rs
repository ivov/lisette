use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;

pub fn check_empty_match_arm(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Match { arms, .. } = expression else {
        return;
    };

    for arm in arms {
        if let Expression::Block { items, span, .. } = &*arm.expression
            && items.is_empty()
        {
            diagnostics.push(diagnostics::lint::empty_match_arm(span));
        }
    }
}
