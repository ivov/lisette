use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use emit::{Emitter, TestEmitConfig};
use syntax::program::File;

use crate::_harness::pipeline::TestPipeline;

const PRELUDE_IMPORT_PATH: &str = "github.com/ivov/lisette/prelude";
const GO_MODULE: &str = "lisette/e2e_suite_tests";

pub struct EmittedTest {
    pub go_code: String,
    pub entry: Option<EntryPoint>,
}

#[derive(Copy, Clone)]
pub enum EntryPoint {
    TestWrapper,
    Test,
    Main,
}

impl EntryPoint {
    pub fn as_go_name(self) -> &'static str {
        match self {
            EntryPoint::TestWrapper => "__test__",
            EntryPoint::Test => "test",
            EntryPoint::Main => "main",
        }
    }
}

pub fn compile_e2e_suite_test(input: &str, package_name: &str) -> Result<EmittedTest, String> {
    let pipeline = TestPipeline::new(input).wrapped().e2e_suite_mode();
    let compiled = pipeline.compile();
    let result = compiled.run_inference();

    if !result.errors.is_empty() {
        return Err(format!(
            "type inference failed: {} error(s); first: {:?}",
            result.errors.len(),
            result.errors.first()
        ));
    }

    let file = File {
        id: 0,
        module_id: result.module_id.clone(),
        name: "test.lis".to_string(),
        source: String::new(),
        items: result.ast,
    };

    let config = TestEmitConfig {
        definitions: &result.definitions,
        module_id: &result.module_id,
        go_module: GO_MODULE,
        unused: &result.unused,
        mutations: &result.mutations,
        ufcs_methods: &result.ufcs_methods,
        go_package_names: &result.go_package_names,
    };
    let mut emitter = Emitter::new_for_tests(&config, None);
    let mut emitted_files = emitter.emit_files(&[&file], &result.module_id);

    if emitted_files.is_empty() {
        return Err("emitter produced no files".to_string());
    }

    let mut output = emitted_files.remove(0);
    output.package_name = package_name.to_string();

    let go_code = output.to_go();
    let entry = detect_entry(&go_code);

    Ok(EmittedTest { go_code, entry })
}

pub fn detect_entry(go_code: &str) -> Option<EntryPoint> {
    if go_code.contains("func __test__()") {
        Some(EntryPoint::TestWrapper)
    } else if go_code.contains("func test()") {
        Some(EntryPoint::Test)
    } else if go_code.contains("func main()") {
        Some(EntryPoint::Main)
    } else {
        None
    }
}

pub fn extract_imports(go_code: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut in_block = false;
    for line in go_code.lines() {
        let line = line.trim();
        if line.starts_with("import (") {
            in_block = true;
            continue;
        }
        if in_block {
            if line == ")" {
                in_block = false;
                continue;
            }
            if let Some(path) = parse_import_line(line) {
                imports.push(path);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("import ")
            && let Some(path) = parse_import_line(rest)
        {
            imports.push(path);
        }
    }
    imports
}

fn parse_import_line(s: &str) -> Option<String> {
    let s = s.trim();
    let quote_start = s.find('"')?;
    let after = &s[quote_start + 1..];
    let quote_end = after.find('"')?;
    Some(after[..quote_end].to_string())
}

pub fn skip_reason_for_imports(go_code: &str) -> Option<String> {
    for path in extract_imports(go_code) {
        if path == PRELUDE_IMPORT_PATH {
            continue;
        }
        let first = path.split('/').next().unwrap_or("");
        if first.contains('.') {
            return Some(format!("non-stdlib import: {path}"));
        }
    }
    None
}

pub struct HarvestedTest {
    pub name: String,
    pub input: String,
    pub snap_body: String,
}

pub fn harvest_snapshots(snapshots_dir: &Path) -> Vec<HarvestedTest> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(snapshots_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("snap") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(input) = parse_description_input(&content) else {
            continue;
        };
        let snap_body = parse_snap_body(&content).unwrap_or_default();
        out.push(HarvestedTest {
            name: stem.to_string(),
            input,
            snap_body,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// Limitation: assumes the snap body itself does not contain a `---` separator.
// Insta-emitted snaps for Go code do not, so this is safe for our use.
fn parse_snap_body(snap: &str) -> Option<String> {
    let mut parts = snap.splitn(3, "---");
    parts.next()?;
    parts.next()?;
    Some(parts.next()?.trim_start_matches('\n').to_string())
}

fn parse_description_input(snap: &str) -> Option<String> {
    let header = snap.split("---").nth(1)?;
    for line in header.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("description:") else {
            continue;
        };
        let rest = rest.trim();
        let rest = rest.strip_prefix('"')?;
        let rest = rest.strip_suffix('"')?;
        let unescaped = unescape_yaml(rest);
        let input = unescaped.strip_prefix("input: ")?;
        return Some(input.to_string());
    }
    None
}

fn unescape_yaml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('0') => out.push('\0'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub fn write_subpackage(
    target_dir: &Path,
    name: &str,
    go_code: &str,
    entry: EntryPoint,
) -> std::io::Result<()> {
    let pkg_dir = target_dir.join(format!("test_{name}"));
    fs::create_dir_all(&pkg_dir)?;
    fs::write(pkg_dir.join("test.go"), go_code)?;

    let entry_name = entry.as_go_name();
    let runner = format!(
        "package test_{name}\n\nimport \"testing\"\n\nfunc TestRun(t *testing.T) {{\n\tdefer func() {{\n\t\tif r := recover(); r != nil {{\n\t\t\tt.Fatalf(\"panic: %v\", r)\n\t\t}}\n\t}}()\n\t{entry_name}()\n}}\n"
    );
    fs::write(pkg_dir.join("test_test.go"), runner)?;
    Ok(())
}

pub fn write_go_mod(
    target_dir: &Path,
    prelude_path: &Path,
    go_version: &str,
) -> std::io::Result<()> {
    let abs = prelude_path.canonicalize()?;
    let content = format!(
        "module {GO_MODULE}\n\ngo {go_version}\n\nrequire {PRELUDE_IMPORT_PATH} v0.0.0\n\nreplace {PRELUDE_IMPORT_PATH} => {}\n",
        abs.display()
    );
    fs::write(target_dir.join("go.mod"), content)
}

pub fn run_go_test(target_dir: &Path, timeout_per_pkg: &str) -> Result<String, String> {
    let output = Command::new("go")
        .args(["test", "-timeout", timeout_per_pkg, "./..."])
        .current_dir(target_dir)
        .env("NO_COLOR", "1")
        .output()
        .map_err(|e| format!("failed to spawn go: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let combined = format!("{stdout}{stderr}");
    if output.status.success() {
        Ok(combined)
    } else {
        Err(combined)
    }
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate must have parent")
        .to_path_buf()
}

pub fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("spec/emit/snapshots")
}

pub fn prelude_dir() -> PathBuf {
    repo_root().join("prelude")
}

pub fn target_dir() -> PathBuf {
    repo_root().join("target/e2e_suite")
}

pub fn read_skip_list() -> std::collections::HashSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("e2e_suite/skip.txt");
    let Ok(content) = fs::read_to_string(&path) else {
        return std::collections::HashSet::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let name = line.split('#').next().unwrap_or("").trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

pub fn read_go_version() -> std::io::Result<String> {
    let v = fs::read_to_string(repo_root().join("go-version"))?;
    let trimmed = v.trim();
    let mut parts = trimmed.split('.');
    let major = parts.next().unwrap_or("1");
    let minor = parts.next().unwrap_or("25");
    Ok(format!("{major}.{minor}"))
}
