use std::borrow::Borrow;

use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::passes::walk::visit_ast;
use diagnostics::LisetteDiagnostic;
use syntax::ast::{AttributeArg, Expression, Span};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LintName(EcoString);

impl Borrow<str> for LintName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

struct Allow {
    span: Span,
    lints: HashSet<LintName>,
}

#[derive(Default)]
pub(super) struct AllowIndex {
    by_file: HashMap<u32, Vec<Allow>>,
}

impl Extend<AllowIndex> for AllowIndex {
    fn extend<T: IntoIterator<Item = AllowIndex>>(&mut self, iter: T) {
        for index in iter {
            for (file_id, mut allows) in index.by_file {
                self.by_file.entry(file_id).or_default().append(&mut allows);
            }
        }
    }
}

impl FromIterator<AllowIndex> for AllowIndex {
    fn from_iter<T: IntoIterator<Item = AllowIndex>>(iter: T) -> Self {
        let mut combined = Self::default();
        combined.extend(iter);
        combined
    }
}

pub(super) fn collect_function_allows(items: &[Expression]) -> AllowIndex {
    collect_allows(items, |expression| {
        matches!(expression, Expression::Function { .. })
    })
}

pub(super) fn collect_declaration_allows(items: &[Expression]) -> AllowIndex {
    collect_allows(items, |_| true)
}

fn collect_allows(
    items: &[Expression],
    mut include: impl FnMut(&Expression) -> bool,
) -> AllowIndex {
    let mut out = AllowIndex::default();
    visit_ast(
        items,
        &mut |expression, _| {
            if !include(expression) {
                return;
            }
            let flags = allow_flags(expression);
            if !flags.is_empty() {
                let span = expression.get_span();
                out.by_file
                    .entry(span.file_id)
                    .or_default()
                    .push(Allow { span, lints: flags });
            }
        },
        &mut |_, _| {},
    );
    out
}

fn allow_flags(expression: &Expression) -> HashSet<LintName> {
    let attributes = match expression {
        Expression::Function { attributes, .. }
        | Expression::Struct { attributes, .. }
        | Expression::Enum { attributes, .. }
        | Expression::TypeAlias { attributes, .. } => attributes,
        _ => return HashSet::default(),
    };
    attributes
        .iter()
        .filter(|attribute| attribute.name == "allow")
        .flat_map(|attribute| {
            attribute.args.iter().filter_map(|arg| match arg {
                AttributeArg::Flag(name) => Some(LintName(name.as_str().into())),
                _ => None,
            })
        })
        .collect()
}

/// Drop any AST-walk lint named in an enclosing `#[allow(...)]`.
pub(super) fn filter_allowed(
    diagnostics: Vec<LisetteDiagnostic>,
    allows: &AllowIndex,
) -> Vec<LisetteDiagnostic> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| !is_allowed(diagnostic, allows, false))
        .collect()
}

pub(super) fn filter_unused_allowed(
    diagnostics: Vec<LisetteDiagnostic>,
    allows: &AllowIndex,
) -> Vec<LisetteDiagnostic> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| !is_allowed(diagnostic, allows, true))
        .collect()
}

fn is_allowed(diagnostic: &LisetteDiagnostic, allows: &AllowIndex, unused_only: bool) -> bool {
    let Some(lint_name) = diagnostic.lint_name() else {
        return false;
    };
    if unused_only && !is_suppressible_unused_lint(lint_name) {
        return false;
    }
    let Some(point) = diagnostic.location_offset() else {
        return false;
    };
    let point = point as u32;
    let Some(file_id) = diagnostic.file_id() else {
        return false;
    };
    allows.by_file.get(&file_id).is_some_and(|file_allows| {
        file_allows.iter().any(|allow| {
            allow.span.byte_offset <= point
                && point < allow.span.end()
                && allow.lints.contains(lint_name)
        })
    })
}

fn is_suppressible_unused_lint(lint_name: &str) -> bool {
    matches!(
        lint_name,
        "unused_function" | "unused_type" | "unused_struct_field" | "unused_enum_variant"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(file_id: u32, name: &str) -> AllowIndex {
        let span = Span::new(file_id, 0, 100);
        let mut lints = HashSet::default();
        lints.insert(LintName(name.into()));
        let mut by_file = HashMap::default();
        by_file.insert(file_id, vec![Allow { span, lints }]);
        AllowIndex { by_file }
    }

    #[test]
    fn allow_suppresses_matching_lint_in_same_file() {
        let allows = allow(0, "unused_function");
        let diagnostic = diagnostics::lint::unused_function(&Span::new(0, 10, 5));
        assert!(filter_allowed(vec![diagnostic], &allows).is_empty());
    }

    #[test]
    fn allow_does_not_suppress_overlapping_offset_in_another_file() {
        let allows = allow(0, "unused_function");
        let diagnostic = diagnostics::lint::unused_function(&Span::new(1, 10, 5));
        assert_eq!(filter_allowed(vec![diagnostic], &allows).len(), 1);
    }

    #[test]
    fn allow_is_specific_to_the_named_lint() {
        let allows = allow(0, "unused_type");
        let diagnostic = diagnostics::lint::unused_function(&Span::new(0, 10, 5));
        assert_eq!(filter_allowed(vec![diagnostic], &allows).len(), 1);
    }

    #[test]
    fn unused_filter_ignores_lints_outside_the_whitelist() {
        let allows = allow(0, "internal_type_leak");
        let diagnostic =
            diagnostics::lint::private_type_in_public_api(Some(&Span::new(0, 10, 5)), "T", "f");
        assert_eq!(filter_unused_allowed(vec![diagnostic], &allows).len(), 1);
    }

    #[test]
    fn unused_filter_suppresses_whitelisted_lint() {
        let allows = allow(0, "unused_function");
        let diagnostic = diagnostics::lint::unused_function(&Span::new(0, 10, 5));
        assert!(filter_unused_allowed(vec![diagnostic], &allows).is_empty());
    }

    #[test]
    fn function_allows_ignore_struct_and_enum_declarations() {
        let source = "#[allow(unused_type)]\nstruct Foo { x: int }\n";
        let items = parse_items(source);
        assert!(collect_function_allows(&items).by_file.is_empty());
        assert_eq!(collect_declaration_allows(&items).by_file.len(), 1);
    }

    fn parse_items(source: &str) -> Vec<Expression> {
        use syntax::lex::Lexer;
        use syntax::parse::Parser;
        let tokens = Lexer::new(source, 0).lex().tokens;
        Parser::new(tokens, source).parse().ast
    }
}
