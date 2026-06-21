use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use owo_colors::OwoColorize;
use semantics::store::ENTRY_MODULE_ID;
use serde::Deserialize;
use syntax::ast::Span;

use crate::go_cli::GoTestEvent;
use crate::output::{format_backticks, format_elapsed};
use diagnostics::LisetteDiagnostic;
use lisette::pipeline::{Sources, TestIndex};

/// Per (package, test): expected chunk count `n` and the gathered `(index, hex)` chunks.
type FailChunks = HashMap<(String, String), (usize, Vec<(usize, String)>)>;

const FAIL_ATTR_KEY: &str = "lisette-fail";

const DESC_MAX_WIDTH: usize = 100;
const DESC_MIN_WIDTH: usize = 20;
const DESC_MAX_LINES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Passed,
    Failed,
    Aborted,
    NotRun,
}

/// One framed chunk of a failure record: `d` concatenated over `i in 0..n`.
#[derive(Deserialize)]
struct FailEnvelope {
    i: usize,
    n: usize,
    d: String,
}

#[derive(Deserialize, Clone)]
struct Operand {
    label: String,
    value: String,
}

#[derive(Deserialize, Clone)]
pub struct FailureRecord {
    file: u32,
    lo: u32,
    hi: u32,
    #[serde(default)]
    kind: String,
    message: String,
    #[serde(default)]
    operands: Vec<Operand>,
}

pub struct TestRow {
    pub package: String,
    pub name: String,
    pub description: Option<String>,
    pub status: Status,
    pub elapsed: Option<f64>,
    pub output: String,
    pub failure: Option<FailureRecord>,
    pub children: Vec<TestRow>,
    pub span: Span,
}

pub struct Report {
    pub rows: Vec<TestRow>,
    pub build_output: String,
    package_output: HashMap<String, String>,
    failed_packages: HashSet<String>,
    build_failed_packages: HashSet<String>,
    go_module: String,
    pub test_elapsed: f64,
}

fn name_or_title_contains(fn_name: &str, title: Option<&str>, pattern: &str) -> bool {
    fn_name.contains(pattern) || title.is_some_and(|t| t.contains(pattern))
}

pub fn matching_tests(index: &TestIndex, go_module: &str, filter: &str) -> Vec<(String, String)> {
    index
        .tests()
        .iter()
        .filter_map(|test| {
            let prefix = format!("{}.", test.module_id);
            let fn_name = test
                .qualified_name
                .strip_prefix(&prefix)
                .unwrap_or(&test.qualified_name);
            if !name_or_title_contains(fn_name, test.title.as_deref(), filter) {
                return None;
            }
            let package = if test.module_id == ENTRY_MODULE_ID {
                go_module.to_string()
            } else {
                format!("{}/{}", go_module, test.module_id)
            };
            Some((package, go_test_name(fn_name)))
        })
        .collect()
}

#[cfg(test)]
pub fn build_report(index: &TestIndex, events: &[GoTestEvent], go_module: &str) -> Report {
    build_report_filtered(index, events, go_module, None)
}

pub fn build_report_filtered(
    index: &TestIndex,
    events: &[GoTestEvent],
    go_module: &str,
    filter: Option<&str>,
) -> Report {
    let mut terminal: HashMap<(String, String), (Status, Option<f64>)> = HashMap::new();
    let mut started: HashSet<(String, String)> = HashSet::new();
    let mut outputs: HashMap<(String, String), String> = HashMap::new();
    let mut build_output = String::new();
    let mut package_output: HashMap<String, String> = HashMap::new();
    let mut failed_packages: HashSet<String> = HashSet::new();
    let mut build_failed_packages: HashSet<String> = HashSet::new();
    let mut fail_chunks: FailChunks = HashMap::new();
    let mut test_elapsed: f64 = 0.0;

    for event in events {
        if event.action == "attr"
            && event.key.as_deref() == Some(FAIL_ATTR_KEY)
            && let (Some(test), Some(value)) = (&event.test, &event.value)
            && let Ok(envelope) = serde_json::from_str::<FailEnvelope>(value)
        {
            let entry = fail_chunks
                .entry((event.package.clone(), test.clone()))
                .or_insert((envelope.n, Vec::new()));
            entry.0 = envelope.n;
            entry.1.push((envelope.i, envelope.d));
            continue;
        }
        let Some(test) = &event.test else {
            match event.action.as_str() {
                "build-output" => {
                    if let Some(text) = &event.output {
                        build_output.push_str(text);
                    }
                    if let Some(path) = &event.import_path {
                        build_failed_packages.insert(package_of_import_path(path).to_string());
                    }
                }
                "build-fail" => {
                    if let Some(path) = &event.import_path {
                        build_failed_packages.insert(package_of_import_path(path).to_string());
                    }
                }
                "output" => {
                    if let Some(text) = &event.output {
                        package_output
                            .entry(event.package.clone())
                            .or_default()
                            .push_str(text);
                    }
                }
                "pass" => {
                    test_elapsed = test_elapsed.max(event.elapsed.unwrap_or(0.0));
                }
                "fail" => {
                    failed_packages.insert(event.package.clone());
                    test_elapsed = test_elapsed.max(event.elapsed.unwrap_or(0.0));
                }
                _ => {}
            }
            continue;
        };
        let key = (event.package.clone(), test.clone());
        match event.action.as_str() {
            "run" => {
                started.insert(key);
            }
            "pass" => {
                terminal.insert(key, (Status::Passed, event.elapsed));
            }
            "fail" => {
                terminal.insert(key, (Status::Failed, event.elapsed));
            }
            "output" => {
                if let Some(text) = &event.output {
                    outputs.entry(key).or_default().push_str(text);
                }
            }
            _ => {}
        }
    }

    let failures = reassemble_failures(fail_chunks);

    let mut rows = Vec::new();
    for test in index.tests() {
        let prefix = format!("{}.", test.module_id);
        let fn_name = test
            .qualified_name
            .strip_prefix(&prefix)
            .unwrap_or(&test.qualified_name);
        if let Some(pattern) = filter
            && !name_or_title_contains(fn_name, test.title.as_deref(), pattern)
        {
            continue;
        }
        let package = if test.module_id == ENTRY_MODULE_ID {
            go_module.to_string()
        } else {
            format!("{}/{}", go_module, test.module_id)
        };
        let go_name = go_test_name(fn_name);
        let key = (package.clone(), go_name.clone());
        let (status, elapsed) = match terminal.get(&key).copied() {
            Some(found) => found,
            None if started.contains(&key) => (Status::Aborted, None),
            None => (Status::NotRun, None),
        };
        let children =
            collect_children(&package, &go_name, &terminal, &started, &outputs, &failures);
        rows.push(TestRow {
            package,
            name: test.title.clone().unwrap_or_else(|| fn_name.to_string()),
            description: test.doc.clone(),
            status,
            elapsed,
            output: outputs.get(&key).cloned().unwrap_or_default(),
            failure: failures.get(&key).cloned(),
            children,
            span: test.span,
        });
    }
    Report {
        rows,
        build_output,
        package_output,
        failed_packages,
        build_failed_packages,
        go_module: go_module.to_string(),
        test_elapsed,
    }
}

fn go_test_name(fn_name: &str) -> String {
    emit::go_test_function_name(fn_name)
}

/// Requires every index `0..n`; a missing chunk drops to raw output, not a truncated diagnostic.
fn reassemble_failures(chunks: FailChunks) -> HashMap<(String, String), FailureRecord> {
    chunks
        .into_iter()
        .filter_map(|(key, (n, parts))| reassemble_one(n, parts).map(|record| (key, record)))
        .collect()
}

fn reassemble_one(n: usize, parts: Vec<(usize, String)>) -> Option<FailureRecord> {
    if n == 0 {
        return None;
    }
    let mut slots: Vec<Option<String>> = vec![None; n];
    for (i, d) in parts {
        *slots.get_mut(i)? = Some(d);
    }
    let mut joined = String::new();
    for slot in slots {
        joined.push_str(&slot?);
    }
    let bytes = decode_hex(&joined)?;
    serde_json::from_slice::<FailureRecord>(&bytes).ok()
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((nibble(pair[0])? << 4) | nibble(pair[1])?))
        .collect()
}

fn collect_children(
    package: &str,
    parent: &str,
    terminal: &HashMap<(String, String), (Status, Option<f64>)>,
    started: &HashSet<(String, String)>,
    outputs: &HashMap<(String, String), String>,
    failures: &HashMap<(String, String), FailureRecord>,
) -> Vec<TestRow> {
    let prefix = format!("{parent}/");
    let mut segments: Vec<&str> = terminal
        .keys()
        .chain(started.iter())
        .filter(|(pkg, name)| pkg == package && name.starts_with(&prefix))
        .map(|(_, name)| name[prefix.len()..].split('/').next().unwrap_or(""))
        .collect();
    segments.sort_unstable();
    segments.dedup();

    segments
        .into_iter()
        .map(|segment| {
            let full = format!("{parent}/{segment}");
            let key = (package.to_string(), full.clone());
            let (status, elapsed) = match terminal.get(&key).copied() {
                Some(found) => found,
                None if started.contains(&key) => (Status::Aborted, None),
                None => (Status::NotRun, None),
            };
            TestRow {
                package: package.to_string(),
                name: segment.to_string(),
                description: None,
                status,
                elapsed,
                output: outputs.get(&key).cloned().unwrap_or_default(),
                failure: failures.get(&key).cloned(),
                children: collect_children(package, &full, terminal, started, outputs, failures),
                span: Span::new(0, 0, 0),
            }
        })
        .collect()
}

/// `go test` names a build failure `pkg [pkg.test]`; strip the suffix to match package events.
fn package_of_import_path(import_path: &str) -> &str {
    import_path
        .split_once(" [")
        .map_or(import_path, |(pkg, _)| pkg)
}

fn package_display<'a>(package: &'a str, go_module: &str) -> &'a str {
    if package == go_module {
        package.rsplit('/').next().unwrap_or(package)
    } else if let Some(rel) = package
        .strip_prefix(go_module)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        rel
    } else {
        package
    }
}

pub fn render(report: &Report, sources: &Sources, color: bool, total: Duration) -> String {
    let mut out = String::from("\n");

    if report.rows.is_empty() {
        out.push_str("  No tests found\n");
        return out;
    }

    let mut by_package: BTreeMap<&str, Vec<&TestRow>> = BTreeMap::new();
    for row in &report.rows {
        by_package.entry(&row.package).or_default().push(row);
    }

    let term_width = terminal_width();
    for (index, (package, mut group)) in by_package.into_iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        group.sort_by(|a, b| {
            let file_a = sources.get(&a.span.file_id).map(|s| s.filename.as_str());
            let file_b = sources.get(&b.span.file_id).map(|s| s.filename.as_str());
            (file_a, a.span.byte_offset).cmp(&(file_b, b.span.byte_offset))
        });
        let header = package_display(package, &report.go_module);
        let header = if color {
            header.bright_magenta().to_string()
        } else {
            header.to_string()
        };
        out.push_str(&format!("  {header}\n"));
        render_rows(&mut out, &group, "    ", color, term_width);

        // Crash before any test ran (init/`TestMain` panic): cause is package-level only.
        if !report.build_failed_packages.contains(package)
            && report.failed_packages.contains(package)
            && group.iter().all(|r| r.status == Status::NotRun)
            && let Some(text) = report.package_output.get(package)
        {
            for line in text.lines() {
                let line = line.trim_end();
                if line.is_empty() {
                    continue;
                }
                out.push_str(&dim(&format!("        {line}"), color));
                out.push('\n');
            }
        }
    }

    render_failures(&mut out, &report.rows, &report.go_module, sources, color);

    out.push('\n');
    out.push_str(&summary(&report.rows, total, color));
    out
}

fn render_rows(out: &mut String, rows: &[&TestRow], prefix: &str, color: bool, term_width: usize) {
    let any_described = rows.iter().any(|r| r.description.is_some());
    for (i, row) in rows.iter().enumerate() {
        let last = i + 1 == rows.len();
        let branch = if last { "└── " } else { "├── " };
        let timing = match row.elapsed {
            Some(seconds) if seconds > 0.0 => {
                format!(" {}", format_elapsed(Duration::from_secs_f64(seconds)))
            }
            _ => String::new(),
        };
        if row.children.is_empty() {
            let glyph = if row.status == Status::NotRun {
                dim("⊘", color)
            } else {
                mark(row.status, color)
            };
            let suffix = match row.status {
                Status::Aborted => dim(" (aborted)", color),
                _ => String::new(),
            };
            out.push_str(&format!(
                "{prefix}{branch}{glyph} {}{suffix}{timing}\n",
                format_backticks(&row.name, color)
            ));
        } else {
            out.push_str(&format!(
                "{prefix}{branch}{}{timing}\n",
                format_backticks(&row.name, color)
            ));
        }

        let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });

        if let Some(description) = &row.description {
            let line = description.split_whitespace().collect::<Vec<_>>().join(" ");
            if !line.is_empty() {
                let gutter = if row.children.is_empty() {
                    "  "
                } else {
                    "│ "
                };
                let indent = child_prefix.chars().count() + gutter.chars().count();
                let width = term_width
                    .saturating_sub(indent)
                    .clamp(DESC_MIN_WIDTH, DESC_MAX_WIDTH);
                for wrapped in wrap_description(&line, width, DESC_MAX_LINES) {
                    out.push_str(&format!(
                        "{child_prefix}{gutter}{}\n",
                        format_description(&wrapped, color)
                    ));
                }
            }
        }

        let children: Vec<&TestRow> = row.children.iter().collect();
        render_rows(out, &children, &child_prefix, color, term_width);

        if any_described && !last {
            out.push_str(&format!("{prefix}│\n"));
        }
    }
}

fn render_failures(
    out: &mut String,
    rows: &[TestRow],
    go_module: &str,
    sources: &Sources,
    color: bool,
) {
    // The flat Failures section loses the tree's package grouping, so prefix the package when the run
    // spans more than one, to disambiguate same-named tests across packages.
    let multi_package = rows
        .iter()
        .map(|r| &r.package)
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let mut blocks: Vec<(String, Option<String>, String)> = Vec::new();
    for row in rows {
        let prefix = if multi_package {
            package_display(&row.package, go_module).to_string()
        } else {
            String::new()
        };
        collect_failures(row, &prefix, sources, color, &mut blocks);
    }
    if blocks.is_empty() {
        return;
    }

    out.push('\n');
    let heading = if color {
        "Failures".bold().to_string()
    } else {
        "Failures".to_string()
    };
    out.push_str(&format!("  {heading}\n"));
    for (path, kind, body) in blocks {
        out.push('\n');
        let glyph = mark(Status::Failed, color);
        let name = format_backticks(&path, color);
        match kind {
            Some(kind) => out.push_str(&format!("  {glyph} {name} · {kind}\n")),
            None => out.push_str(&format!("  {glyph} {name}\n")),
        }
        for line in body.lines() {
            out.push_str(&format!("    {line}\n"));
        }
    }
}

fn collect_failures(
    row: &TestRow,
    prefix: &str,
    sources: &Sources,
    color: bool,
    blocks: &mut Vec<(String, Option<String>, String)>,
) {
    let path = if prefix.is_empty() {
        row.name.clone()
    } else {
        format!("{prefix} › {}", row.name)
    };

    if matches!(row.status, Status::Failed | Status::Aborted) {
        if let Some((kind, body)) = row
            .failure
            .as_ref()
            .and_then(|record| render_failure(record, sources, color))
        {
            blocks.push((path.clone(), Some(kind), body));
        } else if !has_failing_descendant(row) {
            let text = row
                .output
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.is_empty())
                .map(|line| dim(line, color))
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                blocks.push((path.clone(), None, text));
            }
        }
    }

    for child in &row.children {
        collect_failures(child, &path, sources, color, blocks);
    }
}

fn has_failing_descendant(row: &TestRow) -> bool {
    row.children.iter().any(|child| {
        matches!(child.status, Status::Failed | Status::Aborted) || has_failing_descendant(child)
    })
}

fn render_failure(
    record: &FailureRecord,
    sources: &Sources,
    color: bool,
) -> Option<(String, String)> {
    let info = sources.get(&record.file)?;
    let span = Span::new(record.file, record.lo, record.hi.saturating_sub(record.lo));

    let (label, notes): (String, Vec<String>) = if record.kind == "labeled" {
        let label = record
            .operands
            .iter()
            .map(|operand| format!("{}: {}", operand.label, operand.value))
            .collect::<Vec<_>>()
            .join(" · ");
        (label, Vec::new())
    } else {
        let label = record
            .operands
            .first()
            .map(|operand| operand.value.clone())
            .unwrap_or_else(|| record.message.clone());
        let notes = record
            .operands
            .iter()
            .skip(1)
            .map(|operand| format!("{}: {}", operand.label, operand.value))
            .collect();
        (label, notes)
    };

    let mut diagnostic =
        LisetteDiagnostic::error(record.message.clone()).with_span_primary_label(&span, label);
    if !notes.is_empty() {
        diagnostic = diagnostic.with_note(notes.join("\n"));
    }
    let rendered =
        diagnostics::render::render_to_string(&diagnostic, &info.source, &info.filename, color);
    let body = rendered.lines().skip(1).collect::<Vec<_>>().join("\n");
    Some((record.message.clone(), body))
}

fn summary(rows: &[TestRow], total: Duration, color: bool) -> String {
    let passed = rows.iter().filter(|r| r.status == Status::Passed).count();
    let failed = rows.iter().filter(|r| r.status == Status::Failed).count();
    let aborted = rows.iter().filter(|r| r.status == Status::Aborted).count();
    let not_run = rows.iter().filter(|r| r.status == Status::NotRun).count();

    let any_failure = failed > 0 || aborted > 0;
    let glyph = mark(
        if any_failure {
            Status::Failed
        } else {
            Status::Passed
        },
        color,
    );

    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(red(&format!("{failed} failed"), color));
    }
    if aborted > 0 {
        parts.push(red(&format!("{aborted} aborted"), color));
    }
    parts.push(green(&format!("{passed} passed"), color));
    if not_run > 0 {
        parts.push(format!("{not_run} not run"));
    }

    format!(
        "  {glyph} {} {}\n",
        parts.join(" · "),
        format_elapsed(total)
    )
}

fn green(text: &str, color: bool) -> String {
    if color {
        text.green().to_string()
    } else {
        text.to_string()
    }
}

fn red(text: &str, color: bool) -> String {
    if color {
        text.red().to_string()
    } else {
        text.to_string()
    }
}

fn format_description(text: &str, color: bool) -> String {
    if !color {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let prose = &rest[..open];
        if !prose.is_empty() {
            out.push_str(&prose.dimmed().to_string());
        }
        let after = &rest[open + 1..];
        match after.find('`') {
            Some(close) => {
                let code = &after[..close];
                if !code.is_empty() {
                    out.push_str(&code.bright_magenta().dimmed().to_string());
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&format!("`{after}").dimmed().to_string());
                return out;
            }
        }
    }
    if !rest.is_empty() {
        out.push_str(&rest.dimmed().to_string());
    }
    out
}

fn terminal_width() -> usize {
    terminal_size::terminal_size_of(std::io::stderr())
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(100)
}

fn wrap_description(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        let mut rest = word;
        while rest.chars().count() > width {
            let split = rest
                .char_indices()
                .nth(width)
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            lines.push(rest[..split].to_string());
            rest = &rest[split..];
        }
        current = rest.to_string();
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let head: String = last.chars().take(width.saturating_sub(1)).collect();
            *last = format!("{}…", head.trim_end());
        }
    }
    lines
}

/// A non-`Passed` row fails the run only when `go test` itself did; a filtered test must not.
pub fn exit_code(rows: &[TestRow], run_success: bool) -> i32 {
    let any_failure = rows
        .iter()
        .any(|r| matches!(r.status, Status::Failed | Status::Aborted));
    if !run_success || any_failure { 1 } else { 0 }
}

fn mark(status: Status, color: bool) -> String {
    let symbol = match status {
        Status::Passed => "✓",
        Status::Failed | Status::Aborted => "✗",
        Status::NotRun => "⊘",
    };
    if !color {
        return symbol.to_string();
    }
    match status {
        Status::Passed => symbol.green().to_string(),
        Status::Failed | Status::Aborted => symbol.red().to_string(),
        Status::NotRun => symbol.to_string(),
    }
}

fn dim(text: &str, color: bool) -> String {
    if color {
        text.dimmed().to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lisette::pipeline::SourceInfo;
    use syntax::ast::Span;
    use syntax::program::TestFunction;

    fn span() -> Span {
        Span::new(0, 0, 1)
    }

    fn index(entries: &[(&str, &str)]) -> TestIndex {
        let mut index = TestIndex::default();
        for (module_id, name) in entries {
            index.push(TestFunction {
                module_id: module_id.to_string(),
                qualified_name: format!("{module_id}.{name}"),
                title: None,
                doc: None,
                span: span(),
            });
        }
        index
    }

    fn event(action: &str, package: &str, test: Option<&str>, output: Option<&str>) -> GoTestEvent {
        GoTestEvent {
            action: action.to_string(),
            package: package.to_string(),
            test: test.map(str::to_string),
            elapsed: Some(0.003),
            output: output.map(str::to_string),
            import_path: None,
            key: None,
            value: None,
        }
    }

    fn build_output_event(package: &str, output: &str) -> GoTestEvent {
        GoTestEvent {
            action: "build-output".to_string(),
            package: String::new(),
            test: None,
            elapsed: None,
            output: Some(output.to_string()),
            import_path: Some(format!("{package} [{package}.test]")),
            key: None,
            value: None,
        }
    }

    fn attr_event(package: &str, test: &str, value: &str) -> GoTestEvent {
        GoTestEvent {
            action: "attr".to_string(),
            package: package.to_string(),
            test: Some(test.to_string()),
            elapsed: None,
            output: None,
            import_path: None,
            key: Some(FAIL_ATTR_KEY.to_string()),
            value: Some(value.to_string()),
        }
    }

    fn no_sources() -> Sources {
        Sources::default()
    }

    fn hex_encode(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02x}")).collect()
    }

    fn fail_value(inner_json: &str) -> String {
        format!(r#"{{"i":0,"n":1,"d":"{}"}}"#, hex_encode(inner_json))
    }

    #[test]
    fn wrap_description_wraps_and_truncates() {
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";

        let short = wrap_description("alpha beta", 40, 3);
        assert_eq!(short, vec!["alpha beta"]);

        let wrapped = wrap_description(text, 16, 3);
        assert!(wrapped.len() <= 3);
        assert!(wrapped.iter().all(|l| l.chars().count() <= 16));
        assert!(
            wrapped.last().unwrap().ends_with('…'),
            "a too-long description truncates its last line, got: {wrapped:?}"
        );

        let long_word = wrap_description("supercalifragilisticexpialidocious", 10, 3);
        assert!(long_word.iter().all(|l| l.chars().count() <= 10));
    }

    #[test]
    fn all_pass_groups_by_package() {
        let index = index(&[(ENTRY_MODULE_ID, "root_smoke"), ("math", "adds_numbers")]);
        let events = vec![
            event("pass", "demo", Some("TestRootSmoke"), None),
            event("pass", "demo/math", Some("TestAddsNumbers"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(7));

        assert!(text.contains("  demo\n"));
        assert!(text.contains("✓ root_smoke"));
        assert!(text.contains("  math\n"));
        assert!(text.contains("✓ adds_numbers"));
        assert!(text.contains("2 passed"));
        assert_eq!(exit_code(&report.rows, true), 0);
    }

    #[test]
    fn subtests_nest_under_their_parent() {
        let index = index(&[(ENTRY_MODULE_ID, "parent")]);
        let events = vec![
            event("run", "demo", Some("TestParent"), None),
            event("run", "demo", Some("TestParent/alpha"), None),
            event("pass", "demo", Some("TestParent/alpha"), None),
            event("pass", "demo", Some("TestParent"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].children.len(), 1);
        assert_eq!(report.rows[0].children[0].name, "alpha");

        let text = render(&report, &no_sources(), false, Duration::from_millis(1));
        let parent_line = text.lines().position(|l| l.contains("parent")).unwrap();
        let child_line = text.lines().position(|l| l.contains("alpha")).unwrap();
        assert!(child_line > parent_line, "subtest renders under its parent");
        assert!(text.contains("✓ alpha"), "leaf subtest keeps its mark");
        assert!(
            !text.contains("✓ parent"),
            "a passing grouping has no redundant tick, got:\n{text}"
        );
        assert!(text.contains("1 passed"));
    }

    #[test]
    fn parent_own_output_shows_even_with_subtests() {
        let index = index(&[(ENTRY_MODULE_ID, "parent")]);
        let events = vec![
            event("run", "demo", Some("TestParent"), None),
            event("run", "demo", Some("TestParent/child"), None),
            event("pass", "demo", Some("TestParent/child"), None),
            event(
                "output",
                "demo",
                Some("TestParent"),
                Some("parent panicked\n"),
            ),
            event("fail", "demo", Some("TestParent"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));

        let tree = text.split("Failures").next().unwrap_or(&text);
        assert!(
            !tree.contains("✗ parent") && !tree.contains("✓ parent"),
            "a grouping carries no sigil in the tree, got:\n{text}"
        );
        assert!(text.contains("✓ child"));
        assert!(
            text.contains("parent panicked"),
            "a failed parent's own output must show even with subtests, got:\n{text}"
        );
    }

    #[test]
    fn nested_subtest_failure_attaches_to_leaf() {
        let index = index(&[(ENTRY_MODULE_ID, "parent")]);
        let events = vec![
            event("run", "demo", Some("TestParent"), None),
            event("run", "demo", Some("TestParent/group"), None),
            event("run", "demo", Some("TestParent/group/inner"), None),
            event(
                "output",
                "demo",
                Some("TestParent/group/inner"),
                Some("boom\n"),
            ),
            event("fail", "demo", Some("TestParent/group/inner"), None),
            event("fail", "demo", Some("TestParent/group"), None),
            event("fail", "demo", Some("TestParent"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let inner = &report.rows[0].children[0].children[0];
        assert_eq!(inner.name, "inner");
        assert_eq!(inner.status, Status::Failed);

        let text = render(&report, &no_sources(), false, Duration::from_millis(1));
        assert!(text.contains("boom"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn failed_test_shows_output_and_counts() {
        let index = index(&[(ENTRY_MODULE_ID, "boom")]);
        let events = vec![
            event("fail", "demo", Some("TestBoom"), None),
            event("output", "demo", Some("TestBoom"), Some("panic: boom\n")),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(2));

        assert!(text.contains("✗ boom"));
        assert!(text.contains("panic: boom"));
        assert!(text.contains("1 failed"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn declared_test_with_no_event_is_not_run() {
        let index = index(&[(ENTRY_MODULE_ID, "ghost")]);
        let report = build_report(&index, &[], "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));

        assert!(text.contains("⊘ ghost"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn filtered_run_does_not_fail_when_go_succeeds() {
        let index = index(&[(ENTRY_MODULE_ID, "kept"), (ENTRY_MODULE_ID, "filtered")]);
        let events = vec![event("pass", "demo", Some("TestKept"), None)];
        let report = build_report(&index, &events, "demo");

        assert_eq!(exit_code(&report.rows, true), 0);
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));
        assert!(text.contains("⊘ filtered"));
        assert!(text.contains("1 passed · 1 not run"));
    }

    #[test]
    fn build_failure_output_is_captured_from_build_output_events() {
        let index = index(&[(ENTRY_MODULE_ID, "never_runs")]);
        let events = vec![
            build_output_event("demo", "# demo\n"),
            build_output_event("demo", "./ops_test.go:3:5: undefined: foo\n"),
            event("fail", "demo", None, Some("FAIL\tdemo [build failed]\n")),
        ];
        let report = build_report(&index, &events, "demo");

        assert!(report.build_output.contains("undefined: foo"));
        assert!(!report.build_output.contains("[build failed]"));
        assert!(report.rows.iter().all(|r| r.status == Status::NotRun));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn build_failure_in_one_package_does_not_hide_panic_in_another() {
        let index = index(&[("a", "broken"), ("b", "crashes")]);
        let events = vec![
            build_output_event("demo/a", "./a_test.go:1:1: undefined: foo\n"),
            event(
                "output",
                "demo/a",
                None,
                Some("FAIL\tdemo/a [build failed]\n"),
            ),
            event("fail", "demo/a", None, None),
            event("output", "demo/b", None, Some("panic: boom in b\n")),
            event("fail", "demo/b", None, None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));

        assert!(report.build_output.contains("undefined: foo"));
        assert!(text.contains("panic: boom in b"));
        assert!(!text.contains("[build failed]"));
    }

    #[test]
    fn build_output_survives_a_per_test_failure_in_another_package() {
        let index = index(&[("a", "passes"), ("b", "never_runs")]);
        let events = vec![
            event("fail", "demo/a", Some("TestPasses"), None),
            build_output_event("demo/b", "./b_test.go:1:1: undefined: bar\n"),
        ];
        let report = build_report(&index, &events, "demo");

        assert!(report.build_output.contains("undefined: bar"));
    }

    #[test]
    fn started_test_without_terminal_is_aborted_and_shows_output() {
        let index = index(&[(ENTRY_MODULE_ID, "hangs")]);
        let events = vec![
            event("run", "demo", Some("TestHangs"), None),
            event(
                "output",
                "demo",
                Some("TestHangs"),
                Some("panic: test timed out\n"),
            ),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));

        assert!(text.contains("✗ hangs (aborted)"));
        assert!(text.contains("panic: test timed out"));
        assert!(text.contains("1 aborted"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn package_panic_before_any_test_shows_cause() {
        let index = index(&[(ENTRY_MODULE_ID, "one"), (ENTRY_MODULE_ID, "two")]);
        let events = vec![
            event("output", "demo", None, Some("panic: init blew up\n")),
            event("output", "demo", None, Some("goroutine 1 [running]:\n")),
            event("fail", "demo", None, None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));

        assert!(report.rows.iter().all(|r| r.status == Status::NotRun));
        assert!(text.contains("panic: init blew up"));
        assert!(text.contains("goroutine 1"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn package_panic_is_not_suppressed_by_another_packages_failure() {
        let index = index(&[("a", "fails"), ("b", "never")]);
        let events = vec![
            event("run", "demo/a", Some("TestFails"), None),
            event("fail", "demo/a", Some("TestFails"), None),
            event("output", "demo/b", None, Some("panic: boom in b\n")),
            event("fail", "demo/b", None, None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));

        assert!(text.contains("panic: boom in b"));
    }

    #[test]
    fn import_path_normalizes_test_binary_suffix() {
        assert_eq!(package_of_import_path("demo [demo.test]"), "demo");
        assert_eq!(package_of_import_path("a/b [a/b.test]"), "a/b");
        assert_eq!(package_of_import_path("demo"), "demo");
    }

    #[test]
    fn empty_index_reports_no_tests() {
        let report = build_report(&TestIndex::default(), &[], "demo");
        let text = render(&report, &no_sources(), false, Duration::from_millis(0));
        assert!(text.contains("No tests found"));
        assert_eq!(exit_code(&report.rows, true), 0);
    }

    #[test]
    fn failure_record_reassembles_and_attaches() {
        let index = index(&[(ENTRY_MODULE_ID, "parses")]);
        let inner = r#"{"file":7,"lo":3,"hi":9,"message":"test returned Err","operands":[{"label":"error","value":"boom"}]}"#;
        let events = vec![
            event("run", "demo", Some("TestParses"), None),
            attr_event("demo", "TestParses", &fail_value(inner)),
            event("fail", "demo", Some("TestParses"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let record = report.rows[0]
            .failure
            .as_ref()
            .expect("a lisette-fail record must attach to the failing test");
        assert_eq!(record.file, 7);
        assert_eq!((record.lo, record.hi), (3, 9));
        assert_eq!(record.operands[0].value, "boom");
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn failure_record_reassembles_from_out_of_order_chunks() {
        let index = index(&[(ENTRY_MODULE_ID, "big")]);
        let inner = r#"{"file":1,"lo":0,"hi":3,"message":"test returned Err","operands":[{"label":"error","value":"日本語"}]}"#;
        let hex = hex_encode(inner);
        let (first, second) = hex.split_at(hex.len() / 2);
        let events = vec![
            attr_event(
                "demo",
                "TestBig",
                &format!(r#"{{"i":1,"n":2,"d":"{second}"}}"#),
            ),
            attr_event(
                "demo",
                "TestBig",
                &format!(r#"{{"i":0,"n":2,"d":"{first}"}}"#),
            ),
            event("fail", "demo", Some("TestBig"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let record = report.rows[0]
            .failure
            .as_ref()
            .expect("two chunks must reassemble into one record");
        assert_eq!(record.operands[0].value, "日本語");
    }

    #[test]
    fn missing_chunk_yields_no_record() {
        let index = index(&[(ENTRY_MODULE_ID, "big")]);
        let events = vec![
            attr_event("demo", "TestBig", r#"{"i":0,"n":3,"d":"7b"}"#),
            attr_event("demo", "TestBig", r#"{"i":2,"n":3,"d":"7d"}"#),
            event("fail", "demo", Some("TestBig"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert!(
            report.rows[0].failure.is_none(),
            "an incomplete record must not produce a (truncated) diagnostic"
        );
    }

    #[test]
    fn failure_renders_spanned_block_when_source_known() {
        let index = index(&[(ENTRY_MODULE_ID, "parses")]);
        let inner = r#"{"file":7,"lo":3,"hi":9,"message":"test returned Err","operands":[{"label":"error","value":"boom"}]}"#;
        let events = vec![
            attr_event("demo", "TestParses", &fail_value(inner)),
            event("fail", "demo", Some("TestParses"), None),
        ];
        let report = build_report(&index, &events, "demo");

        let mut sources = no_sources();
        sources.insert(
            7,
            SourceInfo {
                source: "fn parses() {}\n".to_string(),
                filename: "x.test.lis".to_string(),
            },
        );
        let text = render(&report, &sources, false, Duration::from_millis(1));
        assert!(text.contains("test returned Err"), "got:\n{text}");
        assert!(text.contains("boom"), "got:\n{text}");
        assert!(text.contains("x.test.lis"), "got:\n{text}");
    }

    fn titled_index() -> TestIndex {
        let mut index = TestIndex::default();
        index.push(TestFunction {
            module_id: "csv".to_string(),
            qualified_name: "csv.splits_csv".to_string(),
            title: Some("splits and trims CSV fields".to_string()),
            doc: Some("Trims surrounding whitespace before splitting.".to_string()),
            span: span(),
        });
        index.push(TestFunction {
            module_id: "csv".to_string(),
            qualified_name: "csv.parses_number".to_string(),
            title: None,
            doc: None,
            span: span(),
        });
        index
    }

    #[test]
    fn title_replaces_name_and_doc_renders_as_description() {
        let report = build_report(&titled_index(), &[], "demo");
        let titled = report
            .rows
            .iter()
            .find(|r| r.name == "splits and trims CSV fields")
            .expect("title should replace the function name");
        assert_eq!(
            titled.description.as_deref(),
            Some("Trims surrounding whitespace before splitting.")
        );
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));
        assert!(
            text.contains("splits and trims CSV fields")
                && text.contains("Trims surrounding whitespace"),
            "got:\n{text}"
        );
    }

    #[test]
    fn filter_keeps_only_matching_rows_by_name_or_title() {
        let by_title = build_report_filtered(&titled_index(), &[], "demo", Some("and trims"));
        assert_eq!(by_title.rows.len(), 1);
        assert_eq!(by_title.rows[0].name, "splits and trims CSV fields");

        let by_name = build_report_filtered(&titled_index(), &[], "demo", Some("parses"));
        assert_eq!(by_name.rows.len(), 1);
        assert_eq!(by_name.rows[0].name, "parses_number");
    }

    #[test]
    fn matching_tests_is_case_sensitive_over_name_and_title_with_package() {
        let index = titled_index();
        assert_eq!(
            matching_tests(&index, "demo", "and trims"),
            vec![("demo/csv".to_string(), "TestSplitsCsv".to_string())]
        );
        assert_eq!(
            matching_tests(&index, "demo", "parses"),
            vec![("demo/csv".to_string(), "TestParsesNumber".to_string())]
        );
        assert!(
            matching_tests(&index, "demo", "Trims").is_empty(),
            "case-sensitive: `Trims` must not match the lowercase title term"
        );
        assert!(matching_tests(&index, "demo", "zzz").is_empty());
    }
}
