use crate::passes::walk::visit_ast;
use diagnostics::LisetteDiagnostic;
use syntax::ast::{AttributeArg, Expression, Span};

/// Every declaration carrying `#[allow(...)]`, as (span, allowed lint names); a
/// diagnostic whose location falls inside such a span is suppressed.
pub(super) fn collect_declaration_allows(items: &[Expression]) -> Vec<(Span, Vec<String>)> {
    let mut out = Vec::new();
    visit_ast(
        items,
        &mut |expression| {
            let flags = allow_flags(expression);
            if !flags.is_empty() {
                out.push((expression.get_span(), flags));
            }
        },
        &mut |_| {},
    );
    out
}

fn allow_flags(expression: &Expression) -> Vec<String> {
    let Expression::Function { attributes, .. } = expression else {
        return Vec::new();
    };
    attributes
        .iter()
        .filter(|attribute| attribute.name == "allow")
        .flat_map(|attribute| {
            attribute.args.iter().filter_map(|arg| match arg {
                AttributeArg::Flag(name) => Some(name.clone()),
                _ => None,
            })
        })
        .collect()
}

pub(super) fn filter_allowed(
    diagnostics: Vec<LisetteDiagnostic>,
    allows: &[(Span, Vec<String>)],
) -> Vec<LisetteDiagnostic> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| !is_allowed(diagnostic, allows))
        .collect()
}

fn is_allowed(diagnostic: &LisetteDiagnostic, allows: &[(Span, Vec<String>)]) -> bool {
    let Some(lint_name) = diagnostic.lint_name() else {
        return false;
    };
    let Some(point) = diagnostic.location_offset() else {
        return false;
    };
    let point = point as u32;
    allows.iter().any(|(span, flags)| {
        span.byte_offset <= point
            && point < span.end()
            && flags.iter().any(|flag| flag == lint_name)
    })
}
