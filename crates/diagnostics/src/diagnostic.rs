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

fn span_to_labeled(span: &Span, text: String, primary: bool) -> LabeledSpan {
    let source_span = miette::SourceSpan::new(
        (span.byte_offset as usize).into(),
        span.byte_length as usize,
    );
    if primary {
        LabeledSpan::new_primary_with_span(Some(text), source_span)
    } else {
        LabeledSpan::new_with_span(Some(text), source_span)
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
    labels: Vec<LabeledSpan>,
    help: Option<String>,
    note: Option<String>,
    severity: Severity,
    code: Option<String>,
    file_id: Option<u32>,
    use_color: bool,
}

impl fmt::Display for LisetteDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.use_color {
            let styled_message = match self.severity {
                Severity::Error => {
                    format_with_backticks(&self.message, true, |s| format!("{}", s.red().bold()))
                }
                Severity::Warning => {
                    format_with_backticks(&self.message, true, |s| format!("{}", s.yellow().bold()))
                }
                Severity::Advice => {
                    format_with_backticks(&self.message, true, |s| format!("{}", s.cyan().bold()))
                }
            };
            write!(f, "{}", styled_message)?;
        } else {
            self.message.fmt(f)?;
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
            use_color: self.use_color,
        }))
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let use_color = self.use_color;
        let severity = self.severity;

        let formatted_labels = self.labels.iter().map(move |span| {
            if let Some(label) = span.label() {
                let formatted = if use_color {
                    let base_style = match severity {
                        Severity::Error => |s: &str| format!("{}", s.red()),
                        Severity::Warning => |s: &str| format!("{}", s.yellow()),
                        Severity::Advice => |s: &str| format!("{}", s.cyan()),
                    };
                    format_with_backticks(label, true, base_style)
                } else {
                    label.to_string()
                };
                if span.primary() {
                    LabeledSpan::new_primary_with_span(Some(formatted), *span.inner())
                } else {
                    LabeledSpan::new_with_span(Some(formatted), *span.inner())
                }
            } else {
                span.clone()
            }
        });

        Some(Box::new(formatted_labels))
    }

    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        None // rendered with the help text instead
    }
}

impl LisetteDiagnostic {
    pub fn plain_message(&self) -> &str {
        &self.message
    }

    pub fn plain_help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    pub fn plain_note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            labels: Vec::new(),
            help: None,
            note: None,
            severity: Severity::Error,
            code: None,
            file_id: None,
            use_color: false,
        }
    }

    pub fn warn(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            labels: Vec::new(),
            help: None,
            note: None,
            severity: Severity::Warning,
            code: None,
            file_id: None,
            use_color: false,
        }
    }

    pub fn with_color(mut self, use_color: bool) -> Self {
        self.use_color = use_color;
        self
    }

    pub fn with_span_label(mut self, span: &Span, text: impl Into<String>) -> Self {
        if self.file_id.is_none() {
            self.file_id = Some(span.file_id);
        }
        self.labels.push(span_to_labeled(span, text.into(), false));
        self
    }

    pub fn with_span_primary_label(mut self, span: &Span, text: impl Into<String>) -> Self {
        if self.file_id.is_none() {
            self.file_id = Some(span.file_id);
        }
        self.labels.push(span_to_labeled(span, text.into(), true));
        self
    }

    pub fn with_labels(mut self, labels: Vec<LabeledSpan>) -> Self {
        self.labels.extend(labels);
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_lex_code(mut self, code: &str) -> Self {
        self.code = Some(format!("lex.{}", code));
        self
    }

    pub fn with_parse_code(mut self, code: &str) -> Self {
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

    pub fn with_lint_code(mut self, code: &str) -> Self {
        self.code = Some(format!("lint.{}", code));
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_source_code(self, source: IndexedSource, filename: String) -> miette::Report {
        miette::Report::new(self).with_source_code(miette::NamedSource::new(filename, source))
    }

    pub fn code_str(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn primary_offset(&self) -> usize {
        self.labels.first().map(|l| l.offset()).unwrap_or(0)
    }

    pub fn file_id(&self) -> Option<u32> {
        self.file_id
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }
}
