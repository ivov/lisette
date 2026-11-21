//! WebAssembly bindings for the Lisette compiler.
//!
//! All functions are called directly from JavaScript via wasm-bindgen.
//! Diagnostics are serialised as JSON strings so the TS layer can decode them.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use lisette_passes::analyze;
use lisette_semantics::loader::MemoryLoader;
use lisette_semantics::{
    AnalysisScope, AnalyzeInput, CompilePhase, EntryFile, ProjectKind, RecoverTarget,
};

// ─── Panic hook ───────────────────────────────────────────────────────────────
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// The Lisette compiler version this WASM was built against.
#[wasm_bindgen]
pub fn version() -> String {
    env!("LIS_VERSION").to_string()
}

// ─── Serialisable output types ────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct JsDiagnostic {
    severity: String,
    message: String,
    line: u32,
    col: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_col: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

#[derive(Serialize)]
struct JsCompileResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    go_source: Option<String>,
    diagnostics: Vec<JsDiagnostic>,
}

// ─── Helper: byte offset → (line, col), both 1-based ────────────────────────
fn offset_to_line_col(source: &str, byte_offset: usize) -> (u32, u32) {
    let prefix = &source[..source.floor_char_boundary(byte_offset)];
    let line = (prefix.bytes().filter(|&b| b == b'\n').count() + 1) as u32;
    let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    let col = (prefix[line_start..].encode_utf16().count() + 1) as u32;
    (line, col)
}

// ─── Convert LisetteDiagnostic to JsDiagnostic ────────────────────────────────

fn convert_lisette_diag(
    diag: &lisette_diagnostics::LisetteDiagnostic,
    source: &str,
) -> JsDiagnostic {
    let message = diag.plain_message().to_string();
    let severity = if diag.is_error() {
        "error"
    } else if diag.is_info() {
        "info"
    } else {
        "warning"
    }
    .to_string();

    let offset = diag.primary_offset();
    let (line, col, end_line, end_col) = {
        let (l, c) = offset_to_line_col(source, offset);
        (l, c, None, None)
    };

    JsDiagnostic {
        severity,
        message,
        line,
        col,
        end_line,
        end_col,
        code: diag.code_str().map(str::to_string),
    }
}

// ─── Core pipeline ────────────────────────────────────────────────────────────

const PLAYGROUND_FILE: &str = "playground.lis";

fn run_pipeline(
    code: &str,
    phase: CompilePhase,
) -> (Vec<lisette_emit::OutputFile>, Vec<JsDiagnostic>) {
    let mut loader = MemoryLoader::new();
    loader.add_file("_entry_", PLAYGROUND_FILE, code);

    let input = AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Script {
            inside_project: false,
        },
        loader: &loader,
        entry: Some(EntryFile::new(
            code.to_string(),
            PLAYGROUND_FILE.to_string(),
            PLAYGROUND_FILE.to_string(),
        )),
        compile_phase: phase,
        project_kind: ProjectKind::Binary,
        locator: &lisette_deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: false,
        recover_target: RecoverTarget::None,
    };

    let mut analysis = analyze(input);
    let mut diagnostics = analysis
        .diagnostics()
        .iter()
        .map(|diagnostic| convert_lisette_diag(diagnostic, code))
        .collect();
    if analysis.has_parse_errors() {
        return (Vec::new(), diagnostics);
    }

    if matches!(phase, CompilePhase::Check) || analysis.failed() {
        return (vec![], diagnostics);
    }

    let go_files = match lisette_emit::Planner::emit(
        &mut analysis.emit_input,
        "",
        "main",
        lisette_emit::EmitOptions {
            sourcemap: false,
            sourcemap_base: None,
            emit_tests: false,
        },
    ) {
        Ok(files) => files,
        Err(emit_diagnostics) => {
            diagnostics.extend(
                emit_diagnostics
                    .iter()
                    .map(|diagnostic| convert_lisette_diag(diagnostic, code)),
            );
            Vec::new()
        }
    };

    (go_files, diagnostics)
}

// ─── Public WASM API ──────────────────────────────────────────────────────────

/// Format Lisette source. Returns the formatted source, or the original on failure.
#[wasm_bindgen]
pub fn format(code: &str) -> String {
    match lisette_format::format_source(code) {
        Ok(formatted) => formatted,
        Err(_) => code.to_string(),
    }
}

/// Type-check source and return a JSON array of diagnostics.
#[wasm_bindgen]
pub fn check(code: &str) -> String {
    let (_files, diags) = run_pipeline(code, CompilePhase::Check);
    serde_json::to_string(&diags).unwrap_or_else(|_| "[]".to_string())
}

/// Compile Lisette → Go. Returns a JSON object:
/// `{ "ok": bool, "go_source": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub fn compile(code: &str) -> String {
    let (files, diags) = run_pipeline(code, CompilePhase::Emit);
    let has_errors = diags.iter().any(|d| d.severity == "error");

    let go_source = if !has_errors && !files.is_empty() {
        Some(
            files
                .iter()
                .map(|f| f.to_go())
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    } else {
        None
    };

    let result = JsCompileResult {
        ok: !has_errors && go_source.is_some(),
        go_source,
        diagnostics: diags,
    };

    serde_json::to_string(&result).unwrap_or_else(|_| {
        r#"{"ok":false,"diagnostics":[{"severity":"error","message":"Internal error","line":1,"col":1}]}"#.to_string()
    })
}

// ─── Language server ──────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct LspSession {
    session: lisette_lsp::session::Session,
}

#[wasm_bindgen]
impl LspSession {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            session: lisette_lsp::session::Session::new(),
        }
    }

    pub fn handle(&self, message: &str) -> String {
        let replies = match serde_json::from_str(message) {
            Ok(message) => self.session.handle(&message),
            Err(_) => Vec::new(),
        };
        serde_json::to_string(&replies).unwrap_or_else(|_| "[]".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_columns_count_utf16_code_units() {
        let source = "let a = \"Köln 🎉\"; x";
        let offset = source.find('x').unwrap();
        assert_eq!(offset_to_line_col(source, offset), (1, 20));
    }

    #[test]
    fn info_diagnostic_converts_to_info_severity() {
        let diag = lisette_diagnostics::LisetteDiagnostic::info("advisory");
        assert_eq!(convert_lisette_diag(&diag, "").severity, "info");
    }

    #[test]
    fn warning_diagnostic_converts_to_warning_severity() {
        let diag = lisette_diagnostics::LisetteDiagnostic::warn("w");
        assert_eq!(convert_lisette_diag(&diag, "").severity, "warning");
    }

    #[test]
    fn compile_reports_emitter_diagnostics() {
        let source = r#"
import FooBar "go:fmt"

pub fn foo_bar() -> int { 1 }

fn main() {
  let _ = FooBar.Println("x")
  let _ = foo_bar()
}
"#;

        let (files, diagnostics) = run_pipeline(source, CompilePhase::Emit);

        assert!(files.is_empty());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("emit.go_name_collision"))
        );
    }
}
