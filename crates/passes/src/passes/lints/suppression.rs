use crate::passes::walk::visit_ast;
use diagnostics::LisetteDiagnostic;
use syntax::ast::{AttributeArg, Expression, Span};

const SUPPRESSIBLE_UNUSED_LINTS: &[&str] = &[
    "unused_function",
    "unused_type",
    "unused_struct_field",
    "unused_enum_variant",
];

pub(super) fn collect_function_allows(items: &[Expression]) -> Vec<(Span, Vec<String>)> {
    collect_allows(items, |expression| {
        matches!(expression, Expression::Function { .. })
    })
}

pub(super) fn collect_declaration_allows(items: &[Expression]) -> Vec<(Span, Vec<String>)> {
    collect_allows(items, |_| true)
}

fn collect_allows(
    items: &[Expression],
    mut include: impl FnMut(&Expression) -> bool,
) -> Vec<(Span, Vec<String>)> {
    let mut out = Vec::new();
    visit_ast(
        items,
        &mut |expression, _| {
            if !include(expression) {
                return;
            }
            let flags = allow_flags(expression);
            if !flags.is_empty() {
                out.push((expression.get_span(), flags));
            }
        },
        &mut |_, _| {},
    );
    out
}

fn allow_flags(expression: &Expression) -> Vec<String> {
    let attributes = match expression {
        Expression::Function { attributes, .. }
        | Expression::Struct { attributes, .. }
        | Expression::Enum { attributes, .. }
        | Expression::TypeAlias { attributes, .. } => attributes,
        _ => return Vec::new(),
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

/// Drop any AST-walk lint named in an enclosing `#[allow(...)]`.
pub(super) fn filter_allowed(
    diagnostics: Vec<LisetteDiagnostic>,
    allows: &[(Span, Vec<String>)],
) -> Vec<LisetteDiagnostic> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| !is_allowed(diagnostic, allows, None))
        .collect()
}

pub(super) fn filter_unused_allowed(
    diagnostics: Vec<LisetteDiagnostic>,
    allows: &[(Span, Vec<String>)],
) -> Vec<LisetteDiagnostic> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| !is_allowed(diagnostic, allows, Some(SUPPRESSIBLE_UNUSED_LINTS)))
        .collect()
}

fn is_allowed(
    diagnostic: &LisetteDiagnostic,
    allows: &[(Span, Vec<String>)],
    restrict_to: Option<&[&str]>,
) -> bool {
    let Some(lint_name) = diagnostic.lint_name() else {
        return false;
    };
    if let Some(restrict_to) = restrict_to
        && !restrict_to.contains(&lint_name)
    {
        return false;
    }
    let Some(point) = diagnostic.location_offset() else {
        return false;
    };
    let point = point as u32;
    let file_id = diagnostic.file_id();
    allows.iter().any(|(span, flags)| {
        Some(span.file_id) == file_id
            && span.byte_offset <= point
            && point < span.end()
            && flags.iter().any(|flag| flag == lint_name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(file_id: u32, name: &str) -> (Span, Vec<String>) {
        (Span::new(file_id, 0, 100), vec![name.to_string()])
    }

    #[test]
    fn allow_suppresses_matching_lint_in_same_file() {
        let allows = [allow(0, "unused_function")];
        let diagnostic = diagnostics::lint::unused_function(&Span::new(0, 10, 5));
        assert!(filter_allowed(vec![diagnostic], &allows).is_empty());
    }

    #[test]
    fn allow_does_not_suppress_overlapping_offset_in_another_file() {
        let allows = [allow(0, "unused_function")];
        let diagnostic = diagnostics::lint::unused_function(&Span::new(1, 10, 5));
        assert_eq!(filter_allowed(vec![diagnostic], &allows).len(), 1);
    }

    #[test]
    fn allow_is_specific_to_the_named_lint() {
        let allows = [allow(0, "unused_type")];
        let diagnostic = diagnostics::lint::unused_function(&Span::new(0, 10, 5));
        assert_eq!(filter_allowed(vec![diagnostic], &allows).len(), 1);
    }

    #[test]
    fn unused_filter_ignores_lints_outside_the_whitelist() {
        let allows = [allow(0, "internal_type_leak")];
        let diagnostic =
            diagnostics::lint::private_type_in_public_api(Some(&Span::new(0, 10, 5)), "T", "f");
        assert_eq!(filter_unused_allowed(vec![diagnostic], &allows).len(), 1);
    }

    #[test]
    fn unused_filter_suppresses_whitelisted_lint() {
        let allows = [allow(0, "unused_function")];
        let diagnostic = diagnostics::lint::unused_function(&Span::new(0, 10, 5));
        assert!(filter_unused_allowed(vec![diagnostic], &allows).is_empty());
    }

    #[test]
    fn function_allows_ignore_struct_and_enum_declarations() {
        let source = "#[allow(unused_type)]\nstruct Foo { x: int }\n";
        let items = parse_items(source);
        assert!(collect_function_allows(&items).is_empty());
        assert_eq!(collect_declaration_allows(&items).len(), 1);
    }

    fn parse_items(source: &str) -> Vec<Expression> {
        use syntax::lex::Lexer;
        use syntax::parse::Parser;
        let tokens = Lexer::new(source, 0).lex().tokens;
        Parser::new(tokens, source).parse().ast
    }
}
