use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use emit::{EmitOptions, OutputFile, Planner};

use passes::analyze;
use semantics::cache::EmitStamp;
use semantics::inference::{AnalyzeInput, SemanticConfig};

pub use semantics::inference::CompilePhase;
use semantics::loader::Loader;
pub use syntax::program::TestIndex;

const ENTRY_FILE_ID: u32 = 0;

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub source: String,
    pub filename: String,
}

/// Per-file source, with key as file_id, for mapping diagnostics back to source text.
pub type Sources = HashMap<u32, SourceInfo>;

#[derive(Debug, Clone)]
pub struct CompileConfig {
    pub target_phase: CompilePhase,
    pub go_module: String,
    pub standalone_mode: bool,
    pub load_siblings: bool,
    pub sourcemap: bool,
    pub emit_tests: bool,
    pub project_root: Option<PathBuf>,
    pub locator: TypedefLocator,
}

#[derive(Debug)]
pub struct CompileResult {
    pub output: Vec<OutputFile>,
    pub errors: Vec<LisetteDiagnostic>,
    pub lints: Vec<LisetteDiagnostic>,
    pub sources: Sources,
    pub user_file_count: usize,
    pub live_modules: Vec<String>,
    pub emit_stamps: Vec<EmitStamp>,
    pub test_index: TestIndex,
}

pub fn compile(
    source: &str,
    filename: &str,
    display_path: &str,
    config: &CompileConfig,
    fs: &dyn Loader,
) -> CompileResult {
    let syntax_result = syntax::build_ast(source, ENTRY_FILE_ID);
    if syntax_result.failed() {
        let errors = syntax_result.errors.into_iter().map(Into::into).collect();
        let mut sources = HashMap::default();
        sources.insert(
            ENTRY_FILE_ID,
            SourceInfo {
                source: source.to_string(),
                filename: display_path.to_string(),
            },
        );
        return CompileResult {
            output: vec![],
            errors,
            lints: vec![],
            sources,
            user_file_count: 1,
            live_modules: vec![],
            emit_stamps: vec![],
            test_index: TestIndex::default(),
        };
    }

    let disable_cache =
        config.emit_tests || (config.sourcemap && config.target_phase == CompilePhase::Emit);

    let analyze_output = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: config.standalone_mode,
            load_siblings: config.load_siblings,
        },
        loader: fs,
        source: source.to_string(),
        filename: filename.to_string(),
        display_path: display_path.to_string(),
        ast: syntax_result.ast,
        project_root: config.project_root.clone(),
        compile_phase: config.target_phase,
        emit_tests: config.emit_tests,
        locator: config.locator.clone(),
        go_module: config.go_module.clone(),
        disable_cache,
    });
    let semantic_result = analyze_output.result;
    let emit_stamps = analyze_output.emit_stamps;

    let user_file_count: usize = semantic_result
        .modules
        .values()
        .map(|module| module.file_ids.len())
        .sum();

    let sources: HashMap<u32, SourceInfo> = semantic_result
        .files
        .iter()
        .map(|(file_id, file)| {
            (
                *file_id,
                SourceInfo {
                    source: file.source.clone(),
                    filename: file.display_path.clone(),
                },
            )
        })
        .collect();

    let failed = semantic_result.failed();
    let mut errors = semantic_result.errors.clone();
    let lints = semantic_result.lints.clone();
    let live_modules: Vec<String> = semantic_result.modules.keys().cloned().collect();
    let test_index = semantic_result.test_index.clone();

    if failed || config.target_phase == CompilePhase::Check {
        return CompileResult {
            output: vec![],
            errors,
            lints,
            sources,
            user_file_count,
            live_modules,
            emit_stamps,
            test_index,
        };
    }

    let mut output = Planner::emit(
        &semantic_result.into_emit_input(),
        &config.go_module,
        EmitOptions {
            sourcemap: config.sourcemap,
            emit_tests: config.emit_tests,
        },
    );

    for file in &mut output {
        errors.append(&mut file.diagnostics);
    }

    if errors.iter().any(|d| d.is_error()) {
        return CompileResult {
            output: vec![],
            errors,
            lints,
            sources,
            user_file_count,
            live_modules,
            emit_stamps,
            test_index,
        };
    }

    CompileResult {
        output,
        errors,
        lints,
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
    use semantics::inference::PARALLEL_THRESHOLD;
    use std::fs as stdfs;
    use tempfile::tempdir;

    fn check_diagnostics(project_dir: &std::path::Path) -> Vec<(bool, Option<String>)> {
        let (_, locator) = TypedefLocator::from_project_with_manifest(project_dir).unwrap();
        let src_main = project_dir.join("src").join("main.lis");
        let source = stdfs::read_to_string(&src_main).unwrap();
        let config = CompileConfig {
            target_phase: CompilePhase::Check,
            go_module: "test".to_string(),
            standalone_mode: false,
            load_siblings: true,
            sourcemap: false,
            emit_tests: false,
            project_root: Some(project_dir.to_path_buf()),
            locator,
        };
        let working_dir = src_main
            .parent()
            .and_then(|p| p.to_str())
            .expect("temp project path is valid utf-8");
        let fs_loader = LocalFileSystem::new(working_dir);
        let result = compile(&source, "main.lis", "src/main.lis", &config, &fs_loader);

        let mut diags: Vec<(bool, Option<String>)> = result
            .errors
            .iter()
            .chain(result.lints.iter())
            .map(|d| (d.is_error(), d.code_str().map(|s| s.to_string())))
            .collect();
        diags.sort();
        diags
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
            !root.join("target/cache/leaky.cache").exists(),
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
        let fs_loader = LocalFileSystem::new(working_dir);
        let build_result = syntax::build_ast(&source, ENTRY_FILE_ID);
        let result = analyze(AnalyzeInput {
            config: SemanticConfig {
                run_lints: true,
                standalone_mode: false,
                load_siblings: true,
            },
            loader: &fs_loader,
            source,
            filename: "main.lis".to_string(),
            display_path: "src/main.lis".to_string(),
            ast: build_result.ast,
            project_root: Some(project_dir.to_path_buf()),
            compile_phase: CompilePhase::Check,
            emit_tests: false,
            locator,
            go_module: "test".to_string(),
            disable_cache: false,
        })
        .result;

        let mut cached: Vec<String> = result.cached_modules.iter().cloned().collect();
        cached.sort();
        let mut diags: Vec<(bool, Option<String>)> = result
            .errors
            .iter()
            .chain(result.lints.iter())
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
                root.join(format!("target/cache/m{i}.cache")).exists(),
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
        let fs_loader = LocalFileSystem::new(working_dir);
        let build_result = syntax::build_ast(&source, ENTRY_FILE_ID);
        let output = analyze(AnalyzeInput {
            config: SemanticConfig {
                run_lints: true,
                standalone_mode: false,
                load_siblings: true,
            },
            loader: &fs_loader,
            source,
            filename: "main.lis".to_string(),
            display_path: "src/main.lis".to_string(),
            ast: build_result.ast,
            project_root: Some(project_dir.to_path_buf()),
            compile_phase: CompilePhase::Check,
            emit_tests: false,
            locator,
            go_module: "test".to_string(),
            disable_cache: false,
        });
        let mut names: Vec<String> = output
            .result
            .test_index
            .tests()
            .iter()
            .map(|t| t.qualified_name.clone())
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
            root.join("target/cache/math.cache").exists(),
            "math must be cached after the cold run for this test to be meaningful"
        );

        let warm = test_index_names(root);
        assert_eq!(
            cold, warm,
            "tests in a cached module must survive a warm build"
        );
    }
}
