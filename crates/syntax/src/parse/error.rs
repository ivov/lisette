use crate::ast::Span;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub labels: Vec<(Span, String)>,
    pub help: Option<String>,
    pub note: Option<String>,
    pub code: String,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span, label: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            labels: vec![(span, label.into())],
            help: None,
            note: None,
            code: String::new(),
        }
    }

    pub fn with_span_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.labels.push((span, label.into()));
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
        self.code = format!("lex.{}", code);
        self
    }

    pub fn with_parse_code(mut self, code: &str) -> Self {
        self.code = format!("parse.{}", code);
        self
    }
}
