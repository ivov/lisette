use std::borrow::Cow;
use std::time::Duration;

use rustc_hash::FxHashMap;

use crate::LisetteDiagnostic;
use crate::diagnostic::IndexedSource;
use miette::{GraphicalReportHandler, GraphicalTheme, ThemeCharacters, ThemeStyles};
use owo_colors::{OwoColorize, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Graphical,
    Unix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Errors,
    Warnings,
}

impl Filter {
    fn show_errors(&self) -> bool {
        matches!(self, Self::All | Self::Errors)
    }

    fn show_warnings(&self) -> bool {
        matches!(self, Self::All | Self::Warnings)
    }

    fn show_info(&self) -> bool {
        matches!(self, Self::All)
    }
}

fn format_time(elapsed: Duration) -> String {
    if elapsed.as_secs() >= 1 {
        format!("{:.2}s", elapsed.as_secs_f64())
    } else if elapsed.as_millis() > 0 {
        format!("{}ms", elapsed.as_millis())
    } else {
        format!("{}μs", elapsed.as_micros())
    }
}

pub fn print_summary(
    file_count: usize,
    elapsed: Duration,
    errors: usize,
    warnings: usize,
    info: usize,
) {
    let time_string = format_time(elapsed);
    let use_color = std::env::var("NO_COLOR").is_err();
    let time_display = if use_color {
        format!("({})", time_string).dimmed().to_string()
    } else {
        format!("({})", time_string)
    };
    let files_str = if file_count == 1 {
        "1 file"
    } else {
        &format!("{} files", file_count)
    };

    if errors == 0 && warnings == 0 && info == 0 {
        let (mark, message) = if use_color {
            (
                "✓".green().to_string(),
                "All checks passed".green().to_string(),
            )
        } else {
            ("✓".to_string(), "All checks passed".to_string())
        };
        eprintln!("  {} {} · {} {}", mark, message, files_str, time_display);
    } else {
        let mut parts = Vec::new();
        if errors > 0 {
            let count = if errors == 1 {
                "1 error".to_string()
            } else {
                format!("{} errors", errors)
            };
            parts.push(if use_color {
                count.red().to_string()
            } else {
                count
            });
        }
        if warnings > 0 {
            let count = if warnings == 1 {
                "1 warning".to_string()
            } else {
                format!("{} warnings", warnings)
            };
            parts.push(if use_color {
                count.yellow().to_string()
            } else {
                count
            });
        }
        if info > 0 {
            let count = if info == 1 {
                "1 advisory".to_string()
            } else {
                format!("{} advisories", info)
            };
            parts.push(if use_color {
                count.blue().to_string()
            } else {
                count
            });
        }
        eprintln!("  {} · {} {}", parts.join(" · "), files_str, time_display);
    }
}

fn color_handler(highlight: Style) -> GraphicalReportHandler {
    let theme = GraphicalTheme {
        characters: ThemeCharacters {
            error: "✕".into(),
            warning: "▲".into(),
            advice: "●".into(),
            ..ThemeCharacters::unicode()
        },
        styles: ThemeStyles {
            error: Style::new().red(),
            warning: Style::new().yellow(),
            advice: Style::new().blue(),
            link: Style::new(),
            help: Style::new().dimmed(),
            highlights: vec![highlight],
            ..ThemeStyles::ansi()
        },
    };
    GraphicalReportHandler::new_themed(theme).with_wrap_lines(false)
}

fn accent_handler(accent: Style) -> GraphicalReportHandler {
    let theme = GraphicalTheme {
        characters: ThemeCharacters {
            error: "✕".into(),
            warning: "▲".into(),
            advice: "●".into(),
            ..ThemeCharacters::unicode()
        },
        styles: ThemeStyles {
            error: Style::new().red(),
            warning: Style::new().yellow(),
            advice: accent,
            link: Style::new(),
            help: Style::new().dimmed(),
            highlights: vec![accent],
            ..ThemeStyles::ansi()
        },
    };
    GraphicalReportHandler::new_themed(theme).with_wrap_lines(false)
}

fn nocolor_handler() -> GraphicalReportHandler {
    let theme = GraphicalTheme {
        characters: ThemeCharacters {
            error: "✕".into(),
            warning: "▲".into(),
            advice: "●".into(),
            ..ThemeCharacters::unicode()
        },
        styles: ThemeStyles::none(),
    };
    GraphicalReportHandler::new_themed(theme).with_wrap_lines(false)
}

fn graphical_report(
    handler: &GraphicalReportHandler,
    diagnostic: &LisetteDiagnostic,
    source: Option<(&IndexedSource, &str)>,
    use_color: bool,
) -> Option<String> {
    let report = miette::Report::new(diagnostic.clone().into_rendered(use_color));
    let report = match source {
        Some((source, filename)) => {
            report.with_source_code(miette::NamedSource::new(filename, source.clone()))
        }
        None => report,
    };
    let mut output = String::new();
    handler.render_report(&mut output, report.as_ref()).ok()?;
    Some(output)
}

pub fn render_to_string(
    diagnostic: &LisetteDiagnostic,
    source: &str,
    filename: &str,
    use_color: bool,
    context_lines: usize,
) -> String {
    let accent = if diagnostic.is_error() {
        Style::new().red()
    } else if diagnostic.is_info() {
        Style::new().blue()
    } else {
        Style::new().yellow()
    };
    let handler = if use_color {
        accent_handler(accent)
    } else {
        nocolor_handler()
    }
    .with_context_lines(context_lines);
    let report = diagnostic.clone().into_rendered(use_color);
    let report = miette::Report::new(report).with_source_code(miette::NamedSource::new(
        filename,
        IndexedSource::new(source),
    ));
    let mut output = String::new();
    let _ = handler.render_report(&mut output, report.as_ref());
    output
}

fn render_group<F: Fn(u32) -> Option<(String, String)>>(
    diagnostics: &[&LisetteDiagnostic],
    highlight: Style,
    use_color: bool,
    sources: &mut SourceCache<F>,
) {
    if diagnostics.is_empty() {
        return;
    }
    let handler = if use_color {
        color_handler(highlight)
    } else {
        nocolor_handler()
    };
    for diagnostic in diagnostics {
        let source = sources.get(diagnostic.file_id());
        if let Some(output) = graphical_report(&handler, diagnostic, source, use_color) {
            eprintln!("{}", output);
        }
    }
}

pub struct Counts {
    pub files: usize,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
}

/// Resolves the file a diagnostic's spans are measured against.
pub struct SourceCache<F> {
    get_source: F,
    cache: FxHashMap<u32, Option<(IndexedSource, String)>>,
}

impl<F: Fn(u32) -> Option<(String, String)>> SourceCache<F> {
    pub fn new(get_source: F) -> Self {
        Self {
            get_source,
            cache: FxHashMap::default(),
        }
    }

    fn get(&mut self, file_id: Option<u32>) -> Option<(&IndexedSource, &str)> {
        let file_id = file_id?;
        let get_source = &self.get_source;
        let entry = self.cache.entry(file_id).or_insert_with(|| {
            get_source(file_id).map(|(source, name)| (IndexedSource::new(&source), name))
        });
        entry.as_ref().map(|(source, name)| (source, name.as_str()))
    }
}

fn partition_diagnostics<'a>(
    diagnostics: &'a [LisetteDiagnostic],
    filter: &Filter,
) -> DiagnosticGroups<'a> {
    let mut groups = DiagnosticGroups::default();

    for diagnostic in diagnostics {
        if diagnostic.is_error() {
            if filter.show_errors() {
                groups.errors.push(diagnostic);
            }
        } else if diagnostic.is_info() {
            if filter.show_info() {
                groups.info.push(diagnostic);
            }
        } else if filter.show_warnings() {
            groups.warnings.push(diagnostic);
        }
    }

    groups
}

#[derive(Default)]
struct DiagnosticGroups<'a> {
    errors: Vec<&'a LisetteDiagnostic>,
    warnings: Vec<&'a LisetteDiagnostic>,
    info: Vec<&'a LisetteDiagnostic>,
}

impl DiagnosticGroups<'_> {
    fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty() && self.info.is_empty()
    }

    fn counts(&self, file_count: usize) -> Counts {
        Counts {
            files: file_count.max(1),
            errors: self.errors.len(),
            warnings: self.warnings.len(),
            info: self.info.len(),
        }
    }
}

pub fn render_all(
    diagnostics: &[LisetteDiagnostic],
    mut sources: SourceCache<impl Fn(u32) -> Option<(String, String)>>,
    file_count: usize,
    filter: &Filter,
) -> Counts {
    let groups = partition_diagnostics(diagnostics, filter);

    if !groups.is_empty() {
        eprintln!(); // Blank line before first diagnostic
    }

    let use_color = std::env::var("NO_COLOR").is_err();

    render_group(&groups.errors, Style::new().red(), use_color, &mut sources);
    render_group(
        &groups.warnings,
        Style::new().yellow(),
        use_color,
        &mut sources,
    );
    render_group(&groups.info, Style::new().blue(), use_color, &mut sources);

    groups.counts(file_count)
}

fn breaks_line(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{b}' | '\u{c}')
}

fn flatten_to_one_line(text: &str) -> Cow<'_, str> {
    if !text.contains(breaks_line) {
        return Cow::Borrowed(text);
    }

    let mut flattened = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if !character.is_whitespace() {
            flattened.push(character);
            continue;
        }

        let mut end = start + character.len_utf8();
        while let Some(&(index, next)) = chars.peek() {
            if !next.is_whitespace() {
                break;
            }
            end = index + next.len_utf8();
            chars.next();
        }

        let run = &text[start..end];
        if run.contains(breaks_line) {
            flattened.push(' ');
        } else {
            flattened.push_str(run);
        }
    }

    Cow::Owned(flattened.trim().to_string())
}

/// Renders one diagnostic as `file:line:col: severity: message: label · help [code flags]`.
pub fn unix_line(diagnostic: &LisetteDiagnostic, source: Option<(&IndexedSource, &str)>) -> String {
    let mut line = String::new();
    if let Some((source, filename)) = source
        && let Some(offset) = diagnostic.location_offset()
    {
        let (lineno, column) = source.line_col(offset);
        line.push_str(&format!(
            "{}:{}:{}: ",
            flatten_to_one_line(filename),
            lineno,
            column
        ));
    } else if let Some(path) = diagnostic.file_location() {
        line.push_str(&format!("{}:1:1: ", flatten_to_one_line(path)));
    }

    line.push_str(diagnostic.severity_word());
    line.push_str(": ");
    line.push_str(&flatten_to_one_line(diagnostic.plain_message()));

    if let Some(label) = diagnostic.located_label() {
        line.push_str(": ");
        line.push_str(&flatten_to_one_line(label));
    }

    if let Some(help) = diagnostic.help_and_note() {
        line.push_str(" · ");
        line.push_str(&flatten_to_one_line(&help));
    }

    if let Some(code) = diagnostic.code_str() {
        line.push_str(&format!(" [{}]", code));
    }
    if diagnostic.fix().is_some() {
        line.push_str(" [autofixable]");
    }

    line
}

/// Builds the stdout text (one diagnostic per line, no color, no banner) and the
/// counts the caller needs for the stderr summary and exit code.
pub fn render_unix(
    diagnostics: &[LisetteDiagnostic],
    mut sources: SourceCache<impl Fn(u32) -> Option<(String, String)>>,
    file_count: usize,
    filter: &Filter,
) -> (String, Counts) {
    let groups = partition_diagnostics(diagnostics, filter);

    let mut output = String::new();
    for diagnostic in groups
        .errors
        .iter()
        .chain(&groups.warnings)
        .chain(&groups.info)
    {
        output.push_str(&unix_line(diagnostic, sources.get(diagnostic.file_id())));
        output.push('\n');
    }

    let counts = groups.counts(file_count);
    (output, counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::Span;

    const ENTRY_SOURCE: &str = "fn main() {\n  let x = 1;\n  let y = 2;\n}\n";

    fn show_all() -> Filter {
        Filter::All
    }

    fn entry_only(file_id: u32) -> Option<(String, String)> {
        (file_id == 0).then(|| (ENTRY_SOURCE.to_string(), "src/main.lis".to_string()))
    }

    fn points_into(file_id: u32) -> LisetteDiagnostic {
        LisetteDiagnostic::error("Points into another file")
            .with_span_primary_label(&Span::new(file_id, 32, 6), "over here")
    }

    fn graphical_output(diagnostic: &LisetteDiagnostic) -> String {
        let mut sources = SourceCache::new(entry_only);
        let source = sources.get(diagnostic.file_id());
        graphical_report(&nocolor_handler(), diagnostic, source, false).expect("rendering succeeds")
    }

    #[test]
    fn each_severity_lands_in_its_own_bucket() {
        let diagnostics = vec![
            LisetteDiagnostic::error("e"),
            LisetteDiagnostic::warn("w"),
            LisetteDiagnostic::info("i"),
        ];
        let groups = partition_diagnostics(&diagnostics, &show_all());
        assert_eq!(
            (
                groups.errors.len(),
                groups.warnings.len(),
                groups.info.len()
            ),
            (1, 1, 1)
        );
    }

    #[test]
    fn info_hidden_under_errors_only() {
        let diagnostics = vec![LisetteDiagnostic::info("i")];
        let filter = Filter::Errors;
        let groups = partition_diagnostics(&diagnostics, &filter);
        assert!(groups.info.is_empty());
    }

    #[test]
    fn info_hidden_under_warnings_only() {
        let diagnostics = vec![LisetteDiagnostic::info("i")];
        let filter = Filter::Warnings;
        let groups = partition_diagnostics(&diagnostics, &filter);
        assert!(groups.info.is_empty());
    }

    #[test]
    fn unix_omits_the_location_of_an_unresolved_file() {
        let (output, _) = render_unix(
            &[points_into(7)],
            SourceCache::new(entry_only),
            1,
            &show_all(),
        );
        assert_eq!(output, "error: Points into another file: over here\n");
    }

    #[test]
    fn unix_locates_a_resolved_file() {
        let (output, _) = render_unix(
            &[points_into(0)],
            SourceCache::new(entry_only),
            1,
            &show_all(),
        );
        assert_eq!(
            output,
            "src/main.lis:3:8: error: Points into another file: over here\n"
        );
    }

    #[test]
    fn graphical_omits_the_source_frame_of_an_unresolved_file() {
        let output = graphical_output(&points_into(7));
        assert!(output.contains("Points into another file"));
        assert!(!output.contains("src/main.lis"));
        assert!(!output.contains("let y = 2"));
    }

    #[test]
    fn graphical_draws_the_source_frame_of_a_resolved_file() {
        let output = graphical_output(&points_into(0));
        assert!(output.contains("src/main.lis"));
        assert!(output.contains("let y = 2"));
    }

    #[test]
    fn unix_counts_and_labels_info_separately() {
        let diagnostics = vec![LisetteDiagnostic::info("advisory")];
        let (output, counts) =
            render_unix(&diagnostics, SourceCache::new(|_| None), 1, &show_all());
        assert_eq!(counts.errors, 0);
        assert_eq!(counts.warnings, 0);
        assert_eq!(counts.info, 1);
        assert!(output.contains("info: advisory"));
    }
}
