use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

use deps::TypedefLocator;
use diagnostics::LisetteDiagnostic;
use emit::{EmitOptions, OutputFile, Planner};

use semantics::analyze::{AnalyzeInput, SemanticConfig, analyze};
use semantics::cache::EmitStamp;

pub use semantics::analyze::CompilePhase;
use semantics::loader::Loader;

const ENTRY_FILE_ID: u32 = 0;

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub source: String,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct CompileConfig {
    pub target_phase: CompilePhase,
    pub go_module: String,
    pub standalone_mode: bool,
    pub load_siblings: bool,
    pub sourcemap: bool,
    pub project_root: Option<PathBuf>,
    pub locator: TypedefLocator,
}

#[derive(Debug)]
pub struct CompileResult {
    pub output: Vec<OutputFile>,
    pub errors: Vec<LisetteDiagnostic>,
    pub lints: Vec<LisetteDiagnostic>,
    pub sources: HashMap<u32, SourceInfo>,
    pub user_file_count: usize,
    pub live_modules: Vec<String>,
    pub emit_stamps: Vec<EmitStamp>,
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
        };
    }

    let disable_cache = config.sourcemap && config.target_phase == CompilePhase::Emit;

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

    if failed || config.target_phase == CompilePhase::Check {
        return CompileResult {
            output: vec![],
            errors,
            lints,
            sources,
            user_file_count,
            live_modules,
            emit_stamps,
        };
    }

    let mut output = Planner::emit(
        &semantic_result.into_emit_input(),
        &config.go_module,
        EmitOptions {
            sourcemap: config.sourcemap,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::LocalFileSystem;
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
}
