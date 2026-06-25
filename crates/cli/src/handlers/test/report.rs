use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use owo_colors::{OwoColorize, Style};
use semantics::store::ENTRY_MODULE_ID;
use serde::Deserialize;
use syntax::ast::Span;

use crate::go_cli::GoTestEvent;
use crate::output::{format_backticks, format_elapsed};
use diagnostics::LisetteDiagnostic;
use lisette::pipeline::{Sources, TestIndex};

/// Per (package, test): expected chunk count `n` and the gathered `(index, hex)` chunks.
type FailChunks = HashMap<(String, String), (usize, Vec<(usize, String)>)>;

struct EventTables {
    terminal: HashMap<(String, String), (Status, Option<f64>)>,
    started: HashSet<(String, String)>,
    outputs: HashMap<(String, String), String>,
    failures: HashMap<(String, String), FailureRecord>,
    skip_reasons: HashMap<(String, String), String>,
    subtest_names: HashMap<(String, String), String>,
    logs: HashMap<(String, String), Vec<LogRecord>>,
}

const FAIL_ATTR_KEY: &str = "lisette-fail";
const SKIP_ATTR_KEY: &str = "lisette-skip";
const SUBTEST_ATTR_KEY: &str = "lisette-subtest";
const LOG_ATTR_KEY: &str = "lisette-log";

const DESC_MAX_WIDTH: usize = 100;
const DESC_MIN_WIDTH: usize = 20;
const DESC_MAX_LINES: usize = 3;

const OPERAND_MAX_CHARS: usize = 160;
const OPERAND_MIN_CHARS: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Passed,
    Failed,
    Crashed,
    Skipped,
    Unreached,
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

#[derive(Deserialize, Clone)]
pub struct LogRecord {
    file: u32,
    lo: u32,
    hi: u32,
    value: String,
}

pub struct TestRow {
    pub package: String,
    pub go_name: String,
    pub name: String,
    pub description: Option<String>,
    pub status: Status,
    pub elapsed: Option<f64>,
    pub output: String,
    pub failure: Option<FailureRecord>,
    pub skip_reason: Option<String>,
    pub logs: Vec<LogRecord>,
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

pub fn all_test_keys(index: &TestIndex, go_module: &str) -> HashSet<(String, String)> {
    index
        .tests()
        .iter()
        .map(|test| {
            let prefix = format!("{}.", test.module_id);
            let fn_name = test
                .qualified_name
                .strip_prefix(&prefix)
                .unwrap_or(&test.qualified_name);
            let package = if test.module_id == ENTRY_MODULE_ID {
                go_module.to_string()
            } else {
                format!("{}/{}", go_module, test.module_id)
            };
            (package, go_test_name(fn_name))
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
    selected: Option<&HashSet<(String, String)>>,
) -> Report {
    let mut terminal: HashMap<(String, String), (Status, Option<f64>)> = HashMap::new();
    let mut started: HashSet<(String, String)> = HashSet::new();
    let mut outputs: HashMap<(String, String), String> = HashMap::new();
    let mut build_output = String::new();
    let mut package_output: HashMap<String, String> = HashMap::new();
    let mut failed_packages: HashSet<String> = HashSet::new();
    let mut build_failed_packages: HashSet<String> = HashSet::new();
    let mut fail_chunks: FailChunks = HashMap::new();
    let mut skip_reasons: HashMap<(String, String), String> = HashMap::new();
    let mut subtest_names: HashMap<(String, String), String> = HashMap::new();
    let mut logs: HashMap<(String, String), Vec<LogRecord>> = HashMap::new();
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
        if event.action == "attr"
            && event.key.as_deref() == Some(SKIP_ATTR_KEY)
            && let (Some(test), Some(value)) = (&event.test, &event.value)
            && let Some(reason) = decode_hex(value).and_then(|b| String::from_utf8(b).ok())
        {
            skip_reasons.insert((event.package.clone(), test.clone()), reason);
            continue;
        }
        if event.action == "attr"
            && event.key.as_deref() == Some(SUBTEST_ATTR_KEY)
            && let (Some(test), Some(value)) = (&event.test, &event.value)
            && let Some(name) = decode_hex(value).and_then(|b| String::from_utf8(b).ok())
        {
            subtest_names.insert((event.package.clone(), test.clone()), name);
            continue;
        }
        if event.action == "attr"
            && event.key.as_deref() == Some(LOG_ATTR_KEY)
            && let (Some(test), Some(value)) = (&event.test, &event.value)
            && let Some(record) =
                decode_hex(value).and_then(|b| serde_json::from_slice::<LogRecord>(&b).ok())
        {
            logs.entry((event.package.clone(), test.clone()))
                .or_default()
                .push(record);
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
            "skip" => {
                terminal.insert(key, (Status::Skipped, event.elapsed));
            }
            "output" => {
                if let Some(text) = &event.output {
                    outputs.entry(key).or_default().push_str(text);
                }
            }
            _ => {}
        }
    }

    let tables = EventTables {
        terminal,
        started,
        outputs,
        failures: reassemble_failures(fail_chunks),
        skip_reasons,
        subtest_names,
        logs,
    };

    let mut rows = Vec::new();
    for test in index.tests() {
        let prefix = format!("{}.", test.module_id);
        let fn_name = test
            .qualified_name
            .strip_prefix(&prefix)
            .unwrap_or(&test.qualified_name);
        let package = if test.module_id == ENTRY_MODULE_ID {
            go_module.to_string()
        } else {
            format!("{}/{}", go_module, test.module_id)
        };
        let go_name = go_test_name(fn_name);
        let key = (package.clone(), go_name.clone());
        if let Some(set) = selected
            && !set.contains(&key)
        {
            continue;
        }
        let (status, elapsed) = match tables.terminal.get(&key).copied() {
            Some(found) => found,
            None if tables.started.contains(&key) => (Status::Crashed, None),
            None => (Status::Unreached, None),
        };
        let children = collect_children(&package, &go_name, &tables);
        rows.push(TestRow {
            package,
            go_name: go_name.clone(),
            name: test.title.clone().unwrap_or_else(|| fn_name.to_string()),
            description: test.doc.clone(),
            status,
            elapsed,
            output: tables.outputs.get(&key).cloned().unwrap_or_default(),
            failure: tables.failures.get(&key).cloned(),
            skip_reason: tables.skip_reasons.get(&key).cloned(),
            logs: tables.logs.get(&key).cloned().unwrap_or_default(),
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

fn collect_children(package: &str, parent: &str, tables: &EventTables) -> Vec<TestRow> {
    let prefix = format!("{parent}/");
    let real: HashSet<&str> = tables
        .terminal
        .keys()
        .chain(tables.started.iter())
        .filter(|(pkg, name)| pkg == package && name.starts_with(&prefix))
        .map(|(_, name)| name.as_str())
        .chain(std::iter::once(parent))
        .collect();

    let mut children_of: HashMap<&str, Vec<&str>> = HashMap::new();
    for full in real.iter().copied().filter(|&full| full != parent) {
        if let Some(mother) = subtest_parent(package, full, &real, tables) {
            children_of.entry(mother).or_default().push(full);
        }
    }
    subtest_rows(package, parent, &children_of, tables)
}

fn subtest_rows(
    package: &str,
    parent: &str,
    children_of: &HashMap<&str, Vec<&str>>,
    tables: &EventTables,
) -> Vec<TestRow> {
    let mut children = children_of.get(parent).cloned().unwrap_or_default();
    children.sort_unstable();

    children
        .into_iter()
        .map(|full| {
            let key = (package.to_string(), full.to_string());
            let (status, elapsed) = match tables.terminal.get(&key).copied() {
                Some(found) => found,
                None if tables.started.contains(&key) => (Status::Crashed, None),
                None => (Status::Unreached, None),
            };
            let name = tables
                .subtest_names
                .get(&key)
                .filter(|original| !original.is_empty())
                .cloned()
                .unwrap_or_else(|| full[parent.len() + 1..].to_string());
            TestRow {
                package: package.to_string(),
                go_name: full.to_string(),
                name,
                description: None,
                status,
                elapsed,
                output: tables.outputs.get(&key).cloned().unwrap_or_default(),
                failure: tables.failures.get(&key).cloned(),
                skip_reason: tables.skip_reasons.get(&key).cloned(),
                logs: tables.logs.get(&key).cloned().unwrap_or_default(),
                children: subtest_rows(package, full, children_of, tables),
                span: Span::new(0, 0, 0),
            }
        })
        .collect()
}

fn subtest_parent<'a>(
    package: &str,
    full: &'a str,
    real: &HashSet<&'a str>,
    tables: &EventTables,
) -> Option<&'a str> {
    let key = (package.to_string(), full.to_string());
    if let Some(original) = tables.subtest_names.get(&key) {
        let own_segments = original.matches('/').count() + 1;
        if let Some(parent) = strip_trailing_segments(full, own_segments)
            && real.contains(parent)
        {
            return Some(parent);
        }
    }
    longest_real_prefix(full, real)
}

fn strip_trailing_segments(full: &str, count: usize) -> Option<&str> {
    let mut end = full.len();
    for _ in 0..count {
        end = full[..end].rfind('/')?;
    }
    Some(&full[..end])
}

fn longest_real_prefix<'a>(full: &'a str, real: &HashSet<&str>) -> Option<&'a str> {
    let mut best = None;
    for (i, ch) in full.char_indices() {
        if ch == '/' && real.contains(&full[..i]) {
            best = Some(&full[..i]);
        }
    }
    best
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

pub fn render(
    report: &Report,
    sources: &Sources,
    color: bool,
    total: Duration,
    term_width: usize,
) -> String {
    let mut out = String::from("\n");

    if report.rows.is_empty() {
        out.push_str("  No tests found\n");
        return out;
    }

    let mut by_package: BTreeMap<&str, Vec<&TestRow>> = BTreeMap::new();
    for row in &report.rows {
        by_package.entry(&row.package).or_default().push(row);
    }

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
        render_package_rows(&mut out, &group, sources, color, term_width);

        // Crash before any test ran (init/`TestMain` panic): cause is package-level only.
        if !report.build_failed_packages.contains(package)
            && report.failed_packages.contains(package)
            && group.iter().all(|r| r.status == Status::Unreached)
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

    render_logs(&mut out, &report.rows, &report.go_module, sources, color);
    render_failures(
        &mut out,
        &report.rows,
        &report.go_module,
        sources,
        color,
        term_width,
    );

    out.push('\n');
    out.push_str(&summary(&report.rows, total, color));
    if nothing_executed(&report.rows) {
        let note = format_backticks(
            "No tests ran. `go test` reported success but executed nothing.",
            color,
        );
        out.push_str(&format!("  {note}\n"));
    }
    out
}

fn render_package_rows(
    out: &mut String,
    rows: &[&TestRow],
    sources: &Sources,
    color: bool,
    term_width: usize,
) {
    let file_of = |row: &TestRow| {
        sources
            .get(&row.span.file_id)
            .map(|info| info.filename.as_str())
            .unwrap_or("")
    };

    for chunk in rows.chunk_by(|&a, &b| file_of(a) == file_of(b)) {
        let file = file_of(chunk[0]);
        let basename = file.rsplit('/').next().unwrap_or(file);
        if basename.is_empty() {
            render_rows(out, chunk, "    ", color, term_width);
        } else {
            out.push_str(&format!("    {basename}\n"));
            render_rows(out, chunk, "      ", color, term_width);
        }
    }
}

fn render_rows(out: &mut String, rows: &[&TestRow], prefix: &str, color: bool, term_width: usize) {
    for (i, row) in rows.iter().enumerate() {
        let last = i + 1 == rows.len();
        let branch = if last { "└── " } else { "├── " };
        let timing = match row.elapsed {
            Some(seconds) if seconds > 0.0 => {
                format!(" {}", format_elapsed(Duration::from_secs_f64(seconds)))
            }
            _ => String::new(),
        };
        let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });

        // A skip is the grouping's own act and is not carried by its children, so it still shows `#`.
        if row.children.is_empty() || row.status == Status::Skipped {
            let glyph = mark(row.status, color);
            let annotation = match row.status {
                Status::Skipped => row.skip_reason.clone(),
                Status::Crashed => crash_summary(&row.output),
                _ => None,
            };
            let base_width = prefix.chars().count()
                + branch.chars().count()
                + 2
                + row.name.chars().count()
                + timing.chars().count();
            let inline = annotation
                .as_ref()
                .filter(|reason| base_width + reason.chars().count() + 3 <= term_width);
            let suffix = match inline {
                Some(reason) => dim(&format!(" ({reason})"), color),
                None => String::new(),
            };
            out.push_str(&format!(
                "{prefix}{branch}{glyph} {}{suffix}{timing}\n",
                format_backticks(&row.name, color)
            ));
            if inline.is_none()
                && let Some(reason) = &annotation
            {
                let gutter = if row.children.is_empty() {
                    "  "
                } else {
                    "│ "
                };
                let indent = child_prefix.chars().count() + gutter.chars().count();
                let width = term_width
                    .saturating_sub(indent)
                    .clamp(DESC_MIN_WIDTH, DESC_MAX_WIDTH);
                let collapsed = reason.split_whitespace().collect::<Vec<_>>().join(" ");
                for wrapped in wrap_description(&collapsed, width, DESC_MAX_LINES) {
                    out.push_str(&dim(&format!("{child_prefix}{gutter}{wrapped}"), color));
                    out.push('\n');
                }
            }
        } else {
            out.push_str(&format!(
                "{prefix}{branch}{}{timing}\n",
                format_backticks(&row.name, color)
            ));
        }

        let children: Vec<&TestRow> = row.children.iter().collect();
        render_rows(out, &children, &child_prefix, color, term_width);
    }
}

fn render_logs(
    out: &mut String,
    rows: &[TestRow],
    go_module: &str,
    sources: &Sources,
    color: bool,
) {
    let multi_package = rows
        .iter()
        .map(|r| &r.package)
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let mut blocks: Vec<(String, String)> = Vec::new();
    for row in rows {
        let prefix = if multi_package {
            package_display(&row.package, go_module).to_string()
        } else {
            String::new()
        };
        collect_logs(row, &prefix, sources, color, &mut blocks);
    }
    if blocks.is_empty() {
        return;
    }

    out.push('\n');
    let heading = if color {
        "Logs".bold().to_string()
    } else {
        "Logs".to_string()
    };
    out.push_str(&format!("  {heading}\n"));
    let glyph = "≡";
    for (path, body) in blocks {
        out.push('\n');
        out.push_str(&format!("  {glyph} {}\n", path.replace('`', "")));
        for line in body.lines() {
            out.push_str(&format!("    {line}\n"));
        }
    }
}

fn collect_logs(
    row: &TestRow,
    prefix: &str,
    sources: &Sources,
    color: bool,
    blocks: &mut Vec<(String, String)>,
) {
    let path = if prefix.is_empty() {
        row.name.clone()
    } else {
        format!("{prefix} › {}", row.name)
    };
    for record in &row.logs {
        if let Some(body) = render_log(record, sources, color) {
            blocks.push((path.clone(), body));
        }
    }
    for child in &row.children {
        collect_logs(child, &path, sources, color, blocks);
    }
}

fn render_log(record: &LogRecord, sources: &Sources, color: bool) -> Option<String> {
    let info = sources.get(&record.file)?;
    let span = Span::new(record.file, record.lo, record.hi.saturating_sub(record.lo));
    let diagnostic = LisetteDiagnostic::info(String::new())
        .with_span_primary_label(&span, truncate_log_value(&record.value))
        .with_label_accent(Style::new());
    let rendered = diagnostics::render::render_to_string(
        &diagnostic,
        &info.source,
        &info.filename,
        color,
        Style::new(),
        2,
    );
    let body = rendered.lines().skip(1).collect::<Vec<_>>().join("\n");
    Some(dim_source_lines(&body, color))
}

fn dim_source_lines(frame: &str, color: bool) -> String {
    if !color {
        return frame.to_string();
    }
    frame
        .lines()
        .map(|line| match line.find('│') {
            Some(pos) => {
                let (head, code) = line.split_at(pos + '│'.len_utf8());
                format!("{head}{}", code.dimmed())
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_log_value(value: &str) -> String {
    if value.chars().count() <= OPERAND_MAX_CHARS {
        return value.to_string();
    }
    let head: String = value.chars().take(OPERAND_MAX_CHARS - 1).collect();
    format!("{head}…")
}

struct FailureBlock {
    status: Status,
    path: String,
    description: Option<String>,
    kind: Option<String>,
    body: String,
}

fn render_failures(
    out: &mut String,
    rows: &[TestRow],
    go_module: &str,
    sources: &Sources,
    color: bool,
    term_width: usize,
) {
    // The flat Failures section loses the tree's package grouping, so prefix the package when the run
    // spans more than one, to disambiguate same-named tests across packages.
    let multi_package = rows
        .iter()
        .map(|r| &r.package)
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let mut blocks: Vec<FailureBlock> = Vec::new();
    for row in rows {
        let prefix = if multi_package {
            package_display(&row.package, go_module).to_string()
        } else {
            String::new()
        };
        collect_failures(row, &prefix, sources, color, term_width, &mut blocks);
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
    for block in blocks {
        out.push('\n');
        let glyph = mark(block.status, color);
        let bare = block.path.replace('`', "");
        let name = match block.status {
            Status::Crashed => yellow(&bare, color),
            _ => red(&bare, color),
        };
        match block.kind {
            Some(kind) => out.push_str(&format!("  {glyph} {name} · {kind}\n")),
            None => out.push_str(&format!("  {glyph} {name}\n")),
        }
        if let Some(line) =
            failure_description_line(block.description.as_deref(), color, term_width)
        {
            out.push_str(&format!("    {line}\n"));
        }
        for line in block.body.lines() {
            out.push_str(&format!("    {line}\n"));
        }
    }
}

fn failure_description_line(
    description: Option<&str>,
    color: bool,
    term_width: usize,
) -> Option<String> {
    let collapsed = description?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let width = term_width
        .saturating_sub(4)
        .clamp(DESC_MIN_WIDTH, DESC_MAX_WIDTH);
    let line = wrap_description(&collapsed, width, 1).into_iter().next()?;
    Some(format_description(&line, color))
}

fn collect_failures(
    row: &TestRow,
    prefix: &str,
    sources: &Sources,
    color: bool,
    term_width: usize,
    blocks: &mut Vec<FailureBlock>,
) {
    let path = if prefix.is_empty() {
        row.name.clone()
    } else {
        format!("{prefix} › {}", row.name)
    };

    if matches!(row.status, Status::Failed | Status::Crashed) {
        if let Some((kind, body)) = row
            .failure
            .as_ref()
            .and_then(|record| render_failure(record, sources, color, term_width))
        {
            blocks.push(FailureBlock {
                status: row.status,
                path: path.clone(),
                description: row.description.clone(),
                kind: Some(kind),
                body,
            });
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
                blocks.push(FailureBlock {
                    status: row.status,
                    path: path.clone(),
                    description: row.description.clone(),
                    kind: None,
                    body: text,
                });
            }
        }
    }

    for child in &row.children {
        collect_failures(child, &path, sources, color, term_width, blocks);
    }
}

fn has_failing_descendant(row: &TestRow) -> bool {
    row.children.iter().any(|child| {
        matches!(child.status, Status::Failed | Status::Crashed) || has_failing_descendant(child)
    })
}

fn render_failure(
    record: &FailureRecord,
    sources: &Sources,
    color: bool,
    term_width: usize,
) -> Option<(String, String)> {
    let info = sources.get(&record.file)?;
    let span = Span::new(record.file, record.lo, record.hi.saturating_sub(record.lo));

    let budget = operand_budget(term_width);
    let anchor = match record.operands.as_slice() {
        [left, right] => first_divergence(&left.value, &right.value),
        _ => 0,
    };
    let values: Vec<String> = record
        .operands
        .iter()
        .map(|operand| truncate_operand(&operand.value, anchor, budget))
        .collect();

    let paired = matches!(record.kind.as_str(), "relation" | "labeled");
    let mut diagnostic = LisetteDiagnostic::error(record.message.clone());
    if paired {
        let label =
            paired_inline_label(&record.operands, &values, term_width).unwrap_or_else(|| {
                let label_width = record
                    .operands
                    .iter()
                    .map(|operand| operand.label.chars().count())
                    .max()
                    .unwrap_or(0)
                    + 1;
                record
                    .operands
                    .iter()
                    .zip(&values)
                    .map(|(operand, value)| {
                        let token = format!("{}:", operand.label);
                        format!("{token:<label_width$} {value}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        diagnostic = diagnostic.with_span_primary_label(&span, label);
    } else {
        let label = values
            .first()
            .cloned()
            .unwrap_or_else(|| record.message.clone());
        diagnostic = diagnostic.with_span_primary_label(&span, label);
        let notes: Vec<String> = record
            .operands
            .iter()
            .zip(&values)
            .skip(1)
            .map(|(operand, value)| format!("{}: {}", operand.label, value))
            .collect();
        if !notes.is_empty() {
            diagnostic = diagnostic.with_note(notes.join("\n"));
        }
    }
    let rendered = diagnostics::render::render_to_string(
        &diagnostic,
        &info.source,
        &info.filename,
        color,
        Style::new().red(),
        2,
    );
    let body = rendered.lines().skip(1).collect::<Vec<_>>().join("\n");
    Some((record.message.clone(), body))
}

fn count_status(rows: &[TestRow], status: Status) -> usize {
    rows.iter()
        .map(|r| {
            let own = (counts_in_own_right(r) && r.status == status) as usize;
            own + count_status(&r.children, status)
        })
        .sum()
}

fn counts_in_own_right(row: &TestRow) -> bool {
    row.children.is_empty()
        || row.status == Status::Skipped
        || (matches!(row.status, Status::Failed | Status::Crashed)
            && (row.failure.is_some() || !has_failing_descendant(row)))
}

fn summary(rows: &[TestRow], total: Duration, color: bool) -> String {
    let passed = count_status(rows, Status::Passed);
    let failed = count_status(rows, Status::Failed);
    let crashed = count_status(rows, Status::Crashed);
    let skipped = count_status(rows, Status::Skipped);
    let unreached = count_status(rows, Status::Unreached);

    let nothing_ran = passed == 0 && failed == 0 && crashed == 0 && skipped == 0 && unreached > 0;

    let glyph = mark(
        if failed > 0 || nothing_ran {
            Status::Failed
        } else if crashed > 0 {
            Status::Crashed
        } else {
            Status::Passed
        },
        color,
    );

    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(red(&format!("{failed} failed"), color));
    }
    if crashed > 0 {
        parts.push(yellow(&format!("{crashed} crashed"), color));
    }
    let no_other_counts = failed == 0 && crashed == 0 && skipped == 0 && unreached == 0;
    if passed > 0 || no_other_counts {
        parts.push(green(&format!("{passed} passed"), color));
    }
    if skipped > 0 {
        parts.push(blue(&format!("{skipped} skipped"), color));
    }
    if unreached > 0 {
        let text = format!("{unreached} unreached");
        parts.push(if nothing_ran { red(&text, color) } else { text });
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

fn blue(text: &str, color: bool) -> String {
    if color {
        text.blue().to_string()
    } else {
        text.to_string()
    }
}

fn yellow(text: &str, color: bool) -> String {
    if color {
        text.yellow().to_string()
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

fn operand_budget(term_width: usize) -> usize {
    // Reserve the gutter and frame plus one `label: ` prefix.
    let reserved = 41;
    term_width
        .saturating_sub(reserved)
        .clamp(OPERAND_MIN_CHARS, OPERAND_MAX_CHARS)
}

fn paired_inline_label(
    operands: &[Operand],
    values: &[String],
    term_width: usize,
) -> Option<String> {
    let is_scalar = |value: &str| !value.contains(['\n', '{', '[', '(']);
    if !operands.iter().all(|operand| is_scalar(&operand.value)) {
        return None;
    }
    let line = operands
        .iter()
        .zip(values)
        .map(|(operand, value)| format!("{}: {}", operand.label, value))
        .collect::<Vec<_>>()
        .join(" · ");
    (line.chars().count() <= operand_budget(term_width)).then_some(line)
}

fn first_divergence(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

fn truncate_operand(value: &str, anchor: usize, budget: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    let total = chars.len();
    if total <= budget {
        return value.to_string();
    }
    let budget = budget.max(8);
    let lead = budget / 3;
    let start = anchor.saturating_sub(lead).min(total - budget);
    let end = (start + budget).min(total);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < total {
        out.push('…');
    }
    out
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
        .any(|r| matches!(r.status, Status::Failed | Status::Crashed));
    if !run_success || any_failure || nothing_executed(rows) {
        1
    } else {
        0
    }
}

pub fn nothing_executed(rows: &[TestRow]) -> bool {
    let ran = count_status(rows, Status::Passed)
        + count_status(rows, Status::Failed)
        + count_status(rows, Status::Crashed)
        + count_status(rows, Status::Skipped);
    ran == 0 && count_status(rows, Status::Unreached) > 0
}

pub fn failed_keys(rows: &[TestRow]) -> Vec<(String, String)> {
    rows.iter()
        .filter(|r| matches!(r.status, Status::Failed | Status::Crashed))
        .map(|r| (r.package.clone(), r.go_name.clone()))
        .collect()
}

fn mark(status: Status, color: bool) -> String {
    let symbol = match status {
        Status::Passed => "✓",
        Status::Failed => "✕",
        Status::Crashed => "▲",
        Status::Skipped => "○",
        Status::Unreached => "⊘",
    };
    if !color {
        return symbol.to_string();
    }
    match status {
        Status::Passed => symbol.green().to_string(),
        Status::Failed => symbol.red().to_string(),
        Status::Crashed => symbol.yellow().to_string(),
        Status::Skipped => symbol.blue().to_string(),
        Status::Unreached => symbol.to_string(),
    }
}

/// The `panic:` headline from a crashed test's raw `go test` output, if any.
fn crash_summary(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("panic:"))
        .map(str::to_string)
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

    const TEST_WIDTH: usize = 100;

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

    fn skip_attr_event(package: &str, test: &str, reason: &str) -> GoTestEvent {
        GoTestEvent {
            action: "attr".to_string(),
            package: package.to_string(),
            test: Some(test.to_string()),
            elapsed: None,
            output: None,
            import_path: None,
            key: Some(SKIP_ATTR_KEY.to_string()),
            value: Some(hex_encode(reason)),
        }
    }

    fn subtest_attr_event(package: &str, test: &str, name: &str) -> GoTestEvent {
        GoTestEvent {
            action: "attr".to_string(),
            package: package.to_string(),
            test: Some(test.to_string()),
            elapsed: None,
            output: None,
            import_path: None,
            key: Some(SUBTEST_ATTR_KEY.to_string()),
            value: Some(hex_encode(name)),
        }
    }

    fn log_attr_event(package: &str, test: &str, file: u32, value: &str) -> GoTestEvent {
        let record = format!(r#"{{"file":{file},"lo":0,"hi":5,"value":{value:?}}}"#);
        GoTestEvent {
            action: "attr".to_string(),
            package: package.to_string(),
            test: Some(test.to_string()),
            elapsed: None,
            output: None,
            import_path: None,
            key: Some(LOG_ATTR_KEY.to_string()),
            value: Some(hex_encode(&record)),
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
    fn truncate_operand_leaves_short_values_untouched() {
        assert_eq!(truncate_operand("\"hello\"", 0, 40), "\"hello\"");
        let multibyte = "\"日本語\"";
        assert_eq!(truncate_operand(multibyte, 0, 40), multibyte);
    }

    #[test]
    fn truncate_operand_windows_around_the_anchor() {
        let value = format!("\"{}\"", "A".repeat(5000));
        let anchor = 5001;
        let shown = truncate_operand(&value, anchor, 40);

        assert!(
            shown.chars().count() <= 42,
            "got {} chars",
            shown.chars().count()
        );
        assert!(shown.starts_with('…'), "a cut head is marked, got: {shown}");
        assert!(
            shown.ends_with('"'),
            "the anchored tail stays visible, got: {shown}"
        );
    }

    #[test]
    fn first_divergence_finds_the_split_point() {
        assert_eq!(first_divergence("\"abc\"", "\"abX\""), 3);
        assert_eq!(first_divergence("\"abc\"", "\"abc\""), 5);
        assert_eq!(first_divergence("\"ab\"", "\"abc\""), 3);
    }

    #[test]
    fn large_operands_are_truncated_in_the_report() {
        let index = index(&[(ENTRY_MODULE_ID, "huge")]);
        let big = "A".repeat(20_000);
        let inner = serde_json::json!({
            "file": 7,
            "lo": 3,
            "hi": 9,
            "kind": "relation",
            "message": "assertion failed",
            "operands": [
                {"label": "left", "value": format!("\"{big}\"")},
                {"label": "right", "value": format!("\"{big}X\"")},
            ],
        })
        .to_string();
        let events = vec![
            attr_event("demo", "TestHuge", &fail_value(&inner)),
            event("fail", "demo", Some("TestHuge"), None),
        ];
        let report = build_report(&index, &events, "demo");

        let mut sources = no_sources();
        sources.insert(
            7,
            SourceInfo {
                source: "fn huge() {}\n".to_string(),
                filename: "x.test.lis".to_string(),
            },
        );
        let text = render(
            &report,
            &sources,
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        assert!(
            text.chars().count() < 2_000,
            "a 40k-char comparison must not dump in full, got {} chars",
            text.chars().count()
        );
        assert!(
            text.lines().all(|line| line.chars().count() < 200),
            "no rendered line may overflow the terminal"
        );
        assert!(text.contains('…'), "truncation is marked, got:\n{text}");
        assert!(
            text.contains("X\""),
            "the trailing divergence stays visible, got:\n{text}"
        );
        assert!(
            text.contains("left:") && text.contains("right:"),
            "got:\n{text}"
        );
    }

    #[test]
    fn paired_label_inlines_scalars_but_stacks_composites() {
        let render_relation = |left: &str, right: &str| {
            let index = index(&[(ENTRY_MODULE_ID, "cmp")]);
            let inner = serde_json::json!({
                "file": 7,
                "lo": 3,
                "hi": 9,
                "kind": "relation",
                "message": "expected ==",
                "operands": [
                    {"label": "left", "value": left},
                    {"label": "right", "value": right},
                ],
            })
            .to_string();
            let events = vec![
                attr_event("demo", "TestCmp", &fail_value(&inner)),
                event("fail", "demo", Some("TestCmp"), None),
            ];
            let report = build_report(&index, &events, "demo");
            let mut sources = no_sources();
            sources.insert(
                7,
                SourceInfo {
                    source: "fn cmp() {}\n".to_string(),
                    filename: "x.test.lis".to_string(),
                },
            );
            render(
                &report,
                &sources,
                false,
                Duration::from_millis(1),
                TEST_WIDTH,
            )
        };

        let scalar = render_relation("1", "2");
        assert!(
            scalar
                .lines()
                .any(|line| line.contains("left: 1") && line.contains("right: 2")),
            "short scalars share one line, got:\n{scalar}"
        );

        let composite = render_relation("Point { x: 1, y: 2 }", "Point { x: 1, y: 9 }");
        assert!(
            !composite
                .lines()
                .any(|line| line.contains("left:") && line.contains("right:")),
            "composite operands stay stacked, got:\n{composite}"
        );
    }

    #[test]
    fn all_pass_groups_by_package() {
        let index = index(&[(ENTRY_MODULE_ID, "root_smoke"), ("math", "adds_numbers")]);
        let events = vec![
            event("pass", "demo", Some("TestRootSmoke"), None),
            event("pass", "demo/math", Some("TestAddsNumbers"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(7),
            TEST_WIDTH,
        );

        assert!(text.contains("  demo\n"));
        assert!(text.contains("✓ root_smoke"));
        assert!(text.contains("  math\n"));
        assert!(text.contains("✓ adds_numbers"));
        assert!(text.contains("2 passed"));
        assert_eq!(exit_code(&report.rows, true), 0);
    }

    #[test]
    fn nothing_executed_fails_even_when_go_succeeds() {
        let index = index(&[(ENTRY_MODULE_ID, "alpha"), (ENTRY_MODULE_ID, "beta")]);
        let report = build_report(&index, &[], "demo");
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(0),
            TEST_WIDTH,
        );

        assert!(text.contains("2 unreached"));
        assert!(
            text.contains('✕') && !text.contains('✓'),
            "an all-unreached run must not read as a green pass, got:\n{text}"
        );
        assert!(
            text.contains("No tests ran"),
            "a note explains the empty run, got:\n{text}"
        );
        assert!(nothing_executed(&report.rows));
        assert_eq!(
            exit_code(&report.rows, true),
            1,
            "a run that executed nothing must exit non-zero even when `go test` succeeded"
        );
    }

    #[test]
    fn logged_values_render_in_a_logs_section_for_a_passing_test() {
        let index = index(&[(ENTRY_MODULE_ID, "inspects")]);
        let events = vec![
            log_attr_event("demo", "TestInspects", 7, "42"),
            event("pass", "demo", Some("TestInspects"), None),
        ];
        let report = build_report(&index, &events, "demo");

        let mut sources = no_sources();
        sources.insert(
            7,
            SourceInfo {
                source: "fn inspects() {\n  t.log(count)\n}\n".to_string(),
                filename: "x.test.lis".to_string(),
            },
        );
        let text = render(
            &report,
            &sources,
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        assert!(text.contains("Logs"), "a Logs section appears:\n{text}");
        assert!(
            text.contains("inspects"),
            "the logging test is named:\n{text}"
        );
        assert!(text.contains("42"), "the logged value is shown:\n{text}");
        assert!(text.contains("1 passed"));
        assert_eq!(exit_code(&report.rows, true), 0);
    }

    #[test]
    fn every_module_groups_its_tests_under_filenames() {
        let mut index = TestIndex::default();
        for (name, file) in [("adds", 1u32), ("subtracts", 2u32)] {
            index.push(TestFunction {
                module_id: "math".to_string(),
                qualified_name: format!("math.{name}"),
                title: None,
                doc: None,
                span: Span::new(file, 0, 1),
            });
        }
        index.push(TestFunction {
            module_id: "io".to_string(),
            qualified_name: "io.reads".to_string(),
            title: None,
            doc: None,
            span: Span::new(3, 0, 1),
        });
        let events = vec![
            event("pass", "demo/math", Some("TestAdds"), None),
            event("pass", "demo/math", Some("TestSubtracts"), None),
            event("pass", "demo/io", Some("TestReads"), None),
        ];
        let report = build_report(&index, &events, "demo");

        let mut sources = no_sources();
        for (id, path) in [
            (1u32, "src/math/add.test.lis"),
            (2, "src/math/sub.test.lis"),
            (3, "src/io/io.test.lis"),
        ] {
            sources.insert(
                id,
                SourceInfo {
                    source: String::new(),
                    filename: path.to_string(),
                },
            );
        }
        let text = render(
            &report,
            &sources,
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        assert!(
            text.contains("add.test.lis") && text.contains("sub.test.lis"),
            "a split module shows a header per file:\n{text}"
        );
        assert!(
            text.contains("io.test.lis"),
            "a single-file module also shows its file header:\n{text}"
        );
        assert!(
            !text.contains("src/math/add.test.lis"),
            "the file header is the basename, not the full path:\n{text}"
        );
        let io_header = text.lines().position(|l| l.contains("io.test.lis"));
        let reads = text.lines().position(|l| l.contains("✓ reads"));
        assert!(
            io_header < reads,
            "tests sit under their file header:\n{text}"
        );
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

        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
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
    fn summary_counts_subtests_not_their_parent() {
        let index = index(&[(ENTRY_MODULE_ID, "parent")]);
        let events = vec![
            event("run", "demo", Some("TestParent"), None),
            event("run", "demo", Some("TestParent/alpha"), None),
            event("pass", "demo", Some("TestParent/alpha"), None),
            event("run", "demo", Some("TestParent/beta"), None),
            event("pass", "demo", Some("TestParent/beta"), None),
            event("pass", "demo", Some("TestParent"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(
            text.contains("2 passed"),
            "two passing subtests count as two, not one parent, got:\n{text}"
        );
    }

    #[test]
    fn skipped_test_shows_reason_and_is_not_a_failure() {
        let index = index(&[(ENTRY_MODULE_ID, "wip")]);
        let events = vec![
            event("run", "demo", Some("TestWip"), None),
            skip_attr_event("demo", "TestWip", "not ready"),
            event("skip", "demo", Some("TestWip"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert_eq!(report.rows[0].status, Status::Skipped);
        assert_eq!(report.rows[0].skip_reason.as_deref(), Some("not ready"));

        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(text.contains("○ wip (not ready)"), "got:\n{text}");
        assert!(text.contains("1 skipped"));
        assert_eq!(exit_code(&report.rows, true), 0, "a skip is not a failure");
    }

    #[test]
    fn long_skip_reason_wraps_instead_of_overflowing() {
        let index = index(&[(ENTRY_MODULE_ID, "wip")]);
        let reason = "this reason is far too long to sit inline on the row without \
                      running well past the edge of any reasonable terminal width";
        let events = vec![
            event("run", "demo", Some("TestWip"), None),
            skip_attr_event("demo", "TestWip", reason),
            event("skip", "demo", Some("TestWip"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        let row_line = text
            .lines()
            .find(|line| line.contains("○ wip"))
            .expect("the skipped row must render");
        assert!(
            !row_line.contains("this reason"),
            "a long reason must not sit inline, got: {row_line}"
        );
        assert!(
            text.contains("this reason is far too long"),
            "the wrapped reason must still appear, got:\n{text}"
        );
        assert!(
            text.lines()
                .all(|line| line.chars().count() <= DESC_MAX_WIDTH),
            "no wrapped line may overflow, got:\n{text}"
        );
    }

    #[test]
    fn skip_reason_layout_follows_passed_width_not_terminal() {
        let index = index(&[(ENTRY_MODULE_ID, "wip")]);
        let reason = "this reason is far too long to sit inline on the row without \
                      running well past the edge of any reasonable terminal width";
        let events = vec![
            event("run", "demo", Some("TestWip"), None),
            skip_attr_event("demo", "TestWip", reason),
            event("skip", "demo", Some("TestWip"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let row_with_width = |width| {
            render(
                &report,
                &no_sources(),
                false,
                Duration::from_millis(1),
                width,
            )
            .lines()
            .find(|line| line.contains("○ wip"))
            .expect("the skipped row must render")
            .to_string()
        };

        assert!(
            !row_with_width(60).contains("this reason"),
            "a narrow width must wrap the reason off the row"
        );
        assert!(
            row_with_width(300).contains("this reason"),
            "a wide width must keep the reason inline; render must honor the passed width, not the live terminal"
        );
    }

    #[test]
    fn skipped_subtest_renders_under_parent_with_reason() {
        let index = index(&[(ENTRY_MODULE_ID, "parent")]);
        let events = vec![
            event("run", "demo", Some("TestParent"), None),
            event("run", "demo", Some("TestParent/child"), None),
            skip_attr_event("demo", "TestParent/child", "needs net"),
            event("skip", "demo", Some("TestParent/child"), None),
            event("pass", "demo", Some("TestParent"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let child = &report.rows[0].children[0];
        assert_eq!(child.status, Status::Skipped);
        assert_eq!(child.skip_reason.as_deref(), Some("needs net"));

        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(text.contains("○ child (needs net)"), "got:\n{text}");
        assert!(
            text.contains("1 skipped"),
            "a skipped subtest must count in the footer, got:\n{text}"
        );
    }

    #[test]
    fn skipped_parent_with_children_keeps_its_marker() {
        let index = index(&[(ENTRY_MODULE_ID, "parent")]);
        let events = vec![
            event("run", "demo", Some("TestParent"), None),
            event("run", "demo", Some("TestParent/child"), None),
            event("pass", "demo", Some("TestParent/child"), None),
            skip_attr_event("demo", "TestParent", "rest not ready"),
            event("skip", "demo", Some("TestParent"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert_eq!(report.rows[0].status, Status::Skipped);
        assert!(!report.rows[0].children.is_empty());

        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(
            text.contains("○ parent (rest not ready)"),
            "a skipped grouping keeps its marker and reason, got:\n{text}"
        );
        assert!(
            text.contains("✓ child"),
            "its child still renders, got:\n{text}"
        );
        assert!(
            text.contains("1 passed") && text.contains("1 skipped"),
            "the passing child and the skipped grouping both count, got:\n{text}"
        );
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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        let tree = text.split("Failures").next().unwrap_or(&text);
        assert!(
            !tree.contains("✕ parent") && !tree.contains("✓ parent"),
            "a grouping carries no sigil in the tree, got:\n{text}"
        );
        assert!(text.contains("✓ child"));
        assert!(
            text.contains("parent panicked"),
            "a failed parent's own output must show even with subtests, got:\n{text}"
        );
        assert!(
            text.contains("1 failed") && text.contains("1 passed"),
            "the failed grouping and its passing child both count, got:\n{text}"
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

        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(text.contains("boom"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn subtest_shows_source_name_not_go_munged_name() {
        let index = index(&[(ENTRY_MODULE_ID, "names")]);
        let events = vec![
            event("run", "demo", Some("TestNames"), None),
            event("run", "demo", Some("TestNames/hello_world_here"), None),
            subtest_attr_event("demo", "TestNames/hello_world_here", "hello world here"),
            event("fail", "demo", Some("TestNames/hello_world_here"), None),
            event("fail", "demo", Some("TestNames"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert_eq!(report.rows[0].children[0].name, "hello world here");
    }

    #[test]
    fn subtest_name_with_slashes_stays_one_leaf() {
        let index = index(&[(ENTRY_MODULE_ID, "names")]);
        let events = vec![
            event("run", "demo", Some("TestNames"), None),
            event("run", "demo", Some("TestNames/path/to/thing"), None),
            subtest_attr_event("demo", "TestNames/path/to/thing", "path/to/thing"),
            event("fail", "demo", Some("TestNames/path/to/thing"), None),
            event("fail", "demo", Some("TestNames"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let children = &report.rows[0].children;
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "path/to/thing");
        assert!(children[0].children.is_empty());
    }

    #[test]
    fn sibling_subtest_named_like_a_path_prefix_stays_a_sibling() {
        let index = index(&[(ENTRY_MODULE_ID, "names")]);
        let events = vec![
            event("run", "demo", Some("TestNames"), None),
            event("run", "demo", Some("TestNames/path"), None),
            subtest_attr_event("demo", "TestNames/path", "path"),
            event("fail", "demo", Some("TestNames/path"), None),
            event("run", "demo", Some("TestNames/path/to"), None),
            subtest_attr_event("demo", "TestNames/path/to", "path/to"),
            event("fail", "demo", Some("TestNames/path/to"), None),
            event("fail", "demo", Some("TestNames"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let names: Vec<&str> = report.rows[0]
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, vec!["path", "path/to"]);
        assert!(
            report.rows[0]
                .children
                .iter()
                .all(|c| c.children.is_empty()),
            "a name that prefixes a sibling must not adopt it as a child"
        );
    }

    #[test]
    fn duplicate_subtest_names_both_show_the_source_name() {
        let index = index(&[(ENTRY_MODULE_ID, "dupes")]);
        let events = vec![
            event("run", "demo", Some("TestDupes"), None),
            event("run", "demo", Some("TestDupes/dup"), None),
            subtest_attr_event("demo", "TestDupes/dup", "dup"),
            event("pass", "demo", Some("TestDupes/dup"), None),
            event("run", "demo", Some("TestDupes/dup#01"), None),
            subtest_attr_event("demo", "TestDupes/dup#01", "dup"),
            event("pass", "demo", Some("TestDupes/dup#01"), None),
            event("pass", "demo", Some("TestDupes"), None),
        ];
        let report = build_report(&index, &events, "demo");
        let names: Vec<&str> = report.rows[0]
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, vec!["dup", "dup"]);
    }

    #[test]
    fn subtest_without_source_name_falls_back_to_go_segment() {
        let index = index(&[(ENTRY_MODULE_ID, "names")]);
        let events = vec![
            event("run", "demo", Some("TestNames"), None),
            event("run", "demo", Some("TestNames/#00"), None),
            subtest_attr_event("demo", "TestNames/#00", ""),
            event("fail", "demo", Some("TestNames/#00"), None),
            event("fail", "demo", Some("TestNames"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert_eq!(report.rows[0].children[0].name, "#00");
    }

    #[test]
    fn failed_test_shows_output_and_counts() {
        let index = index(&[(ENTRY_MODULE_ID, "boom")]);
        let events = vec![
            event("fail", "demo", Some("TestBoom"), None),
            event("output", "demo", Some("TestBoom"), Some("panic: boom\n")),
        ];
        let report = build_report(&index, &events, "demo");
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(2),
            TEST_WIDTH,
        );

        assert!(text.contains("✕ boom"));
        assert!(text.contains("panic: boom"));
        assert!(text.contains("1 failed"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn declared_test_with_no_event_is_not_run() {
        let index = index(&[(ENTRY_MODULE_ID, "ghost")]);
        let report = build_report(&index, &[], "demo");
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        assert!(text.contains("⊘ ghost"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn filtered_run_does_not_fail_when_go_succeeds() {
        let index = index(&[(ENTRY_MODULE_ID, "kept"), (ENTRY_MODULE_ID, "filtered")]);
        let events = vec![event("pass", "demo", Some("TestKept"), None)];
        let report = build_report(&index, &events, "demo");

        assert_eq!(exit_code(&report.rows, true), 0);
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(text.contains("⊘ filtered"));
        assert!(text.contains("1 passed · 1 unreached"));
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
        assert!(report.rows.iter().all(|r| r.status == Status::Unreached));
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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

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
    fn started_test_without_terminal_is_crashed_and_shows_output() {
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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        assert!(text.contains("▲ hangs"));
        assert!(text.contains("panic: test timed out"));
        assert!(text.contains("1 crashed"));
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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

        assert!(report.rows.iter().all(|r| r.status == Status::Unreached));
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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );

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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(0),
            TEST_WIDTH,
        );
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
        let text = render(
            &report,
            &sources,
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(text.contains("test returned Err"), "got:\n{text}");
        assert!(text.contains("boom"), "got:\n{text}");
        assert!(text.contains("x.test.lis"), "got:\n{text}");
    }

    #[test]
    fn failure_block_shows_test_description() {
        let mut index = TestIndex::default();
        index.push(TestFunction {
            module_id: ENTRY_MODULE_ID.to_string(),
            qualified_name: format!("{ENTRY_MODULE_ID}.multiplies"),
            title: None,
            doc: Some(
                "Guards the multiplication that downstream calculations rely on.".to_string(),
            ),
            span: span(),
        });
        let inner = r#"{"file":7,"lo":3,"hi":9,"message":"assertion failed","operands":[{"label":"left","value":"42"},{"label":"right","value":"43"}]}"#;
        let events = vec![
            attr_event("demo", "TestMultiplies", &fail_value(inner)),
            event("fail", "demo", Some("TestMultiplies"), None),
        ];
        let report = build_report(&index, &events, "demo");

        let mut sources = no_sources();
        sources.insert(
            7,
            SourceInfo {
                source: "fn multiplies() {}\n".to_string(),
                filename: "x.test.lis".to_string(),
            },
        );
        let text = render(
            &report,
            &sources,
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        let failures = text
            .split("Failures")
            .nth(1)
            .expect("a Failures section should render");
        assert!(
            failures.contains("Guards the multiplication that downstream calculations rely on."),
            "the failure block should show the test description, got:\n{text}"
        );
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
    fn title_replaces_name_and_description_stays_off_the_tree() {
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
        let text = render(
            &report,
            &no_sources(),
            false,
            Duration::from_millis(1),
            TEST_WIDTH,
        );
        assert!(
            text.contains("splits and trims CSV fields"),
            "the title shows in the tree, got:\n{text}"
        );
        assert!(
            !text.contains("Trims surrounding whitespace"),
            "the description must not render in the tree, got:\n{text}"
        );
    }

    #[test]
    fn selected_set_keeps_only_those_rows() {
        let only_splits: HashSet<(String, String)> =
            [("demo/csv".to_string(), "TestSplitsCsv".to_string())]
                .into_iter()
                .collect();
        let report = build_report_filtered(&titled_index(), &[], "demo", Some(&only_splits));
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].name, "splits and trims CSV fields");
        assert_eq!(report.rows[0].go_name, "TestSplitsCsv");
    }

    #[test]
    fn failed_keys_collects_failed_top_level_tests() {
        let index = index(&[(ENTRY_MODULE_ID, "good"), (ENTRY_MODULE_ID, "bad")]);
        let events = vec![
            event("pass", "demo", Some("TestGood"), None),
            event("fail", "demo", Some("TestBad"), None),
        ];
        let report = build_report(&index, &events, "demo");
        assert_eq!(
            failed_keys(&report.rows),
            vec![("demo".to_string(), "TestBad".to_string())]
        );
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
