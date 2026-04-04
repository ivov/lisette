use std::time::Duration;

use rustc_hash::FxHashMap;

use crate::LisetteDiagnostic;
use crate::diagnostic::IndexedSource;
use miette::{GraphicalReportHandler, GraphicalTheme, ThemeCharacters, ThemeStyles};
use owo_colors::{OwoColorize, Style};

pub struct Filter {
    pub errors_only: bool,
    pub warnings_only: bool,
}

impl Filter {
    pub fn show_errors(&self) -> bool {
        !self.warnings_only
    }

    pub fn show_warnings(&self) -> bool {
        !self.errors_only
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

pub fn print_summary(file_count: usize, elapsed: Duration, errors: i32, warnings: i32) {
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

    if errors == 0 && warnings == 0 {
        eprintln!("  ✓ No issues · {} {}", files_str, time_display);
    } else {
        let mut parts = Vec::new();
        if errors > 0 {
            parts.push(if errors == 1 {
                "1 error".to_string()
            } else {
                format!("{} errors", errors)
            });
        }
        if warnings > 0 {
            parts.push(if warnings == 1 {
                "1 warning".to_string()
            } else {
                format!("{} warnings", warnings)
            });
        }
        let findings = format!("Found {}", parts.join(", "));
        let findings_display = if use_color {
            format!("{}", findings.bold())
        } else {
            findings
        };
        eprintln!("  ✖ {} · {} {}", findings_display, files_str, time_display);
    }
}

fn color_handler(highlight: Style) -> GraphicalReportHandler {
    let theme = GraphicalTheme {
        characters: ThemeCharacters {
            error: "🔴".into(),
            warning: "🟡".into(),
            ..ThemeCharacters::unicode()
        },
        styles: ThemeStyles {
            error: Style::new().red(),
            warning: Style::new().yellow(),
            link: Style::new(),
            help: Style::new().dimmed(),
            highlights: vec![highlight],
            ..ThemeStyles::ansi()
        },
    };
    GraphicalReportHandler::new_themed(theme).with_wrap_lines(false)
}

fn nocolor_handler() -> GraphicalReportHandler {
    let theme = GraphicalTheme {
        characters: ThemeCharacters {
            error: "[error]".into(),
            warning: "[warning]".into(),
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
    let report = diagnostic
        .clone()
        .with_color(use_color)
        .with_source_code(source.clone(), filename.to_string());
    let mut output = String::new();
    if handler.render_report(&mut output, report.as_ref()).is_ok() {
        eprintln!("{}", output);
    }
}

pub struct Counts {
    pub files: usize,
    pub errors: i32,
    pub warnings: i32,
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
    let show_errors = filter.show_errors();
    let show_warnings = filter.show_warnings();

    let (errors, infer_warnings): (Vec<_>, Vec<_>) = if show_errors {
        errors.iter().partition(|d| d.is_error())
    } else {
        (Vec::new(), Vec::new())
    };

    let warnings: Vec<_> = if show_warnings {
        infer_warnings.into_iter().chain(lints.iter()).collect()
    } else {
        Vec::new()
    };

    let has_diagnostics = !errors.is_empty() || !warnings.is_empty();
    if has_diagnostics {
        eprintln!(); // Blank line before first diagnostic
    }

    let use_color = std::env::var("NO_COLOR").is_err();

    let default_source = IndexedSource::new(default_source);
    let default_filename = default_filename.to_string();
    let mut source_cache: FxHashMap<u32, (IndexedSource, String)> = FxHashMap::default();
    let get_cached_source =
        |file_id: Option<u32>, cache: &mut FxHashMap<u32, (IndexedSource, String)>| {
            if let Some(fid) = file_id {
                let entry = cache.entry(fid).or_insert_with(|| {
                    get_source(fid)
                        .map(|(src, name)| (IndexedSource::new(&src), name))
                        .unwrap_or_else(|| (default_source.clone(), default_filename.clone()))
                });
                (entry.0.clone(), entry.1.clone())
            } else {
                (default_source.clone(), default_filename.clone())
            }
        };

    if !errors.is_empty() {
        let handler = if use_color {
            color_handler(Style::new().red())
        } else {
            nocolor_handler()
        };
        for error in &errors {
            let (src, name) = get_cached_source(error.file_id(), &mut source_cache);
            render(&handler, error, &src, &name, use_color);
        }
    }

    if !warnings.is_empty() {
        let handler = if use_color {
            color_handler(Style::new().yellow())
        } else {
            nocolor_handler()
        };
        for warning in &warnings {
            let (src, name) = get_cached_source(warning.file_id(), &mut source_cache);
            render(&handler, warning, &src, &name, use_color);
        }
    }

    Counts {
        files: file_count.max(1),
        errors: errors.len() as i32,
        warnings: warnings.len() as i32,
    }
}
