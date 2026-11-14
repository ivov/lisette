use crate::plan::go_expression::{Position, SourcePosition};
use std::fmt;
use std::mem;

/// Writes a `/*line*/` directive only where a token needs a different line.
#[derive(Default)]
pub(crate) struct GoWriter {
    text: String,
    directives: bool,
    line: Option<Position>,
    pending: Option<Position>,
    inline: bool,
    separator: bool,
}

impl GoWriter {
    pub(crate) fn new(directives: bool) -> Self {
        Self {
            directives,
            ..Self::default()
        }
    }

    pub(crate) fn continuing(text: String, directives: bool) -> Self {
        Self {
            text,
            ..Self::new(directives)
        }
    }

    pub(crate) fn into_text(self) -> String {
        self.text
    }

    pub(crate) fn directives(&self) -> bool {
        self.directives
    }

    pub(crate) fn push(&mut self, character: char) {
        if character == '\n' {
            if self.inline {
                self.separator = true;
                return;
            }
            self.next_line();
        } else {
            self.write_separator();
            if !matches!(character, ')' | ']' | '}' | ',' | ';' | ':' | ' ' | '\t') {
                self.write_pending();
            }
        }
        self.text.push(character);
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        for character in text.chars() {
            self.push(character);
        }
    }

    pub(crate) fn push_foreign(&mut self, text: &str) {
        self.write_separator();
        self.write_pending();
        for character in text.chars() {
            if character == '\n' {
                self.next_line();
            }
            self.text.push(character);
        }
    }

    pub(crate) fn push_position(&mut self, position: &SourcePosition) {
        if let Some(position) = position.get()
            && self.directives
        {
            self.push_directive(position);
        }
    }

    pub(crate) fn push_line_directive(&mut self, directive: &str) {
        if !self.directives || self.inline || self.text.ends_with(directive) {
            return;
        }
        self.text.push_str(directive);
        self.pending = None;
        self.line = Position::from_line_directive(directive);
    }

    pub(crate) fn line(&self) -> Option<Position> {
        self.line.clone()
    }

    /// The next token on the line returns to the literal's start, unless the literal has an end.
    pub(crate) fn close_literal(&mut self, end: &SourcePosition, start: Option<Position>) {
        if !self.directives {
            return;
        }
        match end.get() {
            Some(end) => self.push_directive(end),
            None if !same_line(start.as_ref(), self.line.as_ref()) => self.pending = start,
            None => {}
        }
    }

    pub(crate) fn set_inline(&mut self, inline: bool) -> bool {
        self.separator = false;
        mem::replace(&mut self.inline, inline)
    }

    fn next_line(&mut self) {
        self.pending = None;
        if let Some(line) = &mut self.line {
            line.line += 1;
            line.column = 1;
        }
    }

    fn write_separator(&mut self) {
        if mem::take(&mut self.separator) {
            self.text.push_str("; ");
        }
    }

    fn write_pending(&mut self) {
        if let Some(position) = self.pending.take() {
            self.push_directive(&position);
        }
    }

    fn push_directive(&mut self, position: &Position) {
        self.write_separator();
        self.pending = None;
        if same_line(Some(position), self.line.as_ref()) {
            return;
        }
        match self.indentation_start() {
            Some(start) => self.push_directive_line(position, start),
            None => self.text.push_str(&position.inline_directive()),
        }
        self.line = Some(position.clone());
    }

    fn indentation_start(&self) -> Option<usize> {
        let start = self.text.rfind('\n')? + 1;
        self.text[start..]
            .chars()
            .all(|c| c == '\t' || c == ' ')
            .then_some(start)
    }

    /// gofmt moves a `/*line*/` directive at the start of a line onto a line of its own.
    fn push_directive_line(&mut self, position: &Position, start: usize) {
        let indentation = self.text.split_off(start);
        let previous = self.text[..start - 1].rfind('\n').map_or(0, |at| at + 1);
        if self.text[previous..].starts_with("//line ") {
            self.text.truncate(previous);
        }
        self.text.push_str(&position.line_directive());
        self.text.push_str(&indentation);
    }
}

fn same_line(left: Option<&Position>, right: Option<&Position>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.line == right.line && left.path == right.path,
        (None, None) => true,
        _ => false,
    }
}

impl fmt::Write for GoWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.push_str(text);
        Ok(())
    }
}
