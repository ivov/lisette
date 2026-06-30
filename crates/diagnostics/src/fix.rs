use syntax::ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    span: Span,
    content: Box<str>,
}

impl Edit {
    pub fn replacement(span: Span, content: impl Into<Box<str>>) -> Self {
        Self {
            span,
            content: content.into(),
        }
    }

    pub fn deletion(span: Span) -> Self {
        Self {
            span,
            content: Box::from(""),
        }
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A single applicable fix for one diagnostic.
#[derive(Debug, Clone)]
pub struct Fix {
    message: String,
    edit: Edit,
}

impl Fix {
    pub fn new(message: impl Into<String>, edit: Edit) -> Self {
        Self {
            message: message.into(),
            edit,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn edit(&self) -> &Edit {
        &self.edit
    }
}

#[derive(Debug)]
pub struct FixApplicationOutcome {
    pub source: String,
    pub applied: usize,
}

/// Apply `fixes` to `source` in a single left-to-right pass.
///
/// Fixes are sorted by span. A fix whose edit starts at or before the last
/// applied edit ended is skipped (boundary-adjacent spans count as overlapping)
/// and stays reported as a diagnostic on the next run.
pub fn apply_fixes(source: &str, mut fixes: Vec<&Fix>) -> FixApplicationOutcome {
    fixes.sort_by_key(|fix| {
        let span = fix.edit().span();
        (span.byte_offset, span.end())
    });

    let mut out = String::with_capacity(source.len());
    let mut last_end: Option<u32> = None;
    let mut applied = 0;

    for fix in fixes {
        let span = fix.edit().span();
        if last_end.is_some_and(|end| span.byte_offset <= end) {
            continue;
        }
        out.push_str(&source[last_end.unwrap_or(0) as usize..span.byte_offset as usize]);
        out.push_str(fix.edit().content());
        last_end = Some(span.end());
        applied += 1;
    }

    out.push_str(&source[last_end.unwrap_or(0) as usize..]);
    FixApplicationOutcome {
        source: out,
        applied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(offset: u32, len: u32) -> Span {
        Span::new(0, offset, len)
    }

    #[test]
    fn applies_single_replacement() {
        let fix = Fix::new("x", Edit::replacement(span(0, 5), "y"));
        let applied = apply_fixes("hello world", vec![&fix]);
        assert_eq!(applied.source, "y world");
        assert_eq!(applied.applied, 1);
    }

    #[test]
    fn applies_deletion() {
        let fix = Fix::new("drop", Edit::deletion(span(5, 6)));
        let applied = apply_fixes("hello world!", vec![&fix]);
        assert_eq!(applied.source, "hello!");
    }

    #[test]
    fn applies_non_overlapping_fixes_left_to_right() {
        let a = Fix::new("a", Edit::replacement(span(0, 1), "A"));
        let b = Fix::new("b", Edit::replacement(span(2, 1), "C"));
        let applied = apply_fixes("abc", vec![&b, &a]);
        assert_eq!(applied.source, "AbC");
        assert_eq!(applied.applied, 2);
    }

    #[test]
    fn skips_overlapping_fix() {
        let a = Fix::new("a", Edit::replacement(span(0, 3), "X"));
        let b = Fix::new("b", Edit::replacement(span(2, 3), "Y"));
        let applied = apply_fixes("abcdef", vec![&a, &b]);
        assert_eq!(applied.source, "Xdef");
        assert_eq!(applied.applied, 1);
    }

    #[test]
    fn treats_boundary_adjacent_as_overlap() {
        let a = Fix::new("a", Edit::replacement(span(0, 2), "X"));
        let b = Fix::new("b", Edit::replacement(span(2, 2), "Y"));
        let applied = apply_fixes("abcd", vec![&a, &b]);
        assert_eq!(applied.source, "Xcd");
        assert_eq!(applied.applied, 1);
    }

    #[test]
    fn malformed_fix_output_is_caught_by_reparse() {
        let fix = Fix::new("bad", Edit::replacement(span(0, 1), "((("));
        let applied = apply_fixes("x", vec![&fix]);
        assert_eq!(applied.source, "(((");
        assert!(
            !syntax::build_ast(&applied.source, 0).errors.is_empty(),
            "the CLI write-gate relies on build_ast rejecting malformed fix output"
        );
    }
}
