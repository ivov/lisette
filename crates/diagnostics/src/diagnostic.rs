use miette::{Diagnostic, LabeledSpan, Severity};
use owo_colors::OwoColorize;
use std::fmt;
use std::sync::Arc;

use syntax::ParseError;
use syntax::ast::Span;

/// Source text with a precomputed line-offset index for O(log n) span lookups.
#[derive(Clone, Debug)]
pub struct IndexedSource {
    source: Arc<str>,
    line_starts: Arc<[usize]>,
}

impl IndexedSource {
    pub fn new(s: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, byte) in s.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            source: Arc::from(s),
            line_starts: Arc::from(line_starts),
        }
    }

    pub(crate) fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(idx) => idx.saturating_sub(1),
        };
        (line + 1, offset - self.line_starts[line] + 1)
    }
}

impl miette::SourceCode for IndexedSource {
    fn read_span<'a>(
        &'a self,
        span: &miette::SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        let src = self.source.as_ref();
        let offset = span.offset();
        let len = span.len();

        if offset + len > src.len() {
            return Err(miette::MietteError::OutOfBounds);
        }

        let span_line = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(idx) => idx.saturating_sub(1),
        };

        let start_line = span_line.saturating_sub(context_lines_before);
        let start_offset = self.line_starts[start_line];
        let start_column = if context_lines_before == 0 {
            offset - self.line_starts[span_line]
        } else {
            0
        };

        let span_end = offset + len.saturating_sub(1);
        let end_line = match self.line_starts.binary_search(&span_end) {
            Ok(exact) => exact,
            Err(idx) => idx.saturating_sub(1),
        };

        let last_line = (end_line + context_lines_after).min(self.line_starts.len() - 1);
        let end_offset = if last_line + 1 < self.line_starts.len() {
            self.line_starts[last_line + 1].min(src.len())
        } else {
            src.len()
        };

        Ok(Box::new(miette::MietteSpanContents::new(
            &src.as_bytes()[start_offset..end_offset],
            (start_offset, end_offset - start_offset).into(),
            start_line,
            start_column,
            last_line + 1,
        )))
    }
}

fn strip_period(s: &str, strip: bool) -> &str {
    if strip {
        s.strip_suffix('.').unwrap_or(s)
    } else {
        s
    }
}

#[derive(Debug, Clone)]
struct Label {
    span: Span,
    text: String,
    primary: bool,
}

impl Label {
    fn to_miette(&self, text: String) -> LabeledSpan {
        let source_span = miette::SourceSpan::new(
            (self.span.byte_offset as usize).into(),
            self.span.byte_length as usize,
        );
        if self.primary {
            LabeledSpan::new_primary_with_span(Some(text), source_span)
        } else {
            LabeledSpan::new_with_span(Some(text), source_span)
        }
    }
}

pub use miette::Report;

impl From<ParseError> for LisetteDiagnostic {
    fn from(err: ParseError) -> Self {
        let mut diagnostic = LisetteDiagnostic::error(&err.message);

        for (span, label) in &err.labels {
            diagnostic = diagnostic.with_span_label(span, label);
        }

        if let Some(help) = err.help {
            diagnostic = diagnostic.with_help(help);
        }

        if let Some(note) = err.note {
            diagnostic = diagnostic.with_note(note);
        }

        if !err.code.is_empty() {
            diagnostic = diagnostic.with_code(err.code);
        }

        diagnostic
    }
}

fn format_with_backticks<F>(text: &str, use_color: bool, base_style: F) -> String
where
    F: Fn(&str) -> String,
{
    if !use_color {
        return text.to_string();
    }

    let mut result = String::new();
    let mut chars = text.char_indices().peekable();
    let mut segment_start = 0;

    while let Some((i, ch)) = chars.next() {
        if ch == '`' {
            if i > segment_start {
                result.push_str(&base_style(&text[segment_start..i]));
            }

            let mut found_closing = false;
            for (j, inner_ch) in chars.by_ref() {
                if inner_ch == '`' {
                    let quoted = &text[i + 1..j];
                    result.push_str(&format!("{}", quoted.bright_magenta()));
                    segment_start = j + 1;
                    found_closing = true;
                    break;
                }
            }

            if !found_closing {
                result.push_str(&base_style(&text[i..]));
                segment_start = text.len();
            }
        }
    }

    if segment_start < text.len() {
        result.push_str(&base_style(&text[segment_start..]));
    }

    result
}

#[derive(Debug, Clone)]
#[must_use]
pub struct LisetteDiagnostic {
    message: String,
    labels: Vec<Label>,
    help: Option<String>,
    note: Option<String>,
    severity: Severity,
    code: Option<String>,
    fix: Option<crate::Fix>,
}

impl fmt::Display for LisetteDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_message(f, false)
    }
}

impl LisetteDiagnostic {
    fn fmt_message(&self, f: &mut fmt::Formatter<'_>, use_color: bool) -> fmt::Result {
        if use_color {
            let styled_message = match self.severity {
                Severity::Error => {
                    format_with_backticks(&self.message, true, |s| format!("{}", s.red().bold()))
                }
                Severity::Warning => {
                    format_with_backticks(&self.message, true, |s| format!("{}", s.yellow().bold()))
                }
                Severity::Advice => {
                    format_with_backticks(&self.message, true, |s| format!("{}", s.blue().bold()))
                }
            };
            write!(f, "{}", styled_message)?;
        } else {
            f.write_str(&self.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for LisetteDiagnostic {}

struct HelpText<'a> {
    help: Option<&'a str>,
    note: Option<&'a str>,
    diagnostic_code: Option<&'a str>,
    use_color: bool,
}

impl fmt::Display for HelpText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let use_color = self.use_color;
        let has_code = self.diagnostic_code.is_some();

        let combined = match (self.help, self.note) {
            (Some(h), Some(n)) => format!("{} {}", h, strip_period(n, has_code)),
            (Some(h), None) => strip_period(h, has_code).to_string(),
            (None, Some(n)) => strip_period(n, has_code).to_string(),
            (None, None) => String::new(),
        };

        if !combined.is_empty() {
            if use_color {
                let styled = format_with_backticks(&combined, true, |s| format!("{}", s.dimmed()));
                write!(f, "{}", styled)?;
            } else {
                write!(f, "{}", combined)?;
            }
        }

        if let Some(code) = self.diagnostic_code {
            let is_listing = self
                .help
                .is_some_and(|h| h.lines().skip(1).any(|line| line.starts_with("  ")));
            let prefix = if is_listing { "\ncode: " } else { " · code: " };
            if use_color {
                write!(f, "{}{}", prefix.dimmed(), format!("[{}]", code).dimmed())?;
            } else {
                write!(f, "{}[{}]", prefix, code)?;
            }
        }

        Ok(())
    }
}

impl Diagnostic for LisetteDiagnostic {
    fn severity(&self) -> Option<Severity> {
        Some(self.severity)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        let diagnostic_code = self.code.as_deref();

        if self.help.is_none() && self.note.is_none() && diagnostic_code.is_none() {
            return None;
        }
        Some(Box::new(HelpText {
            help: self.help.as_deref(),
            note: self.note.as_deref(),
            diagnostic_code,
            use_color: false,
        }))
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(self.formatted_labels(false).into_iter()))
    }

    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        None // rendered with the help text instead
    }
}

impl LisetteDiagnostic {
    pub fn plain_message(&self) -> &str {
        &self.message
    }

    pub fn plain_label(&self) -> Option<&str> {
        self.labels
            .iter()
            .map(|label| label.text.as_str())
            .find(|text| !text.is_empty())
    }

    pub fn plain_help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    pub fn plain_note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    fn new(message: impl Into<String>, severity: Severity) -> Self {
        Self {
            message: message.into(),
            labels: Vec::new(),
            help: None,
            note: None,
            severity,
            code: None,
            fix: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, Severity::Error)
    }

    pub fn warn(message: impl Into<String>) -> Self {
        Self::new(message, Severity::Warning)
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message, Severity::Advice)
    }

    pub fn with_span_label(mut self, span: &Span, text: impl Into<String>) -> Self {
        self.push_label(span, text.into(), false);
        self
    }

    pub fn with_span_primary_label(mut self, span: &Span, text: impl Into<String>) -> Self {
        self.push_label(span, text.into(), true);
        self
    }

    fn push_label(&mut self, span: &Span, text: String, primary: bool) {
        self.labels.push(Label {
            span: *span,
            text,
            primary,
        });
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_fix(mut self, fix: crate::Fix) -> Self {
        assert_eq!(
            self.file_id(),
            Some(fix.file_id()),
            "a fix must edit the file labeled by its diagnostic",
        );
        self.fix = Some(fix);
        self
    }

    pub fn fix(&self) -> Option<&crate::Fix> {
        self.fix.as_ref()
    }

    pub(crate) fn with_parse_code(mut self, code: &str) -> Self {
        self.code = Some(format!("parse.{}", code));
        self
    }

    pub fn with_resolve_code(mut self, code: &str) -> Self {
        self.code = Some(format!("resolve.{}", code));
        self
    }

    pub fn with_infer_code(mut self, code: &str) -> Self {
        self.code = Some(format!("infer.{}", code));
        self
    }

    pub(crate) fn with_lint_code(mut self, code: &str) -> Self {
        debug_assert!(
            matches!(self.severity, Severity::Warning | Severity::Advice),
            "with_lint_code requires Warning or Advice severity (got {:?}); \
             use a phase-specific code constructor for errors",
            self.severity,
        );
        self.code = Some(format!("lint.{}", code));
        self
    }

    pub(crate) fn with_attribute_code(mut self, code: &str) -> Self {
        self.code = Some(format!("attribute.{}", code));
        self
    }

    pub(crate) fn with_emit_code(mut self, code: &str) -> Self {
        self.code = Some(format!("emit.{}", code));
        self
    }

    fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_source_code(self, source: IndexedSource, filename: String) -> miette::Report {
        miette::Report::new(self).with_source_code(miette::NamedSource::new(filename, source))
    }

    pub(crate) fn into_rendered(self, use_color: bool) -> RenderedDiagnostic {
        RenderedDiagnostic {
            diagnostic: self,
            use_color,
        }
    }

    fn formatted_labels(&self, use_color: bool) -> Vec<LabeledSpan> {
        let file_id = self.file_id();
        self.labels
            .iter()
            .filter(|label| Some(label.span.file_id) == file_id)
            .map(|label| {
                let formatted = if use_color {
                    let style = |s: &str| match self.severity {
                        Severity::Error => format!("{}", s.red()),
                        Severity::Warning => format!("{}", s.yellow()),
                        Severity::Advice => format!("{}", s.blue()),
                    };
                    format_with_backticks(&label.text, true, style)
                } else {
                    label.text.clone()
                };
                label.to_miette(formatted)
            })
            .collect()
    }

    pub fn code_str(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn lint_name(&self) -> Option<&str> {
        self.code.as_deref()?.strip_prefix("lint.")
    }

    pub fn primary_offset(&self) -> usize {
        self.labels
            .first()
            .map(|label| label.span.byte_offset as usize)
            .unwrap_or(0)
    }

    pub(crate) fn label_points(&self) -> Vec<(u32, usize)> {
        self.labels
            .iter()
            .map(|label| (label.span.file_id, label.span.byte_offset as usize))
            .collect()
    }

    pub fn location_offset(&self) -> Option<usize> {
        let file_id = self.file_id()?;
        self.labels
            .iter()
            .filter(|label| label.span.file_id == file_id)
            .find(|label| label.primary)
            .or_else(|| self.labels.first())
            .map(|label| label.span.byte_offset as usize)
    }

    pub(crate) fn severity_word(&self) -> &'static str {
        match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Advice => "info",
        }
    }

    pub fn file_id(&self) -> Option<u32> {
        self.labels.first().map(|label| label.span.file_id)
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }

    pub fn is_info(&self) -> bool {
        self.severity == Severity::Advice
    }

    pub fn sort_key(a: &Self, b: &Self) -> std::cmp::Ordering {
        a.file_id()
            .cmp(&b.file_id())
            .then_with(|| a.primary_offset().cmp(&b.primary_offset()))
            .then_with(|| a.code_str().cmp(&b.code_str()))
            .then_with(|| a.plain_message().cmp(b.plain_message()))
    }
}

#[derive(Debug)]
pub(crate) struct RenderedDiagnostic {
    diagnostic: LisetteDiagnostic,
    use_color: bool,
}

impl fmt::Display for RenderedDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic.fmt_message(f, self.use_color)
    }
}

impl std::error::Error for RenderedDiagnostic {}

impl Diagnostic for RenderedDiagnostic {
    fn severity(&self) -> Option<Severity> {
        Some(self.diagnostic.severity)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        let diagnostic_code = self.diagnostic.code.as_deref();
        if self.diagnostic.help.is_none()
            && self.diagnostic.note.is_none()
            && diagnostic_code.is_none()
        {
            return None;
        }
        Some(Box::new(HelpText {
            help: self.diagnostic.help.as_deref(),
            note: self.diagnostic.note.as_deref(),
            diagnostic_code,
            use_color: self.use_color,
        }))
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(
            self.diagnostic.formatted_labels(self.use_color).into_iter(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_constructor_is_advice_severity() {
        let diagnostic = LisetteDiagnostic::info("advisory");
        assert!(diagnostic.is_info());
        assert!(!diagnostic.is_error());
        assert!(!diagnostic.is_warning());
        assert_eq!(diagnostic.severity_word(), "info");
    }

    #[test]
    #[should_panic(expected = "a fix must edit the file labeled by its diagnostic")]
    fn rejects_fix_for_a_different_file() {
        let fix = crate::Fix::new("invalid", crate::Edit::deletion(Span::new(1, 0, 1)));
        let _ = LisetteDiagnostic::error("invalid")
            .with_span_label(&Span::new(0, 0, 1), "label")
            .with_fix(fix);
    }

    #[test]
    fn labels_keep_their_own_file_identity() {
        let diagnostic = LisetteDiagnostic::error("cross-file")
            .with_span_primary_label(&Span::new(3, 4, 1), "first")
            .with_span_label(&Span::new(7, 8, 1), "second");

        assert_eq!(diagnostic.file_id(), Some(3));
        assert_eq!(diagnostic.label_points(), vec![(3, 4), (7, 8)]);
        assert_eq!(diagnostic.formatted_labels(false).len(), 1);
    }
}
