use diagnostics::{IndexedSource, LisetteDiagnostic};
use passes::Analysis;
use syntax::ParseError;

pub fn format_diagnostic_for_snapshot(
    diagnostic: &LisetteDiagnostic,
    source: &str,
    filename: &str,
) -> String {
    diagnostics::render::render_to_string(diagnostic, source, filename, false, 1)
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

/// Renders a diagnostic against every file it labels, the way the CLI does.
pub fn format_project_diagnostic_for_snapshot(
    result: &Analysis,
    diagnostic: &LisetteDiagnostic,
) -> String {
    let mut sources = diagnostics::render::SourceCache::new(|file_id: u32| {
        result
            .emit_input
            .files
            .get(&file_id)
            .map(|file| (file.source.clone(), file.name.clone()))
    });
    diagnostics::render::render_with_sources(diagnostic, &mut sources, false, 1)
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
    diagnostics::render::unix_line(diagnostic, Some((&IndexedSource::new(source), filename)))
}

pub fn format_parse_error_unix(error: &ParseError, source: &str, filename: &str) -> String {
    let diagnostic: LisetteDiagnostic = error.clone().into();
    format_diagnostic_unix(&diagnostic, source, filename)
}
