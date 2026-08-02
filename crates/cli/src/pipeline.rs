use rustc_hash::FxHashMap as HashMap;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use emit::{EmitOptions, OutputFile, Planner};

use passes::analyze;
use semantics::cache::EmitStamp;
use semantics::{AnalyzeInput, EntryFile};

use semantics::CompilePhase;
use semantics::loader::Loader;
pub use semantics::{AnalysisScope as CompileScope, ProjectKind};
pub use syntax::program::TestIndex;

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub source: String,
    pub filename: String,
}

/// Per-file source, with key as file_id, for mapping diagnostics back to source text.
pub type Sources = HashMap<u32, SourceInfo>;

#[derive(Debug)]
pub struct CompileConfig<'a> {
    pub mode: CompileMode,
    pub go_module: &'a str,
    pub entry_package_name: &'a str,
    pub scope: CompileScope,
    pub locator: &'a TypedefLocator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileMode {
    Check,
    Emit { sourcemap: bool },
    Test,
}

impl CompileMode {
    fn phase(self) -> CompilePhase {
        match self {
            Self::Check => CompilePhase::Check,
            Self::Emit { .. } => CompilePhase::Emit,
            Self::Test => CompilePhase::Test,
        }
    }

    fn emit_options(self) -> Option<EmitOptions> {
        match self {
            Self::Check => None,
            Self::Emit { sourcemap } => Some(EmitOptions {
                sourcemap,
                emit_tests: false,
            }),
            Self::Test => Some(EmitOptions {
                sourcemap: false,
                emit_tests: true,
            }),
        }
    }

    fn disables_cache(self) -> bool {
        matches!(self, Self::Emit { sourcemap: true } | Self::Test)
    }
}

pub struct CompileEntry<'a> {
    pub source: &'a str,
    pub filename: &'a str,
    pub display_path: &'a str,
}

pub enum CompileInput<'a> {
    Binary(CompileEntry<'a>),
    Library,
}

#[derive(Debug)]
pub struct CompileResult {
    pub output: Vec<OutputFile>,
    pub diagnostics: Vec<LisetteDiagnostic>,
    pub sources: Sources,
    pub user_file_count: usize,
    pub live_modules: Vec<String>,
    pub emit_stamps: Vec<EmitStamp>,
    pub test_index: TestIndex,
}

impl CompileResult {
    pub fn errors(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics[..self.error_count()]
    }

    pub fn lints(&self) -> &[LisetteDiagnostic] {
        &self.diagnostics[self.error_count()..]
    }

    fn error_count(&self) -> usize {
        self.diagnostics
            .partition_point(LisetteDiagnostic::is_error)
    }
}

pub fn compile(
    input: CompileInput<'_>,
    config: CompileConfig<'_>,
    fs: &dyn Loader,
) -> CompileResult {
    let CompileConfig {
        mode,
        go_module,
        entry_package_name,
        scope,
        locator,
    } = config;
    let (entry_file, project_kind) = match input {
        CompileInput::Binary(entry) => (
            Some(EntryFile::new(
                entry.source.to_string(),
                entry.filename.to_string(),
                entry.display_path.to_string(),
            )),
            ProjectKind::Binary,
        ),
        CompileInput::Library => (None, ProjectKind::Library),
    };

    let disable_cache = mode.disables_cache();
    let emit_tests = mode
        .emit_options()
        .is_some_and(|options| options.emit_tests);

    let mut analysis = analyze(AnalyzeInput {
        load_siblings: !matches!(&scope, CompileScope::Standalone { .. }),
        scope,
        loader: fs,
        entry: entry_file,
        compile_phase: mode.phase(),
        project_kind,
        locator,
        go_module,
        disable_cache,
    });
    let failed = analysis.failed();
    let emit_result = match mode.emit_options() {
        Some(options) if !failed => Some(Planner::emit(
            &analysis.emit_input,
            go_module,
            entry_package_name,
            options,
        )),
        _ => None,
    };

    let mut diagnostics = analysis.take_diagnostics();
    if !emit_tests {
        for module_id in &analysis.unreachable_modules {
            diagnostics.push(diagnostics::module_graph::unreachable_module(module_id));
        }
    }

    let mut sources = Sources::default();
    let mut live_modules = Vec::new();
    let mut user_file_count = 0;
    for (file_id, file) in analysis.emit_input.files {
        if !file.is_d_lis() {
            user_file_count += 1;
            live_modules.push(file.module_id);
        }
        sources.insert(
            file_id,
            SourceInfo {
                source: file.source,
                filename: file.display_path,
            },
        );
    }
    live_modules.sort_unstable();
    live_modules.dedup();
    let test_index = analysis.emit_input.test_index;

    let output = match emit_result {
        Some(Ok(output)) => output,
        Some(Err(emit_diagnostics)) => {
            let mut error_count = diagnostics.partition_point(LisetteDiagnostic::is_error);
            for diagnostic in emit_diagnostics {
                if diagnostic.is_error() {
                    diagnostics.insert(error_count, diagnostic);
                    error_count += 1;
                } else {
                    diagnostics.push(diagnostic);
                }
            }
            Vec::new()
        }
        None => Vec::new(),
    };

    let emit_stamps = analysis.emit_stamps;

    CompileResult {
        output,
        diagnostics,
        sources,
        user_file_count,
        live_modules,
        emit_stamps,
        test_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::LocalFileSystem;
    use semantics::PARALLEL_THRESHOLD;
    use std::fs as stdfs;
    use tempfile::tempdir;

    fn check_diagnostics(project_dir: &std::path::Path) -> Vec<(bool, Option<String>)> {
        let (_, locator) = TypedefLocator::from_project_with_manifest(project_dir).unwrap();
        let src_main = project_dir.join("src").join("main.lis");
        let source = stdfs::read_to_string(&src_main).unwrap();
        let config = CompileConfig {
            mode: CompileMode::Check,
            go_module: "test",
            entry_package_name: "main",
            scope: CompileScope::Project(project_dir.to_path_buf()),
            locator: &locator,
        };
        let working_dir = src_main
            .parent()
            .and_then(|p| p.to_str())
            .expect("temp project path is valid utf-8");
        let fs_loader = LocalFileSystem::new(working_dir, Some(project_dir));
        let result = compile(
            CompileInput::Binary(CompileEntry {
                source: &source,
                filename: "main.lis",
                display_path: "src/main.lis",
            }),
            config,
            &fs_loader,
        );

        let mut diags: Vec<(bool, Option<String>)> = result
            .diagnostics
            .iter()
            .map(|d| (d.is_error(), d.code_str().map(|s| s.to_string())))
            .collect();
        diags.sort();
        diags
    }

    fn compile_project_source(
        source: &str,
        project_kind: ProjectKind,
        target_phase: CompilePhase,
        entry_package_name: &str,
    ) -> CompileResult {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        stdfs::create_dir_all(root.join("src")).unwrap();
        stdfs::write(
            root.join("lisette.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let filename = match project_kind {
            ProjectKind::Binary => "main.lis",
            ProjectKind::Library => "lib.lis",
        };
        stdfs::write(root.join("src").join(filename), source).unwrap();

        let (_, locator) = TypedefLocator::from_project_with_manifest(root).unwrap();
        let source_path = root.join("src").join(filename);
        let config = CompileConfig {
            mode: match target_phase {
                CompilePhase::Check => CompileMode::Check,
                CompilePhase::Emit => CompileMode::Emit { sourcemap: false },
                CompilePhase::Test => CompileMode::Test,
            },
            go_module: "test",
            entry_package_name,
            scope: CompileScope::Project(root.to_path_buf()),
            locator: &locator,
        };
        let fs_loader =
            LocalFileSystem::new(source_path.parent().unwrap().to_str().unwrap(), Some(root));
        let input = match project_kind {
            ProjectKind::Binary => CompileInput::Binary(CompileEntry {
                source,
                filename,
                display_path: "src/main.lis",
            }),
            ProjectKind::Library => CompileInput::Library,
        };
        compile(input, config, &fs_loader)
    }

    #[test]
    fn entry_package_name_fact_reaches_emitted_output() {
        let result = compile_project_source(
            "fn main() {}\n",
            ProjectKind::Binary,
            CompilePhase::Emit,
            "widget",
        );
        assert!(result.errors().is_empty(), "{:?}", result.errors());
        assert!(
            result.output.iter().any(|f| f.package_name == "widget"),
            "the entry-package-name fact should reach emitted output, got: {:?}",
            result
                .output
                .iter()
                .map(|f| f.package_name.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn library_main_function_skips_binary_signature_check() {
        let binary = compile_project_source(
            "fn main(x: int) {}\n",
            ProjectKind::Binary,
            CompilePhase::Check,
            "main",
        );
        assert!(
            binary
                .errors()
                .iter()
                .any(|d| d.code_str() == Some("infer.invalid_main_signature")),
            "a binary `main` with parameters must be rejected"
        );

        let library = compile_project_source(
            "fn main(x: int) {}\n",
            ProjectKind::Library,
            CompilePhase::Check,
            "main",
        );
        assert!(
            !library
                .errors()
                .iter()
                .any(|d| d.code_str() == Some("infer.invalid_main_signature")),
            "a library `main` is an ordinary function, not a binary entrypoint"
        );
    }

    #[test]
    fn syntax_error_stops_batch_analysis_but_retains_entry_source() {
        let result =
            compile_project_source("fn main(", ProjectKind::Binary, CompilePhase::Check, "main");

        assert_eq!(
            (
                result
                    .errors()
                    .first()
                    .and_then(LisetteDiagnostic::code_str),
                result.user_file_count,
                result.sources.get(&0).map(|source| source.source.as_str()),
                result.output.is_empty(),
            ),
            (Some("parse.unexpected_token"), 1, Some("fn main("), true)
        );
    }

    #[test]
    fn warm_diagnostics_match_cold_for_param_position_leak() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        stdfs::create_dir_all(root.join("src").join("leaky")).unwrap();
        stdfs::write(
            root.join("lisette.toml"),
            "[project]\nname = \"github.com/test/cache\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        stdfs::write(
            root.join("src").join("main.lis"),
            "import \"leaky\"\n\nfn main() {\n  let _ = leaky.make_item(42)\n}\n",
        )
        .unwrap();
        stdfs::write(
            root.join("src").join("leaky").join("leaky.lis"),
            "struct Item {\n  pub id: int,\n}\n\n\
             pub fn extract_id(it: Item) -> int {\n  it.id\n}\n\n\
             pub fn make_item(id: int) -> int {\n  let it = Item { id: id }\n  it.id\n}\n",
        )
        .unwrap();

        let cold = check_diagnostics(root);
        let warm = check_diagnostics(root);
        let warm_again = check_diagnostics(root);

        assert!(
            cold.iter()
                .any(|(_, code)| code.as_deref() == Some("lint.internal_type_leak")),
            "cold run must produce internal_type_leak; otherwise the test is not exercising the bug. got: {:?}",
            cold
        );
        assert_eq!(
            cold, warm,
            "diagnostics diverge between cold and first warm build"
        );
        assert_eq!(
            cold, warm_again,
            "diagnostics diverge between cold and second warm build"
        );
        assert!(
            !root.join("target/.lisette/cache/leaky.cache").exists(),
            "leaky has warnings; cache must not write it"
        );
    }

    fn analyze_cache_state(
        project_dir: &std::path::Path,
    ) -> (Vec<String>, Vec<(bool, Option<String>)>) {
        let (_, locator) = TypedefLocator::from_project_with_manifest(project_dir).unwrap();
        let src_main = project_dir.join("src").join("main.lis");
        let source = stdfs::read_to_string(&src_main).unwrap();
        let working_dir = src_main
            .parent()
            .and_then(|p| p.to_str())
            .expect("temp project path is valid utf-8");
        let fs_loader = LocalFileSystem::new(working_dir, Some(project_dir));
        let result = analyze(AnalyzeInput {
            load_siblings: true,
            scope: CompileScope::Project(project_dir.to_path_buf()),
            loader: &fs_loader,
            entry: Some(EntryFile::new(
                source,
                "main.lis".to_string(),
                "src/main.lis".to_string(),
            )),
            compile_phase: CompilePhase::Check,
            project_kind: ProjectKind::Binary,
            locator: &locator,
            go_module: "test",
            disable_cache: false,
        });

        let mut cached: Vec<String> = result.emit_input.cached_modules.iter().cloned().collect();
        cached.sort();
        let mut diags: Vec<(bool, Option<String>)> = result
            .diagnostics()
            .iter()
            .map(|d| (d.is_error(), d.code_str().map(|s| s.to_string())))
            .collect();
        diags.sort();
        (cached, diags)
    }

    #[test]
    fn warm_build_parallel_cache_load_matches_cold() {
        const N: usize = PARALLEL_THRESHOLD + 1;
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        stdfs::write(
            root.join("lisette.toml"),
            "[project]\nname = \"github.com/test/parcache\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let mut main = String::new();
        for i in 0..N {
            stdfs::create_dir_all(root.join("src").join(format!("m{i}"))).unwrap();
            stdfs::write(
                root.join("src")
                    .join(format!("m{i}"))
                    .join(format!("m{i}.lis")),
                format!("pub fn val() -> int {{ {i} }}\n"),
            )
            .unwrap();
            main.push_str(&format!("import \"m{i}\"\n"));
        }
        let sum = (0..N)
            .map(|i| format!("m{i}.val()"))
            .collect::<Vec<_>>()
            .join(" + ");
        main.push_str(&format!("\nfn main() {{\n  let _ = {sum}\n}}\n"));
        stdfs::write(root.join("src").join("main.lis"), main).unwrap();

        let mut expected: Vec<String> = (0..N).map(|i| format!("m{i}")).collect();
        expected.sort();

        let (cold_cached, cold_diags) = analyze_cache_state(root);
        assert!(
            cold_cached.is_empty(),
            "cold run must load nothing from cache; got: {cold_cached:?}"
        );
        assert!(
            cold_diags.is_empty(),
            "fixture must be clean; got: {cold_diags:?}"
        );
        for i in 0..N {
            assert!(
                root.join(format!("target/.lisette/cache/m{i}.cache"))
                    .exists(),
                "m{i} must be cached after the cold run"
            );
        }

        let (warm_cached, warm_diags) = analyze_cache_state(root);
        assert_eq!(
            warm_cached, expected,
            "warm run must serve every module from cache via the parallel path"
        );
        assert_eq!(
            warm_diags, cold_diags,
            "warm cross-module resolution must match cold"
        );
    }

    fn test_index_names(project_dir: &std::path::Path) -> Vec<String> {
        let (_, locator) = TypedefLocator::from_project_with_manifest(project_dir).unwrap();
        let src_main = project_dir.join("src").join("main.lis");
        let source = stdfs::read_to_string(&src_main).unwrap();
        let working_dir = src_main
            .parent()
            .and_then(|p| p.to_str())
            .expect("temp project path is valid utf-8");
        let fs_loader = LocalFileSystem::new(working_dir, Some(project_dir));
        let output = analyze(AnalyzeInput {
            load_siblings: true,
            scope: CompileScope::Project(project_dir.to_path_buf()),
            loader: &fs_loader,
            entry: Some(EntryFile::new(
                source,
                "main.lis".to_string(),
                "src/main.lis".to_string(),
            )),
            compile_phase: CompilePhase::Check,
            project_kind: ProjectKind::Binary,
            locator: &locator,
            go_module: "test",
            disable_cache: false,
        });
        let mut names: Vec<String> = output
            .emit_input
            .test_index
            .tests()
            .iter()
            .map(|test| test.qualified_name().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn test_index_retains_cached_module_tests_on_warm_build() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        stdfs::create_dir_all(root.join("src").join("math")).unwrap();
        stdfs::write(
            root.join("lisette.toml"),
            "[project]\nname = \"github.com/test/tests\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        stdfs::write(
            root.join("src").join("main.lis"),
            "import \"math\"\n\nfn main() {\n  let _ = math.add(1, 2)\n}\n",
        )
        .unwrap();
        stdfs::write(
            root.join("src").join("math").join("math.lis"),
            "pub fn add(a: int, b: int) -> int { a + b }\n",
        )
        .unwrap();
        stdfs::write(
            root.join("src").join("math").join("math.test.lis"),
            "#[test]\npub fn alpha() {}\n",
        )
        .unwrap();

        let cold = test_index_names(root);
        assert!(
            cold.contains(&"math.alpha".to_string()),
            "cold run must record math.alpha, got: {cold:?}"
        );

        assert!(
            root.join("target/.lisette/cache/math.cache").exists(),
            "math must be cached after the cold run for this test to be meaningful"
        );

        let warm = test_index_names(root);
        assert_eq!(
            cold, warm,
            "tests in a cached module must survive a warm build"
        );
    }
}
