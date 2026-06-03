use diagnostics::LisetteDiagnostic;
use syntax::ast::{Expression, MatchOrigin, Pattern, Span};

pub fn check_match_single_binding(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Match {
        arms,
        origin: MatchOrigin::Explicit,
        span,
        ..
    } = expression
    else {
        return;
    };

    let [arm] = arms.as_slice() else {
        return;
    };

    if arm.has_guard() {
        return;
    }

    let Pattern::Identifier { identifier, .. } = &arm.pattern else {
        return;
    };

    let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);
    diagnostics.push(diagnostics::lint::match_single_binding(
        &match_keyword_span,
        identifier,
    ));
}
