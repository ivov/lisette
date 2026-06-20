use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use owo_colors::OwoColorize;
use semantics::store::ENTRY_MODULE_ID;
use serde::Deserialize;
use syntax::ast::Span;

use crate::go_cli::GoTestEvent;
use crate::output::format_elapsed;
use diagnostics::LisetteDiagnostic;
use lisette::pipeline::{Sources, TestIndex};

/// Per (package, test): expected chunk count `n` and the gathered `(index, hex)` chunks.
type FailChunks = HashMap<(String, String), (usize, Vec<(usize, String)>)>;

const FAIL_ATTR_KEY: &str = "lisette-fail";

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
    #[serde(default)]
    lo: u32,
    #[serde(default)]
    hi: u32,
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
    pub status: Status,
    pub elapsed: Option<f64>,
    pub output: String,
    pub failure: Option<FailureRecord>,
    pub children: Vec<TestRow>,
}

pub struct Report {
    pub rows: Vec<TestRow>,
    pub build_output: String,
    package_output: HashMap<String, String>,
    failed_packages: HashSet<String>,
    build_failed_packages: HashSet<String>,
}

pub fn build_report(index: &TestIndex, events: &[GoTestEvent], go_module: &str) -> Report {
    let mut terminal: HashMap<(String, String), (Status, Option<f64>)> = HashMap::new();
    let mut started: HashSet<(String, String)> = HashSet::new();
    let mut outputs: HashMap<(String, String), String> = HashMap::new();
    let mut build_output = String::new();
    let mut package_output: HashMap<String, String> = HashMap::new();
    let mut failed_packages: HashSet<String> = HashSet::new();
    let mut build_failed_packages: HashSet<String> = HashSet::new();
    let mut fail_chunks: FailChunks = HashMap::new();

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
                "fail" => {
                    failed_packages.insert(event.package.clone());
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
            name: fn_name.to_string(),
            status,
            elapsed,
            output: outputs.get(&key).cloned().unwrap_or_default(),
            failure: failures.get(&key).cloned(),
            children,
        });
    }
    Report {
        rows,
        build_output,
        package_output,
        failed_packages,
        build_failed_packages,
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
    let mut direct: Vec<&String> = terminal
        .keys()
        .chain(started.iter())
        .filter(|(pkg, name)| {
            pkg == package && name.starts_with(&prefix) && !name[prefix.len()..].contains('/')
        })
        .map(|(_, name)| name)
        .collect();
    direct.sort();
    direct.dedup();

    direct
        .into_iter()
        .map(|full| {
            let key = (package.to_string(), full.clone());
            let (status, elapsed) = match terminal.get(&key).copied() {
                Some(found) => found,
                None if started.contains(&key) => (Status::Aborted, None),
                None => (Status::NotRun, None),
            };
            TestRow {
                package: package.to_string(),
                name: full[prefix.len()..].to_string(),
                status,
                elapsed,
                output: outputs.get(&key).cloned().unwrap_or_default(),
                failure: failures.get(&key).cloned(),
                children: collect_children(package, full, terminal, started, outputs, failures),
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

    for (package, mut group) in by_package {
        group.sort_by(|a, b| a.name.cmp(&b.name));
        out.push_str(&format!("  {package}\n"));
        render_rows(&mut out, &group, "    ", sources, color);

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

    out.push('\n');
    out.push_str(&summary(&report.rows, total));
    out
}

fn render_rows(out: &mut String, rows: &[&TestRow], prefix: &str, sources: &Sources, color: bool) {
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
            let suffix = match row.status {
                Status::NotRun => " (not run)",
                Status::Aborted => " (aborted)",
                _ => "",
            };
            out.push_str(&format!(
                "{prefix}{branch}{} {}{suffix}{timing}\n",
                mark(row.status, color),
                row.name
            ));
        } else {
            out.push_str(&format!("{prefix}{branch}{}{timing}\n", row.name));
        }

        let child_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });

        if let Some(block) = row
            .failure
            .as_ref()
            .and_then(|record| render_failure(record, sources, color))
        {
            for line in block.lines() {
                out.push_str(&format!("{child_prefix}{line}\n"));
            }
        } else if matches!(row.status, Status::Failed | Status::Aborted) {
            for line in row.output.lines() {
                let line = line.trim_end();
                if line.is_empty() {
                    continue;
                }
                out.push_str(&dim(&format!("{child_prefix}    {line}"), color));
                out.push('\n');
            }
        }

        let children: Vec<&TestRow> = row.children.iter().collect();
        render_rows(out, &children, &child_prefix, sources, color);
    }
}

fn render_failure(record: &FailureRecord, sources: &Sources, color: bool) -> Option<String> {
    let info = sources.get(&record.file)?;
    let span = Span::new(record.file, record.lo, record.hi.saturating_sub(record.lo));

    let (label, notes): (String, Vec<String>) = if record.kind == "labeled" {
        let notes = record
            .operands
            .iter()
            .map(|operand| {
                let source = info
                    .source
                    .get(operand.lo as usize..operand.hi as usize)
                    .unwrap_or(&operand.label);
                format!("{}: `{}` is `{}`", operand.label, source, operand.value)
            })
            .collect();
        (record.message.clone(), notes)
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
    Some(diagnostics::render::render_to_string(
        &diagnostic,
        &info.source,
        &info.filename,
        color,
    ))
}

fn summary(rows: &[TestRow], total: Duration) -> String {
    let passed = rows.iter().filter(|r| r.status == Status::Passed).count();
    let failed = rows.iter().filter(|r| r.status == Status::Failed).count();
    let aborted = rows.iter().filter(|r| r.status == Status::Aborted).count();
    let not_run = rows.iter().filter(|r| r.status == Status::NotRun).count();

    let mut parts = vec![format!("{passed} passed")];
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if aborted > 0 {
        parts.push(format!("{aborted} aborted"));
    }
    if not_run > 0 {
        parts.push(format!("{not_run} not run"));
    }
    format!("  {} {}\n", parts.join(", "), format_elapsed(total))
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
        Status::NotRun => "·",
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
        assert!(text.contains("  demo/math\n"));
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

        assert!(
            !text.contains("✗ parent") && !text.contains("✓ parent"),
            "a grouping carries no sigil, got:\n{text}"
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

        assert!(text.contains("· ghost (not run)"));
        assert_eq!(exit_code(&report.rows, false), 1);
    }

    #[test]
    fn filtered_run_does_not_fail_when_go_succeeds() {
        let index = index(&[(ENTRY_MODULE_ID, "kept"), (ENTRY_MODULE_ID, "filtered")]);
        let events = vec![event("pass", "demo", Some("TestKept"), None)];
        let report = build_report(&index, &events, "demo");

        assert_eq!(exit_code(&report.rows, true), 0);
        let text = render(&report, &no_sources(), false, Duration::from_millis(1));
        assert!(text.contains("· filtered (not run)"));
        assert!(text.contains("1 passed, 1 not run"));
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
}
