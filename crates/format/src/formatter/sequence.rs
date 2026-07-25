use super::Formatter;
use crate::comments::{SplitComments, prepend_comments};
use crate::lindig::{Document, strict_break};

pub(super) struct SiblingEntry<'a> {
    pub(super) leading: Option<Document<'a>>,
    pub(super) doc: Document<'a>,
    pub(super) trailing: Option<Document<'a>>,
    pub(super) has_blank_above: bool,
}

pub(super) struct PatternEntry<'a> {
    leading: Option<Document<'a>>,
    doc: Document<'a>,
    trailing: Option<Document<'a>>,
}

pub(super) struct JoinedPattern<'a> {
    pub(super) body: Document<'a>,
    pub(super) close_separator: Document<'a>,
}

impl<'a> Formatter<'a> {
    fn sibling_lead_split(&mut self, has_prev: bool, next_start: u32) -> SplitComments<'a> {
        if has_prev {
            self.comments.take_split_at_line_start(next_start)
        } else {
            SplitComments::leading(self.comments.take_comments_before(next_start))
        }
    }

    pub(super) fn join_pattern_entries(
        entries: Vec<PatternEntry<'a>>,
        trailing_unbroken: &'static str,
    ) -> JoinedPattern<'a> {
        let mut body = Document::Sequence(vec![]);
        let mut prev_had_trailing = false;
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
        let close_sep = if prev_had_trailing {
            strict_break("", trailing_unbroken)
        } else {
            strict_break(",", trailing_unbroken)
        };
        JoinedPattern {
            body,
            close_separator: close_sep,
        }
    }

    /// Split-then-build: `build` runs after the split so its auto-drain sees the post-leading cursor.
    pub(super) fn push_pattern_entry(
        &mut self,
        entries: &mut Vec<PatternEntry<'a>>,
        start: u32,
        build: impl FnOnce(&mut Self) -> Document<'a>,
    ) {
        let split = self.sibling_lead_split(!entries.is_empty(), start);
        if let Some(t) = split.trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        let doc = build(self);
        entries.push(PatternEntry {
            leading: split.leading,
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
        let split = self.sibling_lead_split(!entries.is_empty(), start);
        if let Some(t) = split.trailing
            && let Some(last) = entries.last_mut()
        {
            last.trailing = Some(t);
        }
        let doc = build(self);
        entries.push(SiblingEntry {
            leading: split.leading,
            doc,
            trailing: None,
            has_blank_above: split.has_blank_before_leading,
        });
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
            let split = self.comments.take_split_at_line_start(body_end);
            if let Some(t) = split.trailing
                && let Some(last) = entries.last_mut()
            {
                last.trailing = Some(t);
            }
            split.leading
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

    fn render_inline(joined: JoinedPattern<'_>) -> String {
        Document::str("(")
            .append(strict_break("", ""))
            .append(joined.body)
            .nest(2)
            .append(joined.close_separator)
            .append(")")
            .group()
            .to_pretty_string(80)
    }

    fn render_inline_broken(joined: JoinedPattern<'_>) -> String {
        Document::str("(")
            .append(strict_break("", ""))
            .append(joined.body)
            .nest(2)
            .append(joined.close_separator)
            .append(")")
            .group()
            .force_break()
            .to_pretty_string(80)
    }

    fn comments<'a>(
        source: &'a str,
        ranges: Vec<(u32, u32)>,
        blank_lines: Vec<u32>,
    ) -> Comments<'a> {
        let mut lexed = syntax::lex::Lexer::new(source, 0).lex();
        lexed.blank_lines = blank_lines;
        let selected = lexed
            .tokens
            .into_iter()
            .filter(|token| {
                ranges
                    .iter()
                    .any(|&(start, end)| token.byte_offset == start && token.end_offset() == end)
            })
            .collect::<Vec<_>>();
        Comments::from_lexed(&selected, lexed.blank_lines, source)
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
        let joined = Formatter::join_pattern_entries(entries, "");
        assert_eq!(render_inline(joined), "(a)");
    }

    #[test]
    fn join_pattern_entries_two_entries_unbroken() {
        let entries = vec![entry(None, "a", None), entry(None, "b", None)];
        let joined = Formatter::join_pattern_entries(entries, "");
        assert_eq!(render_inline(joined), "(a, b)");
    }

    #[test]
    fn join_pattern_entries_trailing_forces_no_double_comma() {
        let entries = vec![entry(None, "a", Some("// c1")), entry(None, "b", None)];
        let joined = Formatter::join_pattern_entries(entries, "");
        let out = render_inline(joined);
        assert!(out.contains("a, // c1"), "got: {out}");
        assert!(!out.contains(",,"), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_last_trailing_close_sep_omits_comma() {
        let entries = vec![entry(None, "a", Some("// c"))];
        let joined = Formatter::join_pattern_entries(entries, "");
        let out = render_inline(joined);
        assert!(out.contains("a, // c"), "got: {out}");
        assert!(!out.contains(",)"), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_leading_forces_break() {
        let entries = vec![
            entry(Some("// before a"), "a", None),
            entry(None, "b", None),
        ];
        let joined = Formatter::join_pattern_entries(entries, "");
        let out = render_inline(joined);
        assert!(out.contains("// before a\n  a"), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_rest_only() {
        let joined = Formatter::join_pattern_entries(vec![entry(None, "..rest", None)], "");
        assert_eq!(render_inline(joined), "(..rest)");
    }

    #[test]
    fn join_pattern_entries_entries_then_rest_unbroken() {
        let entries = vec![entry(None, "a", None), entry(None, "b", None)];
        let mut entries = entries;
        entries.push(entry(None, "..rest", None));
        let joined = Formatter::join_pattern_entries(entries, "");
        assert_eq!(render_inline(joined), "(a, b, ..rest)");
    }

    #[test]
    fn join_pattern_entries_rest_with_leading_renders_above_dots() {
        let entries = vec![entry(None, "a", None)];
        let mut entries = entries;
        entries.push(entry(Some("// before rest"), "..", None));
        let joined = Formatter::join_pattern_entries(entries, "");
        let out = render_inline_broken(joined);
        assert!(out.contains("// before rest\n  .."), "got: {out}");
    }

    #[test]
    fn join_pattern_entries_trailing_unbroken_for_struct_brace() {
        let entries = vec![entry(None, "a", None)];
        let joined = Formatter::join_pattern_entries(entries, " ");
        let out = Document::str("{")
            .append(strict_break("", " "))
            .append(joined.body)
            .nest(2)
            .append(joined.close_separator)
            .append("}")
            .group()
            .to_pretty_string(80);
        assert_eq!(out, "{ a }");
    }

    #[test]
    fn sibling_lead_split_no_prev_returns_all_as_leading() {
        let source = "// c\nfn f() {}";
        let comments = comments(source, vec![(0, 4)], Vec::new());
        let mut f = Formatter::new(comments);
        let split = f.sibling_lead_split(false, source.len() as u32);
        assert_eq!(render_opt(split.trailing), None);
        assert_eq!(render_opt(split.leading).as_deref(), Some("// c"));
        assert!(!split.has_blank_before_leading);
    }

    #[test]
    fn sibling_lead_split_with_prev_routes_at_line_start() {
        let source = "x // a\n  // b\n";
        let comments = comments(source, vec![(2, 6), (9, 13)], Vec::new());
        let mut f = Formatter::new(comments);
        let split = f.sibling_lead_split(true, source.len() as u32);
        assert_eq!(render_opt(split.trailing).as_deref(), Some("// a"));
        assert_eq!(render_opt(split.leading).as_deref(), Some("// b"));
    }

    #[test]
    fn push_pattern_entry_attaches_trailing_to_previous() {
        let source = "x // tail\n  y";
        let comments = comments(source, vec![(2, 9)], Vec::new());
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
        let comments = comments(source, vec![(0, 6)], Vec::new());
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
    fn push_pattern_entry_attaches_trailing_and_leading_comments() {
        let source = "x // tail\n// pre\n..rest";
        let comments = comments(source, vec![(2, 9), (10, 16)], Vec::new());
        let mut f = Formatter::new(comments);
        let mut entries: Vec<PatternEntry<'_>> = vec![entry(None, "a", None)];
        f.push_pattern_entry(&mut entries, 17, |_| Document::str("..rest"));
        assert_eq!(
            render_opt(entries[0].trailing.clone()).as_deref(),
            Some("// tail")
        );
        assert_eq!(
            render_opt(entries[1].leading.clone()).as_deref(),
            Some("// pre")
        );
    }

    #[test]
    fn join_sibling_body_attaches_same_line_to_last_entry() {
        let source = "x // tail\n}";
        let comments = comments(source, vec![(2, 9)], Vec::new());
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
        let comments = comments(source, vec![(4, 11)], Vec::new());
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
        let comments = comments(source, vec![(0, 7)], Vec::new());
        let mut f = Formatter::new(comments);
        let body = f.join_sibling_body(Vec::new(), source.len() as u32);
        let out = render_doc(body);
        assert!(out.contains("// only"), "got: {out}");
    }
}
