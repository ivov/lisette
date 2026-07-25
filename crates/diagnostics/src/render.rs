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

fn render(
    handler: &GraphicalReportHandler,
    diagnostic: &LisetteDiagnostic,
    source: &IndexedSource,
    filename: &str,
    use_color: bool,
) {
    let report = diagnostic.clone().into_rendered(use_color);
    let report = miette::Report::new(report)
        .with_source_code(miette::NamedSource::new(filename, source.clone()));
    let mut output = String::new();
    if handler.render_report(&mut output, report.as_ref()).is_ok() {
        eprintln!("{}", output);
    }
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
        let (src, name) = sources.get(diagnostic.file_id());
        render(&handler, diagnostic, &src, &name, use_color);
    }
}

pub struct Counts {
    pub files: usize,
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
}

/// Resolves a `file_id` to its source, falling back to the entry file.
struct SourceCache<F> {
    get_source: F,
    default_source: IndexedSource,
    default_filename: String,
    cache: FxHashMap<u32, (IndexedSource, String)>,
}

impl<F: Fn(u32) -> Option<(String, String)>> SourceCache<F> {
    fn new(get_source: F, default_source: &str, default_filename: &str) -> Self {
        Self {
            get_source,
            default_source: IndexedSource::new(default_source),
            default_filename: default_filename.to_string(),
            cache: FxHashMap::default(),
        }
    }

    fn get(&mut self, file_id: Option<u32>) -> (IndexedSource, String) {
        let Some(fid) = file_id else {
            return (self.default_source.clone(), self.default_filename.clone());
        };
        let default_source = &self.default_source;
        let default_filename = &self.default_filename;
        let get_source = &self.get_source;
        let entry = self.cache.entry(fid).or_insert_with(|| {
            get_source(fid)
                .map(|(src, name)| (IndexedSource::new(&src), name))
                .unwrap_or_else(|| (default_source.clone(), default_filename.clone()))
        });
        (entry.0.clone(), entry.1.clone())
    }
}

fn partition_diagnostics<'a>(
    errors: &'a [LisetteDiagnostic],
    lints: &'a [LisetteDiagnostic],
    filter: &Filter,
) -> DiagnosticGroups<'a> {
    let mut groups = DiagnosticGroups::default();

    for diagnostic in errors.iter().chain(lints.iter()) {
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
    errors: &[LisetteDiagnostic],
    lints: &[LisetteDiagnostic],
    get_source: impl Fn(u32) -> Option<(String, String)>,
    file_count: usize,
    filter: &Filter,
    default_source: &str,
    default_filename: &str,
) -> Counts {
    let groups = partition_diagnostics(errors, lints, filter);

    if !groups.is_empty() {
        eprintln!(); // Blank line before first diagnostic
    }

    let use_color = std::env::var("NO_COLOR").is_err();
    let mut sources = SourceCache::new(get_source, default_source, default_filename);

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

/// Renders one diagnostic as `file:line:col: severity: message [code]`.
pub fn unix_line(diagnostic: &LisetteDiagnostic, source: &IndexedSource, filename: &str) -> String {
    let mut line = String::new();
    if let Some(offset) = diagnostic.location_offset() {
        let (lineno, col) = source.line_col(offset);
        line.push_str(&format!("{}:{}:{}: ", filename, lineno, col));
    }
    line.push_str(diagnostic.severity_word());
    line.push_str(": ");
    line.push_str(diagnostic.plain_message());
    if let Some(code) = diagnostic.code_str() {
        line.push_str(&format!(" [{}]", code));
    }
    line
}

/// Builds the stdout text (one diagnostic per line, no color, no banner) and the
/// counts the caller needs for the stderr summary and exit code.
pub fn render_unix(
    errors: &[LisetteDiagnostic],
    lints: &[LisetteDiagnostic],
    get_source: impl Fn(u32) -> Option<(String, String)>,
    file_count: usize,
    filter: &Filter,
    default_source: &str,
    default_filename: &str,
) -> (String, Counts) {
    let groups = partition_diagnostics(errors, lints, filter);

    let mut sources = SourceCache::new(get_source, default_source, default_filename);
    let mut output = String::new();
    for diagnostic in groups
        .errors
        .iter()
        .chain(&groups.warnings)
        .chain(&groups.info)
    {
        let (src, name) = sources.get(diagnostic.file_id());
        output.push_str(&unix_line(diagnostic, &src, &name));
        output.push('\n');
    }

    let counts = groups.counts(file_count);
    (output, counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show_all() -> Filter {
        Filter::All
    }

    #[test]
    fn each_severity_lands_in_its_own_bucket() {
        let errors = vec![LisetteDiagnostic::error("e")];
        let lints = vec![LisetteDiagnostic::warn("w"), LisetteDiagnostic::info("i")];
        let groups = partition_diagnostics(&errors, &lints, &show_all());
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
        let empty: Vec<LisetteDiagnostic> = Vec::new();
        let lints = vec![LisetteDiagnostic::info("i")];
        let filter = Filter::Errors;
        let groups = partition_diagnostics(&empty, &lints, &filter);
        assert!(groups.info.is_empty());
    }

    #[test]
    fn info_hidden_under_warnings_only() {
        let empty: Vec<LisetteDiagnostic> = Vec::new();
        let lints = vec![LisetteDiagnostic::info("i")];
        let filter = Filter::Warnings;
        let groups = partition_diagnostics(&empty, &lints, &filter);
        assert!(groups.info.is_empty());
    }

    #[test]
    fn unix_counts_and_labels_info_separately() {
        let empty: Vec<LisetteDiagnostic> = Vec::new();
        let lints = vec![LisetteDiagnostic::info("advisory")];
        let (output, counts) = render_unix(&empty, &lints, |_| None, 1, &show_all(), "", "f.lis");
        assert_eq!(counts.errors, 0);
        assert_eq!(counts.warnings, 0);
        assert_eq!(counts.info, 1);
        assert!(output.contains("info: advisory"));
    }
}
