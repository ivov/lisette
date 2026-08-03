use emit::{EmitOptions, Planner};
use passes::{Analysis, analyze};
use semantics::loader::Loader;
use semantics::store::ENTRY_MODULE_ID;
use semantics::{AnalysisScope, AnalyzeInput, EntryFile};

use super::filesystem::MockFileSystem;

fn compile_with(
    fs: MockFileSystem,
    scope: AnalysisScope,
    locator: deps::TypedefLocator,
) -> Analysis {
    let main_source = fs
        .scan_folder(ENTRY_MODULE_ID)
        .get("main.lis")
        .map(|c| c.source.clone())
        .expect("main.lis must exist");

    let load_siblings = !matches!(&scope, AnalysisScope::Script { .. });
    analyze(AnalyzeInput {
        load_siblings,
        scope,
        loader: &fs,
        entry: Some(EntryFile::new(
            main_source,
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        locator: &locator,
        compile_phase: semantics::CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        go_module: "",
        disable_cache: false,
    })
}

pub fn compile_check(fs: MockFileSystem) -> Analysis {
    compile_with(
        fs,
        AnalysisScope::Directory,
        deps::TypedefLocator::default(),
    )
}

pub fn compile_script_entry(
    fs: MockFileSystem,
    entry_name: &str,
    phase: semantics::CompilePhase,
) -> Analysis {
    let source = fs
        .scan_folder(ENTRY_MODULE_ID)
        .get(entry_name)
        .map(|c| c.source.clone())
        .unwrap_or_else(|| panic!("entry file `{entry_name}` must exist"));

    analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Script {
            inside_project: false,
        },
        loader: &fs,
        entry: Some(EntryFile::new(
            source,
            entry_name.to_string(),
            entry_name.to_string(),
        )),
        locator: &deps::TypedefLocator::default(),
        compile_phase: phase,
        project_kind: semantics::ProjectKind::Binary,
        go_module: "",
        disable_cache: true,
    })
}

pub fn compile_check_with_locator(fs: MockFileSystem, locator: deps::TypedefLocator) -> Analysis {
    compile_with(fs, AnalysisScope::Directory, locator)
}

pub fn compile_check_script(fs: MockFileSystem) -> Analysis {
    compile_with(
        fs,
        AnalysisScope::Script {
            inside_project: false,
        },
        deps::TypedefLocator::default(),
    )
}

pub fn locator_with_go_dep(module_path: &str, version: &str) -> deps::TypedefLocator {
    let mut go_deps = std::collections::BTreeMap::new();
    go_deps.insert(
        module_path.to_string(),
        deps::GoDependency::Remote {
            version: version.to_string(),
            via: None,
        },
    );
    deps::TypedefLocator::new(go_deps, None, stdlib::Target::host())
}

pub fn compile_project_files(
    fs: MockFileSystem,
    go_module: &str,
    sourcemap: bool,
) -> Vec<emit::OutputFile> {
    try_compile_project_files(fs, go_module, sourcemap)
        .unwrap_or_else(|diagnostics| panic!("Emission failed: {diagnostics:?}"))
}

pub fn try_compile_project_files(
    fs: MockFileSystem,
    go_module: &str,
    sourcemap: bool,
) -> Result<Vec<emit::OutputFile>, Vec<diagnostics::LisetteDiagnostic>> {
    try_compile_project_files_with_tests(fs, go_module, sourcemap, false)
}

pub fn compile_project_files_with_tests(
    fs: MockFileSystem,
    go_module: &str,
    sourcemap: bool,
    emit_tests: bool,
) -> Vec<emit::OutputFile> {
    try_compile_project_files_with_tests(fs, go_module, sourcemap, emit_tests)
        .unwrap_or_else(|diagnostics| panic!("Emission failed: {diagnostics:?}"))
}

pub fn try_compile_project_files_with_tests(
    fs: MockFileSystem,
    go_module: &str,
    sourcemap: bool,
    emit_tests: bool,
) -> Result<Vec<emit::OutputFile>, Vec<diagnostics::LisetteDiagnostic>> {
    let main_source = fs
        .scan_folder(ENTRY_MODULE_ID)
        .get("main.lis")
        .map(|c| c.source.clone())
        .expect("main.lis must exist");

    let analysis = analyze(AnalyzeInput {
        load_siblings: true,
        scope: AnalysisScope::Directory,
        loader: &fs,
        entry: Some(EntryFile::new(
            main_source,
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        locator: &deps::TypedefLocator::default(),
        compile_phase: if emit_tests {
            semantics::CompilePhase::Test
        } else {
            semantics::CompilePhase::Emit
        },
        project_kind: semantics::ProjectKind::Binary,
        go_module,
        disable_cache: true,
    });
    assert!(
        analysis.errors().is_empty(),
        "Expected no errors, got: {:?}",
        analysis.errors()
    );

    Planner::emit(
        &analysis.emit_input,
        go_module,
        "main",
        EmitOptions {
            sourcemap,
            emit_tests,
        },
    )
}

pub fn compile_project(fs: MockFileSystem, go_module: &str) -> String {
    let mut files = compile_project_files(fs, go_module, false);
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
