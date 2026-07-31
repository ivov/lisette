use diagnostics::{IndexedSource, LisetteDiagnostic};
use miette::{GraphicalReportHandler, GraphicalTheme, ThemeCharacters, ThemeStyles};
use passes::Analysis;
use syntax::ParseError;

fn snapshot_theme() -> GraphicalTheme {
    GraphicalTheme {
        characters: ThemeCharacters {
            error: "✕".into(),
            warning: "▲".into(),
            advice: "●".into(),
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

pub fn format_result_diagnostic_for_snapshot(
    result: &Analysis,
    diagnostic: &LisetteDiagnostic,
) -> String {
    let file_id = diagnostic
        .file_id()
        .expect("diagnostic must carry a file id");
    let file = result
        .emit_input
        .files
        .get(&file_id)
        .expect("diagnostic's file id must be present in the compiled result");
    format_diagnostic_for_snapshot(diagnostic, &file.source, &file.name)
}

/// Escapes carriage returns, which insta's YAML literal blocks cannot round-trip.
pub fn snapshot_description(input: &str) -> String {
    input.replace('\r', "\\r")
}

pub fn format_parse_error_for_snapshot(error: &ParseError, source: &str, filename: &str) -> String {
    let diagnostic: LisetteDiagnostic = error.clone().into();
    format_diagnostic_for_snapshot(&diagnostic, source, filename)
}

pub fn format_diagnostic_unix(
    diagnostic: &LisetteDiagnostic,
    source: &str,
    filename: &str,
) -> String {
    diagnostics::render::unix_line(diagnostic, &IndexedSource::new(source), filename)
}

pub fn format_parse_error_unix(error: &ParseError, source: &str, filename: &str) -> String {
    let diagnostic: LisetteDiagnostic = error.clone().into();
    format_diagnostic_unix(&diagnostic, source, filename)
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
