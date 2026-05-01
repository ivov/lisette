use super::Formatter;
use crate::comments::prepend_comments;
use crate::lindig::{Document, strict_break};

pub(super) struct SiblingEntry<'a> {
    pub(super) leading: Option<Document<'a>>,
    pub(super) doc: Document<'a>,
    pub(super) trailing: Option<Document<'a>>,
    pub(super) has_blank_above: bool,
}

pub(super) struct PatternEntry<'a> {
    pub(super) leading: Option<Document<'a>>,
    pub(super) doc: Document<'a>,
    pub(super) trailing: Option<Document<'a>>,
}

impl<'a> Formatter<'a> {
    /// Splits comments before `next_start` into `(prev_same_line, this_leading, has_blank)`.
    pub(super) fn sibling_lead_split(
        &mut self,
        has_prev: bool,
        next_start: u32,
    ) -> (Option<Document<'a>>, Option<Document<'a>>, bool) {
        if has_prev {
            self.comments.take_split_at_line_start(next_start)
        } else {
            (None, self.comments.take_comments_before(next_start), false)
        }
    }

    /// Joins entries into a comma-separated body; returns `(body, close_sep)`.
    pub(super) fn join_pattern_entries(
        entries: Vec<PatternEntry<'a>>,
        rest: Option<(Option<Document<'a>>, Document<'a>)>,
        trailing_unbroken: &'static str,
    ) -> (Document<'a>, Document<'a>) {
        let mut body = Document::Sequence(vec![]);
        let mut prev_had_trailing = false;
        let entry_count = entries.len();
        let separator = |prev_had_trailing: bool| {
            if prev_had_trailing {
                Document::Newline
            } else {
                strict_break(",", ", ")
            }
        };
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                body = body.append(separator(prev_had_trailing));
            }
            let mut elem = entry.doc;
            if let Some(c) = entry.leading {
                elem = c.append(Document::Newline).force_break().append(elem);
            }
            body = body.append(elem);
            if let Some(t) = entry.trailing {
                body = body
                    .append(Document::str(","))
                    .append(Document::str(" "))
                    .append(t.force_break());
                prev_had_trailing = true;
            } else {
                prev_had_trailing = false;
            }
        }
        if let Some((rest_leading, rest_doc)) = rest {
            if entry_count > 0 {
                body = body.append(separator(prev_had_trailing));
            }
            let mut rest_block = rest_doc;
            if let Some(c) = rest_leading {
                rest_block = c.append(Document::Newline).force_break().append(rest_block);
            }
            body = body.append(rest_block);
            prev_had_trailing = false;
        }
        let close_sep = if prev_had_trailing {
            strict_break("", trailing_unbroken)
        } else {
            strict_break(",", trailing_unbroken)
        };
        (body, close_sep)
    }

    /// Split-then-build: `build` runs after the split so its auto-drain sees the post-leading cursor.
    pub(super) fn push_pattern_entry(
        &mut self,
        entries: &mut Vec<PatternEntry<'a>>,
        start: u32,
        build: impl FnOnce(&mut Self) -> Document<'a>,
    ) {
        let (last_trailing, leading, _) = self.sibling_lead_split(!entries.is_empty(), start);
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        let doc = build(self);
        entries.push(PatternEntry {
            leading,
            doc,
            trailing: None,
        });
    }

    pub(super) fn push_sibling_entry(
        &mut self,
        entries: &mut Vec<SiblingEntry<'a>>,
        start: u32,
        build: impl FnOnce(&mut Self) -> Document<'a>,
    ) {
        let (last_trailing, leading, has_blank) =
            self.sibling_lead_split(!entries.is_empty(), start);
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        let doc = build(self);
        entries.push(SiblingEntry {
            leading,
            doc,
            trailing: None,
            has_blank_above: has_blank,
        });
    }

    /// Sibling split before a rest token; returns the rest's leading.
    pub(super) fn split_for_rest(
        &mut self,
        entries: &mut Vec<PatternEntry<'a>>,
        rest_pos: u32,
    ) -> Option<Document<'a>> {
        let (last_trailing, rest_leading, _) =
            self.sibling_lead_split(!entries.is_empty(), rest_pos);
        if let Some(t) = last_trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        rest_leading
    }

    /// Joins sibling entries and drains body-trailing comments before `body_end`.
    pub(super) fn join_sibling_body(
        &mut self,
        mut entries: Vec<SiblingEntry<'a>>,
        body_end: u32,
    ) -> Document<'a> {
        let standalone = if entries.is_empty() {
            self.comments.take_comments_before(body_end)
        } else {
            let (same_line, standalone, _) = self.comments.take_split_at_line_start(body_end);
            if let Some(t) = same_line
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            standalone
        };

        let mut body = Document::Sequence(vec![]);
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                body = body.append(Document::Newline);
                if entry.has_blank_above {
                    body = body.append(Document::Newline);
                }
            }
            if let Some(c) = entry.leading {
                body = body.append(c.force_break()).append(Document::Newline);
            }
            body = body.append(entry.doc);
            if let Some(t) = entry.trailing {
                body = body.append(Document::str(" ")).append(t);
            }
        }
        if let Some(s) = standalone {
            body = body
                .append(Document::Newline)
                .append(Document::Newline)
                .append(s.force_break());
        }
        body
    }

    /// Drains comments before `start` and prepends them to `build`'s output.
    pub(super) fn with_leading_comments(
        &mut self,
        start: u32,
        build: impl FnOnce(&mut Self) -> Document<'a>,
    ) -> Document<'a> {
        let comments = self.comments.take_comments_before(start);
        let doc = build(self);
        prepend_comments(doc, comments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::Comments;
    use syntax::lex::Trivia;

    fn entry<'a>(
        leading: Option<&'a str>,
        doc: &'a str,
        trailing: Option<&'a str>,
    ) -> PatternEntry<'a> {
        PatternEntry {
            leading: leading.map(Document::str),
            doc: Document::str(doc),
            trailing: trailing.map(Document::str),
        }
    }

    fn render_inline<'a>(body: Document<'a>, close_sep: Document<'a>) -> String {
        Document::str("(")
            .append(strict_break("", ""))
            .append(body)
            .nest(2)
            .append(close_sep)
            .append(")")
            .group()
            .to_pretty_string(80)
    }

    fn render_inline_broken<'a>(body: Document<'a>, close_sep: Document<'a>) -> String {
        Document::str("(")
            .append(strict_break("", ""))
            .append(body)
            .nest(2)
            .append(close_sep)
            .append(")")
            .group()
            .force_break()
            .to_pretty_string(80)
    }

    fn trivia(comments: Vec<(u32, u32)>, blank_lines: Vec<u32>) -> Trivia {
        Trivia {
            comments,
            doc_comments: Vec::new(),
            blank_lines,
        }
    }

    fn render_opt(doc: Option<Document<'_>>) -> Option<String> {
        doc.map(|d| d.to_pretty_string(80))
    }

    fn render_doc(doc: Document<'_>) -> String {
        doc.to_pretty_string(80)
    }

    #[test]
    fn join_pattern_entries_single_entry_unbroken() {
        let entries = vec![entry(None, "a", None)];
        let (body, close_sep) = Formatter::join_pattern_entries(entries, None, "");
        assert_eq!(render_inline(body, close_sep), "(a)");
    }

    #[test]
    fn join_pattern_entries_two_entries_unbroken() {
        let entries = vec![entry(None, "a", None), entry(None, "b", None)];
        let (body, close_sep) = Formatter::join_pattern_entries(entries, None, "");
        assert_eq!(render_inline(body, close_sep), "(a, b)");
    }

    #[test]
    fn join_pattern_entries_trailing_forces_no_double_comma() {
        let entries = vec![entry(None, "a", Some("// c1")), entry(None, "b", None)];
        let (body, close_sep) = Formatter::join_pattern_entries(entries, None, "");
        let out = render_inline(body, close_sep);
        assert!(out.contains("a, // c1"), "got: {out}");
        assert!(!out.contains(",,"), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_last_trailing_close_sep_omits_comma() {
        let entries = vec![entry(None, "a", Some("// c"))];
        let (body, close_sep) = Formatter::join_pattern_entries(entries, None, "");
        let out = render_inline(body, close_sep);
        assert!(out.contains("a, // c"), "got: {out}");
        assert!(!out.contains(",)"), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_leading_forces_break() {
        let entries = vec![
            entry(Some("// before a"), "a", None),
            entry(None, "b", None),
        ];
        let (body, close_sep) = Formatter::join_pattern_entries(entries, None, "");
        let out = render_inline(body, close_sep);
        assert!(out.contains("// before a\n  a"), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_rest_only() {
        let (body, close_sep) =
            Formatter::join_pattern_entries(Vec::new(), Some((None, Document::str("..rest"))), "");
        assert_eq!(render_inline(body, close_sep), "(..rest)");
    }

    #[test]
    fn join_pattern_entries_entries_then_rest_unbroken() {
        let entries = vec![entry(None, "a", None), entry(None, "b", None)];
        let (body, close_sep) =
            Formatter::join_pattern_entries(entries, Some((None, Document::str("..rest"))), "");
        assert_eq!(render_inline(body, close_sep), "(a, b, ..rest)");
    }

    #[test]
    fn join_pattern_entries_rest_with_leading_renders_above_dots() {
        let entries = vec![entry(None, "a", None)];
        let (body, close_sep) = Formatter::join_pattern_entries(
            entries,
            Some((Some(Document::str("// before rest")), Document::str(".."))),
            "",
        );
        let out = render_inline_broken(body, close_sep);
        assert!(out.contains("// before rest\n  .."), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_trailing_unbroken_for_struct_brace() {
        let entries = vec![entry(None, "a", None)];
        let (body, close_sep) = Formatter::join_pattern_entries(entries, None, " ");
        let out = Document::str("{")
            .append(strict_break(" ", " "))
            .append(body)
            .nest(2)
            .append(close_sep)
            .append("}")
            .group()
            .to_pretty_string(80);
        assert_eq!(out, "{ a }");
    }

    #[test]
    fn sibling_lead_split_no_prev_returns_all_as_leading() {
        let source = "// c\nfn f() {}";
        let t = trivia(vec![(0, 4)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let (same, leading, has_blank) = f.sibling_lead_split(false, source.len() as u32);
        assert_eq!(render_opt(same), None);
        assert_eq!(render_opt(leading).as_deref(), Some("// c"));
        assert!(!has_blank);
    }

    #[test]
    fn sibling_lead_split_with_prev_routes_at_line_start() {
        let source = "x // a\n  // b\n";
        let t = trivia(vec![(2, 6), (9, 13)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let (same, leading, _) = f.sibling_lead_split(true, source.len() as u32);
        assert_eq!(render_opt(same).as_deref(), Some("// a"));
        assert_eq!(render_opt(leading).as_deref(), Some("// b"));
    }

    #[test]
    fn push_pattern_entry_attaches_trailing_to_previous() {
        let source = "x // tail\n  y";
        let t = trivia(vec![(2, 9)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let mut entries: Vec<PatternEntry<'_>> = Vec::new();
        f.push_pattern_entry(&mut entries, 0, |_| Document::str("a"));
        f.push_pattern_entry(&mut entries, source.len() as u32, |_| Document::str("b"));
        assert_eq!(entries.len(), 2);
        assert_eq!(
            render_opt(entries[0].trailing.clone()).as_deref(),
            Some("// tail")
        );
        assert!(entries[1].leading.is_none());
    }

    #[test]
    fn push_pattern_entry_split_runs_before_build() {
        let source = "// pre\nx";
        let t = trivia(vec![(0, 6)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let mut entries: Vec<PatternEntry<'_>> = Vec::new();
        let mut build_called = false;
        f.push_pattern_entry(&mut entries, source.len() as u32, |_| {
            build_called = true;
            Document::str("x")
        });
        assert!(build_called);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            render_opt(entries[0].leading.clone()).as_deref(),
            Some("// pre")
        );
    }

    #[test]
    fn split_for_rest_attaches_trailing_and_returns_leading() {
        let source = "x // tail\n// pre\n..rest";
        let t = trivia(vec![(2, 9), (10, 16)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let mut entries: Vec<PatternEntry<'_>> = vec![entry(None, "a", None)];
        let leading = f.split_for_rest(&mut entries, 17);
        assert_eq!(
            render_opt(entries[0].trailing.clone()).as_deref(),
            Some("// tail")
        );
        assert_eq!(render_opt(leading).as_deref(), Some("// pre"));
    }

    #[test]
    fn join_sibling_body_attaches_same_line_to_last_entry() {
        let source = "x // tail\n}";
        let t = trivia(vec![(2, 9)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let entries = vec![SiblingEntry {
            leading: None,
            doc: Document::str("a"),
            trailing: None,
            has_blank_above: false,
        }];
        let body = f.join_sibling_body(entries, 10);
        let out = render_doc(body);
        assert!(out.contains("a // tail"), "got: {out}");
    }

    #[test]
    fn join_sibling_body_standalone_renders_as_separated_block() {
        let source = "x\n  // tail\n}";
        let t = trivia(vec![(4, 11)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let entries = vec![SiblingEntry {
            leading: None,
            doc: Document::str("a"),
            trailing: None,
            has_blank_above: false,
        }];
        let body = f.join_sibling_body(entries, 12);
        let out = render_doc(body);
        assert!(out.contains("a\n\n// tail"), "got: {out}");
    }

    #[test]
    fn join_sibling_body_empty_entries_drains_as_standalone() {
        let source = "// only\n";
        let t = trivia(vec![(0, 7)], Vec::new());
        let comments = Comments::from_trivia(&t, source);
        let mut f = Formatter::new(comments);
        let body = f.join_sibling_body(Vec::new(), source.len() as u32);
        let out = render_doc(body);
        assert!(out.contains("// only"), "got: {out}");
    }
}
