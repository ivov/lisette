use diagnostics::{IndexedSource, LisetteDiagnostic};
use miette::{GraphicalReportHandler, GraphicalTheme, ThemeCharacters, ThemeStyles};
use syntax::ParseError;

fn snapshot_theme() -> GraphicalTheme {
    GraphicalTheme {
        characters: ThemeCharacters {
            error: "[error]".into(),
            warning: "[warning]".into(),
            ..ThemeCharacters::unicode()
        },
        styles: ThemeStyles::none(),
    }
}

pub fn format_diagnostic_for_snapshot(
    diagnostic: &LisetteDiagnostic,
    source: &str,
    filename: &str,
) -> String {
    let handler = GraphicalReportHandler::new()
        .with_theme(snapshot_theme())
        .with_wrap_lines(false);

    let report = diagnostic
        .clone()
        .with_source_code(IndexedSource::new(source), filename.to_string());

    let mut output = String::new();
    handler.render_report(&mut output, report.as_ref()).unwrap();
    output
}

pub fn format_parse_error_for_snapshot(error: &ParseError, source: &str, filename: &str) -> String {
    let diagnostic: LisetteDiagnostic = error.clone().into();
    format_diagnostic_for_snapshot(&diagnostic, source, filename)
}

pub fn format_diagnostic_standalone(diagnostic: &LisetteDiagnostic) -> String {
    let handler = GraphicalReportHandler::new()
        .with_theme(snapshot_theme())
        .with_wrap_lines(false);
    let report = miette::Report::new(diagnostic.clone());

    let mut output = String::new();
    handler.render_report(&mut output, report.as_ref()).unwrap();
    output
}
