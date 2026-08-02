use owo_colors::OwoColorize;
use std::fmt;
use std::sync::Arc;

use crate::graphical::FrameLabel;
use syntax::ParseError;
use syntax::ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Error,
    Warning,
    Advice,
}

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
        let line = self.line_index_of_offset(offset);
        (line + 1, offset - self.line_starts[line] + 1)
    }

    pub(crate) fn text(&self) -> &str {
        &self.source
    }

    pub(crate) fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }

    pub(crate) fn line_index_of_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(index) => index.saturating_sub(1),
        }
    }
}

fn strip_period(s: &str, strip: bool) -> &str {
    if strip {
        s.strip_suffix('.').unwrap_or(s)
    } else {
        s
    }
}

fn combine_help_and_note(help: Option<&str>, note: Option<&str>, has_code: bool) -> Option<String> {
    match (help, note) {
        (Some(help), Some(note)) => Some(format!("{} {}", help, strip_period(note, has_code))),
        (Some(help), None) => Some(strip_period(help, has_code).to_string()),
        (None, Some(note)) => Some(strip_period(note, has_code).to_string()),
        (None, None) => None,
    }
}

#[derive(Debug, Clone)]
struct Label {
    span: Span,
    text: String,
    primary: bool,
}

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
    file_location: Option<String>,
}

impl fmt::Display for LisetteDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
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

        let combined = combine_help_and_note(self.help, self.note, has_code).unwrap_or_default();

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

    pub fn help_and_note(&self) -> Option<String> {
        combine_help_and_note(
            self.help.as_deref(),
            self.note.as_deref(),
            self.code.is_some(),
        )
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
            file_location: None,
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

    pub(crate) fn styled_message(&self, use_color: bool) -> String {
        if !use_color {
            return self.message.clone();
        }
        match self.severity {
            Severity::Error => {
                format_with_backticks(&self.message, true, |s| format!("{}", s.red().bold()))
            }
            Severity::Warning => {
                format_with_backticks(&self.message, true, |s| format!("{}", s.yellow().bold()))
            }
            Severity::Advice => {
                format_with_backticks(&self.message, true, |s| format!("{}", s.blue().bold()))
            }
        }
    }

    pub(crate) fn styled_help_text(&self, use_color: bool) -> Option<String> {
        if self.help.is_none() && self.note.is_none() && self.code.is_none() {
            return None;
        }
        Some(
            HelpText {
                help: self.help.as_deref(),
                note: self.note.as_deref(),
                diagnostic_code: self.code.as_deref(),
                use_color,
            }
            .to_string(),
        )
    }

    pub(crate) fn frame_labels(&self, use_color: bool) -> Vec<FrameLabel> {
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
                FrameLabel::new(
                    label.span.byte_offset as usize,
                    label.span.byte_length as usize,
                    formatted,
                    label.primary,
                )
            })
            .collect()
    }

    /// Byte offset and length of the first label, which anchors the diagnostic.
    pub fn first_label_span(&self) -> Option<(usize, usize)> {
        self.labels.first().map(|label| {
            (
                label.span.byte_offset as usize,
                label.span.byte_length as usize,
            )
        })
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

    fn location_label(&self) -> Option<&Label> {
        let file_id = self.file_id()?;
        self.labels
            .iter()
            .filter(|label| label.span.file_id == file_id)
            .find(|label| label.primary)
            .or_else(|| self.labels.first())
    }

    pub fn location_offset(&self) -> Option<usize> {
        self.location_label()
            .map(|label| label.span.byte_offset as usize)
    }

    pub fn with_file_location(mut self, display_path: impl Into<String>) -> Self {
        self.file_location = Some(display_path.into());
        self
    }

    pub fn file_location(&self) -> Option<&str> {
        self.file_location.as_deref()
    }

    pub fn located_label(&self) -> Option<&str> {
        self.location_label()
            .map(|label| label.text.as_str())
            .filter(|text| !text.is_empty())
            .or_else(|| self.plain_label())
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
        assert_eq!(diagnostic.frame_labels(false).len(), 1);
    }
}
