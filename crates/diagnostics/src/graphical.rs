//! Renders a diagnostic as a message, labeled source frames, and help footer.
//!
//! Derived from the `GraphicalReportHandler` in miette 7.6.0
//! (<https://github.com/zkat/miette>, copyright Kat Marchán and miette
//! contributors, Apache-2.0).

use std::fmt::{self, Write as _};

use owo_colors::{OwoColorize, Style};

use crate::diagnostic::IndexedSource;

const HBAR: char = '─';
const VBAR: char = '│';
const VBAR_BREAK: char = '·';
const UARROW: char = '▲';
const RARROW: char = '▶';
const LTOP: char = '╭';
const LBOT: char = '╰';
const LCROSS: char = '├';
const RCROSS: char = '┤';
const UNDERBAR: char = '┬';
const UNDERLINE: char = '─';
const TAB_WIDTH: usize = 4;

pub(crate) struct FrameTheme {
    pub icon: &'static str,
    pub severity: Style,
    pub highlight: Style,
    pub line_number: Style,
    pub location: Style,
    pub help: Style,
}

#[derive(Clone)]
pub(crate) struct FrameLabel {
    offset: usize,
    length: usize,
    parts: Vec<String>,
    primary: bool,
}

impl PartialEq for FrameLabel {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts && self.offset == other.offset && self.length == other.length
    }
}

impl FrameLabel {
    pub(crate) fn new(offset: usize, length: usize, text: String, primary: bool) -> Self {
        Self {
            offset,
            length,
            parts: text.split('\n').map(str::to_string).collect(),
            primary,
        }
    }

    fn end(&self) -> usize {
        self.offset + self.length
    }

    /// Zero-length spans occupy one column when deciding line membership.
    fn effective_end(&self) -> usize {
        self.offset + self.length.max(1)
    }

    fn joined_text(&self) -> String {
        self.parts.join("\n")
    }

    fn styled_parts(&self, style: Style) -> Vec<String> {
        self.parts
            .iter()
            .map(|part| part.style(style).to_string())
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LabelRenderMode {
    SingleLine,
    MultiLineFirst,
    MultiLineRest,
}

/// Marker columns of the single-line labels under one source line.
struct LabelRow<'a> {
    line: &'a FrameLine<'a>,
    columns: &'a [(&'a FrameLabel, usize)],
}

struct FrameLine<'a> {
    line_number: usize,
    offset: usize,
    length: usize,
    text: &'a str,
}

impl FrameLine<'_> {
    fn end(&self) -> usize {
        self.offset + self.length
    }

    fn span_line_only(&self, label: &FrameLabel) -> bool {
        label.offset >= self.offset && label.end() <= self.end()
    }

    fn span_applies(&self, label: &FrameLabel) -> bool {
        label.offset < self.end() && label.effective_end() > self.offset
    }

    fn span_applies_gutter(&self, label: &FrameLabel) -> bool {
        self.span_applies(label)
            && !(label.offset >= self.offset && label.effective_end() <= self.end())
    }

    fn span_flyby(&self, label: &FrameLabel) -> bool {
        label.offset < self.offset && label.end() > self.end()
    }

    fn span_starts(&self, label: &FrameLabel) -> bool {
        label.offset >= self.offset
    }

    fn span_ends(&self, label: &FrameLabel) -> bool {
        label.end() >= self.offset && label.end() <= self.end()
    }
}

/// The line window a span occupies once context lines are included.
struct ContextWindow {
    offset: usize,
    length: usize,
    start_line: usize,
    last_line: usize,
    end_offset: usize,
}

impl ContextWindow {
    fn end(&self) -> usize {
        self.offset + self.length
    }
}

fn read_window(
    source: &IndexedSource,
    offset: usize,
    length: usize,
    context_lines: usize,
) -> Result<ContextWindow, ()> {
    let text = source.text();
    if offset + length > text.len() {
        return Err(());
    }
    let line_starts = source.line_starts();
    let span_line = source.line_index_of_offset(offset);
    let start_line = span_line.saturating_sub(context_lines);
    let end_line = source.line_index_of_offset(offset + length.saturating_sub(1));
    let last_line = (end_line + context_lines).min(line_starts.len() - 1);
    let end_offset = if last_line + 1 < line_starts.len() {
        line_starts[last_line + 1].min(text.len())
    } else {
        text.len()
    };
    Ok(ContextWindow {
        offset,
        length,
        start_line,
        last_line,
        end_offset,
    })
}

pub(crate) struct FrameSource {
    pub source: IndexedSource,
    pub filename: String,
    pub labels: Vec<FrameLabel>,
}

pub(crate) struct FrameReport<'a> {
    pub message: &'a str,
    pub sources: &'a [FrameSource],
    pub help: Option<&'a str>,
}

pub(crate) fn render_report(
    output: &mut String,
    report: &FrameReport<'_>,
    theme: &FrameTheme,
    context_lines: usize,
) -> fmt::Result {
    render_message(output, report.message, theme)?;
    for frame in report.sources {
        render_snippets(
            output,
            &frame.source,
            &frame.filename,
            &frame.labels,
            theme,
            context_lines,
        )?;
    }
    if let Some(help) = report.help {
        render_help(output, help, theme)?;
    }
    Ok(())
}

/// Prefixes each line of a block without wrapping, dropping trailing
/// whitespace from the indent on blank lines.
fn indent_block(text: &str, initial_indent: &str, subsequent_indent: &str) -> String {
    let mut result = String::with_capacity(2 * text.len());
    let trimmed_indent = subsequent_indent.trim_end();
    for (index, line) in text.split_terminator('\n').enumerate() {
        if index > 0 {
            result.push('\n');
        }
        if index == 0 {
            if line.trim().is_empty() {
                result.push_str(initial_indent.trim_end());
            } else {
                result.push_str(initial_indent);
            }
        } else if line.trim().is_empty() {
            result.push_str(trimmed_indent);
        } else {
            result.push_str(subsequent_indent);
        }
        result.push_str(line);
    }
    if text.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn render_message(output: &mut String, message: &str, theme: &FrameTheme) -> fmt::Result {
    let initial_indent = format!("  {} ", theme.icon.style(theme.severity));
    let rest_indent = format!("  {} ", VBAR.style(theme.severity));
    writeln!(
        output,
        "{}",
        indent_block(message, &initial_indent, &rest_indent)
    )
}

fn render_help(output: &mut String, help: &str, theme: &FrameTheme) -> fmt::Result {
    let initial_indent = "  help: ".style(theme.help).to_string();
    writeln!(
        output,
        "{}",
        indent_block(help, &initial_indent, "        ")
    )
}

fn render_snippets(
    output: &mut String,
    source: &IndexedSource,
    filename: &str,
    labels: &[FrameLabel],
    theme: &FrameTheme,
    context_lines: usize,
) -> fmt::Result {
    let mut sorted = labels.to_vec();
    sorted.sort_unstable_by_key(|label| label.offset);

    let mut contexts: Vec<ContextWindow> = Vec::with_capacity(sorted.len());
    for right in &sorted {
        let right_window = match read_window(source, right.offset, right.length, context_lines) {
            Ok(window) => window,
            Err(()) => {
                writeln!(
                    output,
                    "  [{} `{}` (offset: {}, length: {}): OutOfBounds]",
                    "Failed to read contents for label".style(theme.severity),
                    right.joined_text().style(theme.location),
                    right.offset.style(theme.location),
                    right.length.style(theme.location),
                )?;
                return Ok(());
            }
        };

        if let Some(left) = contexts.last()
            && left.last_line + 1 >= right_window.start_line
            && let Ok(merged) = read_window(
                source,
                left.offset,
                left.end().max(right.end()) - left.offset,
                context_lines,
            )
        {
            contexts.pop();
            contexts.push(merged);
            continue;
        }

        contexts.push(right_window);
    }

    for window in &contexts {
        ContextRenderer::new(source, &sorted, window, theme).render(output, filename)?;
    }
    Ok(())
}

struct ContextRenderer<'a> {
    theme: &'a FrameTheme,
    source: &'a IndexedSource,
    window: &'a ContextWindow,
    labels: &'a [FrameLabel],
    lines: Vec<FrameLine<'a>>,
    max_gutter: usize,
    line_number_width: usize,
}

impl<'a> ContextRenderer<'a> {
    fn new(
        source: &'a IndexedSource,
        labels: &'a [FrameLabel],
        window: &'a ContextWindow,
        theme: &'a FrameTheme,
    ) -> Self {
        let lines = context_lines_of(source, window);

        let mut max_gutter = 0;
        for line in &lines {
            let mut applicable = 0;
            for label in labels {
                if !line.span_line_only(label) && line.span_applies_gutter(label) {
                    applicable += 1;
                }
            }
            max_gutter = max_gutter.max(applicable);
        }

        let line_number_width = lines
            .last()
            .map(|line| line.line_number)
            .unwrap_or(0)
            .to_string()
            .len();

        Self {
            theme,
            source,
            window,
            labels,
            lines,
            max_gutter,
            line_number_width,
        }
    }

    fn render(&self, output: &mut String, filename: &str) -> fmt::Result {
        let contained = |label: &&FrameLabel| {
            self.window.offset <= label.offset && label.end() <= self.window.end()
        };
        let primary_label = self
            .labels
            .iter()
            .filter(contained)
            .find(|label| label.primary)
            .or_else(|| self.labels.iter().find(contained))
            .expect("every context contains the label that created it");

        write!(
            output,
            "{}{}{}",
            " ".repeat(self.line_number_width + 2),
            LTOP,
            HBAR,
        )?;

        let (location_line, location_column) = self.source.line_col(primary_label.offset);
        writeln!(
            output,
            "[{}]",
            format_args!("{}:{}:{}", filename, location_line, location_column)
                .style(self.theme.location)
        )?;

        for line in &self.lines {
            self.write_line_number(output, line.line_number)?;
            self.render_line_gutter(output, line)?;
            self.render_line_text(output, line)?;

            let (single_line, multi_line): (Vec<_>, Vec<_>) = self
                .labels
                .iter()
                .filter(|label| line.span_applies(label))
                .partition(|label| line.span_line_only(label));
            if !single_line.is_empty() {
                self.write_no_line_number(output)?;
                self.render_highlight_gutter(output, line, LabelRenderMode::SingleLine)?;
                self.render_single_line_highlights(output, line, &single_line)?;
            }
            for label in multi_line {
                if line.span_ends(label) && !line.span_starts(label) {
                    self.render_multi_line_end(output, line, label)?;
                }
            }
        }
        writeln!(
            output,
            "{}{}{}",
            " ".repeat(self.line_number_width + 2),
            LBOT,
            HBAR.to_string().repeat(4),
        )?;
        Ok(())
    }

    fn write_line_number(&self, output: &mut String, line_number: usize) -> fmt::Result {
        write!(
            output,
            " {:width$} {} ",
            line_number.style(self.theme.line_number),
            VBAR,
            width = self.line_number_width,
        )
    }

    fn write_no_line_number(&self, output: &mut String) -> fmt::Result {
        write!(
            output,
            " {:width$} {} ",
            "",
            VBAR_BREAK,
            width = self.line_number_width,
        )
    }

    /// Visual width of each character, expanding tabs and skipping ANSI escapes.
    fn visual_char_widths<'t>(&self, text: &'t str) -> impl Iterator<Item = usize> + 't {
        let mut column = 0;
        let mut escaped = false;
        text.chars().map(move |character| {
            let width = match (escaped, character) {
                (false, '\t') => TAB_WIDTH - column % TAB_WIDTH,
                (false, '\x1b') => {
                    escaped = true;
                    0
                }
                (false, character) => {
                    unicode_width::UnicodeWidthChar::width(character).unwrap_or(0)
                }
                (true, 'm') => {
                    escaped = false;
                    0
                }
                (true, _) => 0,
            };
            column += width;
            width
        })
    }

    /// Visual column of a byte offset on a line. Offsets past the end of the
    /// line land one column past the visible text.
    fn visual_offset(&self, line: &FrameLine, offset: usize, start: bool) -> usize {
        let line_range = line.offset..=(line.offset + line.length);
        assert!(line_range.contains(&offset));

        let mut text_index = offset - line.offset;
        while text_index <= line.text.len() && !line.text.is_char_boundary(text_index) {
            if start {
                text_index -= 1;
            } else {
                text_index += 1;
            }
        }
        let text = &line.text[..text_index.min(line.text.len())];
        let text_width = self.visual_char_widths(text).sum();
        if text_index > line.text.len() {
            text_width + 1
        } else {
            text_width
        }
    }

    /// Byte ranges of this line's text covered by labels, merged and
    /// line-relative, so the snippet renders in the highlight color.
    fn covered_ranges(&self, line: &FrameLine<'_>) -> Vec<(usize, usize)> {
        let line_start = line.offset;
        let line_end = line.offset + line.text.len();
        let mut ranges: Vec<(usize, usize)> = self
            .labels
            .iter()
            .filter(|label| label.length > 0)
            .map(|label| (label.offset.max(line_start), label.end().min(line_end)))
            .filter(|(start, end)| start < end)
            .map(|(start, end)| (start - line_start, end - line_start))
            .collect();
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            match merged.last_mut() {
                Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }

    fn render_line_text(&self, output: &mut String, line: &FrameLine<'_>) -> fmt::Result {
        let style = self.theme.highlight;
        let covered = self.covered_ranges(line);
        let mut inside = false;
        let widths = self.visual_char_widths(line.text);
        for ((byte_index, character), width) in line.text.char_indices().zip(widths) {
            let now_inside = covered
                .iter()
                .any(|&(start, end)| byte_index >= start && byte_index < end);
            if now_inside != inside {
                if now_inside {
                    write!(output, "{}", style.prefix_formatter())?;
                } else {
                    write!(output, "{}", style.suffix_formatter())?;
                }
                inside = now_inside;
            }
            if character == '\t' {
                for _ in 0..width {
                    output.write_char(' ')?;
                }
            } else {
                output.write_char(character)?;
            }
        }
        if inside {
            write!(output, "{}", style.suffix_formatter())?;
        }
        output.write_char('\n')?;
        Ok(())
    }

    fn render_line_gutter(&self, output: &mut String, line: &FrameLine) -> fmt::Result {
        if self.max_gutter == 0 {
            return Ok(());
        }
        let style = self.theme.highlight;
        let mut gutter = String::new();
        let applicable = self
            .labels
            .iter()
            .filter(|label| line.span_applies_gutter(label));
        let mut arrow = false;
        for (index, label) in applicable.enumerate() {
            if line.span_starts(label) {
                gutter.push_str(&LTOP.style(style).to_string());
                gutter.push_str(
                    &HBAR
                        .to_string()
                        .repeat(self.max_gutter.saturating_sub(index))
                        .style(style)
                        .to_string(),
                );
                gutter.push_str(&RARROW.style(style).to_string());
                arrow = true;
                break;
            } else if line.span_ends(label) {
                gutter.push_str(&LCROSS.style(style).to_string());
                gutter.push_str(
                    &HBAR
                        .to_string()
                        .repeat(self.max_gutter.saturating_sub(index))
                        .style(style)
                        .to_string(),
                );
                gutter.push_str(&RARROW.style(style).to_string());
                arrow = true;
                break;
            } else if line.span_flyby(label) {
                gutter.push_str(&VBAR.style(style).to_string());
            } else {
                gutter.push(' ');
            }
        }
        write!(
            output,
            "{}{}",
            gutter,
            " ".repeat(
                if arrow { 1 } else { 3 } + self.max_gutter.saturating_sub(gutter.chars().count())
            ),
        )?;
        Ok(())
    }

    fn render_highlight_gutter(
        &self,
        output: &mut String,
        line: &FrameLine,
        render_mode: LabelRenderMode,
    ) -> fmt::Result {
        if self.max_gutter == 0 {
            return Ok(());
        }

        let style = self.theme.highlight;
        let mut gutter_columns = 0;
        let mut gutter = String::new();
        let applicable = self
            .labels
            .iter()
            .filter(|label| line.span_applies_gutter(label));
        for (index, label) in applicable.enumerate() {
            if !line.span_line_only(label) && line.span_ends(label) {
                if render_mode == LabelRenderMode::MultiLineRest {
                    let horizontal_space = self.max_gutter.saturating_sub(index) + 2;
                    for _ in 0..horizontal_space {
                        gutter.push(' ');
                    }
                    gutter_columns += horizontal_space + 1;
                } else {
                    let repeats = self.max_gutter.saturating_sub(index) + 2;
                    gutter.push_str(&LBOT.style(style).to_string());
                    gutter.push_str(
                        &HBAR
                            .to_string()
                            .repeat(
                                repeats
                                    - if render_mode == LabelRenderMode::MultiLineFirst {
                                        1
                                    } else {
                                        0
                                    },
                            )
                            .style(style)
                            .to_string(),
                    );
                    gutter_columns += repeats + 1;
                }
                break;
            } else {
                gutter.push_str(&VBAR.style(style).to_string());
                gutter_columns += 1;
            }
        }

        let padding = (self.max_gutter + 3).saturating_sub(gutter_columns);
        write!(output, "{}{:padding$}", gutter, "")?;
        Ok(())
    }

    fn render_single_line_highlights(
        &self,
        output: &mut String,
        line: &FrameLine,
        single_line: &[&FrameLabel],
    ) -> fmt::Result {
        let style = self.theme.highlight;
        let mut underlines = String::new();
        let mut highest = 0;

        let columns: Vec<(&FrameLabel, usize)> = single_line
            .iter()
            .map(|label| {
                let byte_start = label.offset;
                let byte_end = label.end();
                let start = self.visual_offset(line, byte_start, true).max(highest);
                let end = if label.length == 0 {
                    start + 1
                } else {
                    self.visual_offset(line, byte_end, false).max(start + 1)
                };

                let marker_column = (start + end) / 2;
                let dashes_left = marker_column - start;
                let dashes_right = end - marker_column - 1;
                underlines.push_str(
                    &format!(
                        "{:padding$}{}{}{}",
                        "",
                        UNDERLINE.to_string().repeat(dashes_left),
                        if label.length == 0 { UARROW } else { UNDERBAR },
                        UNDERLINE.to_string().repeat(dashes_right),
                        padding = start.saturating_sub(highest),
                    )
                    .style(style)
                    .to_string(),
                );
                highest = highest.max(end);

                (*label, marker_column)
            })
            .collect();
        writeln!(output, "{}", underlines)?;

        let row = LabelRow {
            line,
            columns: &columns,
        };
        for label in single_line.iter().rev() {
            let parts = label.styled_parts(style);
            if parts.len() == 1 {
                self.write_label_text(output, &row, label, &parts[0], LabelRenderMode::SingleLine)?;
            } else {
                let mut first = true;
                for part in &parts {
                    self.write_label_text(
                        output,
                        &row,
                        label,
                        part,
                        if first {
                            LabelRenderMode::MultiLineFirst
                        } else {
                            LabelRenderMode::MultiLineRest
                        },
                    )?;
                    first = false;
                }
            }
        }
        Ok(())
    }

    fn write_label_text(
        &self,
        output: &mut String,
        row: &LabelRow<'_>,
        label: &FrameLabel,
        text: &str,
        render_mode: LabelRenderMode,
    ) -> fmt::Result {
        let style = self.theme.highlight;
        self.write_no_line_number(output)?;
        self.render_highlight_gutter(output, row.line, LabelRenderMode::SingleLine)?;
        let mut current_column = 1usize;
        for (column_label, marker_column) in row.columns {
            while current_column < *marker_column + 1 {
                write!(output, " ")?;
                current_column += 1;
            }
            if *column_label != label {
                write!(output, "{}", VBAR.to_string().style(style))?;
                current_column += 1;
            } else {
                let connector = match render_mode {
                    LabelRenderMode::SingleLine => {
                        format!("{}{} {}", LBOT, HBAR.to_string().repeat(2), text)
                    }
                    LabelRenderMode::MultiLineFirst => {
                        format!("{}{}{} {}", LBOT, HBAR, RCROSS, text)
                    }
                    LabelRenderMode::MultiLineRest => {
                        format!("  {} {}", VBAR, text)
                    }
                };
                writeln!(output, "{}", connector.style(style))?;
                break;
            }
        }
        Ok(())
    }

    fn render_multi_line_end(
        &self,
        output: &mut String,
        line: &FrameLine,
        label: &FrameLabel,
    ) -> fmt::Result {
        self.write_no_line_number(output)?;

        let parts = label.styled_parts(self.theme.highlight);
        let (first, rest) = parts
            .split_first()
            .expect("split on newline always yields at least one part");

        if rest.is_empty() {
            self.render_highlight_gutter(output, line, LabelRenderMode::SingleLine)?;
            self.render_multi_line_end_single(output, first, LabelRenderMode::SingleLine)?;
        } else {
            self.render_highlight_gutter(output, line, LabelRenderMode::MultiLineFirst)?;
            self.render_multi_line_end_single(output, first, LabelRenderMode::MultiLineFirst)?;
            for part in rest {
                self.write_no_line_number(output)?;
                self.render_highlight_gutter(output, line, LabelRenderMode::MultiLineRest)?;
                self.render_multi_line_end_single(output, part, LabelRenderMode::MultiLineRest)?;
            }
        }
        Ok(())
    }

    fn render_multi_line_end_single(
        &self,
        output: &mut String,
        text: &str,
        render_mode: LabelRenderMode,
    ) -> fmt::Result {
        let style = self.theme.highlight;
        match render_mode {
            LabelRenderMode::SingleLine => {
                writeln!(output, "{} {}", HBAR.style(style), text)?;
            }
            LabelRenderMode::MultiLineFirst => {
                writeln!(output, "{} {}", RCROSS.style(style), text)?;
            }
            LabelRenderMode::MultiLineRest => {
                writeln!(output, "{} {}", VBAR.style(style), text)?;
            }
        }
        Ok(())
    }
}

/// Splits a context window into rendered lines. A line's length keeps its
/// newline bytes, its text drops them, and a lone trailing CR stays visible.
fn context_lines_of<'a>(source: &'a IndexedSource, window: &ContextWindow) -> Vec<FrameLine<'a>> {
    let text = source.text();
    let line_starts = source.line_starts();
    let mut lines = Vec::with_capacity(window.last_line - window.start_line + 1);
    for line_index in window.start_line..=window.last_line {
        let line_offset = line_starts[line_index];
        if line_offset >= window.end_offset {
            break;
        }
        let line_end = if line_index + 1 < line_starts.len() {
            line_starts[line_index + 1].min(window.end_offset)
        } else {
            window.end_offset
        };
        let raw = &text[line_offset..line_end];
        let line_text = raw
            .strip_suffix('\n')
            .map(|stripped| stripped.strip_suffix('\r').unwrap_or(stripped))
            .unwrap_or(raw);
        lines.push(FrameLine {
            line_number: line_index + 1,
            offset: line_offset,
            length: raw.len(),
            text: line_text,
        });
    }
    lines
}
