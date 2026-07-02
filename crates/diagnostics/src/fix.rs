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

/// An applicable fix for one diagnostic, carrying one or more edits that are
/// applied together as a unit.
#[derive(Debug, Clone)]
pub struct Fix {
    message: String,
    edits: Vec<Edit>,
}

impl Fix {
    pub fn new(message: impl Into<String>, edit: Edit) -> Self {
        Self {
            message: message.into(),
            edits: vec![edit],
        }
    }

    pub fn multi(message: impl Into<String>, edits: Vec<Edit>) -> Self {
        Self {
            message: message.into(),
            edits,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }
}

#[derive(Debug)]
pub struct FixApplicationOutcome {
    pub source: String,
    pub applied: usize,
}

/// Apply `fixes` to `source`, each fix atomically. A fix applies only when all of
/// its edits are mutually non-overlapping and clear of every already-applied edit
/// (boundary adjacency counts as overlap), else the whole fix is skipped and stays
/// reported on the next run.
pub fn apply_fixes(source: &str, mut fixes: Vec<&Fix>) -> FixApplicationOutcome {
    fixes.sort_by_key(|fix| fix.edits().iter().map(|edit| edit.span().byte_offset).min());

    let mut accepted: Vec<&Edit> = Vec::new();
    let mut applied = 0;

    for fix in fixes {
        let edits = fix.edits();
        // A fix's own edits are authored together, so adjacent ones concatenate.
        // Only edits that share a byte genuinely conflict.
        let internally_clear = edits.iter().enumerate().all(|(i, a)| {
            edits[i + 1..]
                .iter()
                .all(|b| !spans_share_byte(a.span(), b.span()))
        });
        let clear_of_accepted = edits.iter().all(|edit| {
            accepted
                .iter()
                .all(|other| !spans_overlap(edit.span(), other.span()))
        });
        if internally_clear && clear_of_accepted {
            accepted.extend(edits);
            applied += 1;
        }
    }

    accepted.sort_by_key(|edit| edit.span().byte_offset);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for edit in accepted {
        let start = edit.span().byte_offset as usize;
        out.push_str(&source[cursor..start]);
        out.push_str(edit.content());
        cursor = edit.span().end() as usize;
    }
    out.push_str(&source[cursor..]);
    FixApplicationOutcome {
        source: out,
        applied,
    }
}

fn spans_overlap(a: Span, b: Span) -> bool {
    a.byte_offset <= b.end() && b.byte_offset <= a.end()
}
fn spans_share_byte(a: Span, b: Span) -> bool {
    a.byte_offset < b.end() && b.byte_offset < a.end()
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
    fn applies_all_edits_of_a_multi_edit_fix() {
        let fix = Fix::multi(
            "m",
            vec![
                Edit::replacement(span(0, 1), "A"),
                Edit::replacement(span(4, 1), "E"),
            ],
        );
        let applied = apply_fixes("abcde", vec![&fix]);
        assert_eq!(applied.source, "AbcdE");
        assert_eq!(applied.applied, 1);
    }

    #[test]
    fn applies_adjacent_edits_within_one_fix() {
        let fix = Fix::multi(
            "m",
            vec![
                Edit::replacement(span(0, 1), "X"),
                Edit::deletion(span(1, 2)),
            ],
        );
        let applied = apply_fixes("abcd", vec![&fix]);
        assert_eq!(applied.source, "Xd");
        assert_eq!(applied.applied, 1);
    }

    #[test]
    fn skips_whole_multi_edit_fix_when_one_edit_conflicts() {
        let blocker = Fix::new("b", Edit::replacement(span(0, 1), "Z"));
        let multi = Fix::multi(
            "m",
            vec![
                Edit::replacement(span(0, 1), "A"),
                Edit::replacement(span(4, 1), "E"),
            ],
        );
        let applied = apply_fixes("abcde", vec![&blocker, &multi]);
        assert_eq!(applied.source, "Zbcde");
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
