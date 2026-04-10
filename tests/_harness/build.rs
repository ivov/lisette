use diagnostics::SemanticResult;
use emit::{EmitOptions, Emitter};
use semantics::analyze::{AnalyzeInput, SemanticConfig, analyze};
use semantics::loader::Loader;
use semantics::store::ENTRY_MODULE_ID;

use super::filesystem::MockFileSystem;

const ENTRY_FILE_ID: u32 = 0;

pub fn compile_check(fs: MockFileSystem) -> SemanticResult {
    let main_source = fs
        .scan_folder(ENTRY_MODULE_ID)
        .get("main.lis")
        .cloned()
        .expect("main.lis must exist");

    let build_result = syntax::build_ast(&main_source, ENTRY_FILE_ID);
    if build_result.failed() {
        return SemanticResult::with_parse_errors(build_result.errors, ENTRY_MODULE_ID);
    }

    analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: false,
            load_siblings: true,
        },
        loader: &fs,
        source: main_source,
        filename: "main.lis".to_string(),
        ast: build_result.ast,
        project_root: None,
        go_resolver: deps::GoDepResolver::default(),
        compile_phase: semantics::analyze::CompilePhase::Check,
    })
    .0
}

pub fn compile_check_standalone(fs: MockFileSystem) -> SemanticResult {
    let main_source = fs
        .scan_folder(ENTRY_MODULE_ID)
        .get("main.lis")
        .cloned()
        .expect("main.lis must exist");

    let build_result = syntax::build_ast(&main_source, ENTRY_FILE_ID);
    if build_result.failed() {
        return SemanticResult::with_parse_errors(build_result.errors, ENTRY_MODULE_ID);
    }

    analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: true,
            load_siblings: false,
        },
        loader: &fs,
        source: main_source,
        filename: "main.lis".to_string(),
        ast: build_result.ast,
        project_root: None,
        go_resolver: deps::GoDepResolver::default(),
        compile_phase: semantics::analyze::CompilePhase::Check,
    })
    .0
}

pub fn compile_project(fs: MockFileSystem, go_module: &str) -> String {
    let main_source = fs
        .scan_folder(ENTRY_MODULE_ID)
        .get("main.lis")
        .cloned()
        .expect("main.lis must exist");

    let build_result = syntax::build_ast(&main_source, ENTRY_FILE_ID);
    assert!(
        !build_result.failed(),
        "Expected no parse errors, got: {:?}",
        build_result.errors
    );

    let (analysis, _facts) = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: false,
            load_siblings: true,
        },
        loader: &fs,
        source: main_source,
        filename: "main.lis".to_string(),
        ast: build_result.ast,
        project_root: None,
        go_resolver: deps::GoDepResolver::default(),
        compile_phase: semantics::analyze::CompilePhase::Emit,
    });

    assert!(
        analysis.errors.is_empty(),
        "Expected no errors, got: {:?}",
        analysis.errors
    );

    let options = EmitOptions { debug: false };
    let mut files = Emitter::emit(&analysis.into_emit_input(), go_module, options);
    files.sort_by(|a, b| a.name.cmp(&b.name));

    use std::fmt::Write;

    let mut output = String::new();
    for file in files {
        let _ = writeln!(output, "// === {} ===", file.name);
        output.push_str(&file.to_go());
        output.push_str("\n\n");
    }

    let trimmed_len = output.trim_end().len();
    output.truncate(trimmed_len);
    output
}
