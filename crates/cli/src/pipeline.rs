use rustc_hash::FxHashMap as HashMap;
use std::path::PathBuf;

use diagnostics::LisetteDiagnostic;
use emit::{EmitOptions, Emitter, OutputFile};

use semantics::analyze::{AnalyzeInput, SemanticConfig, analyze};

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
    pub debug: bool,
    pub project_root: Option<PathBuf>,
}

#[derive(Debug)]
pub struct CompileResult {
    pub output: Vec<OutputFile>,
    pub errors: Vec<LisetteDiagnostic>,
    pub lints: Vec<LisetteDiagnostic>,
    pub sources: HashMap<u32, SourceInfo>,
}

pub fn compile(
    source: &str,
    filename: &str,
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
                filename: filename.to_string(),
            },
        );
        return CompileResult {
            output: vec![],
            errors,
            lints: vec![],
            sources,
        };
    }

    let (semantic_result, _facts) = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: config.standalone_mode,
            load_siblings: config.load_siblings,
        },
        loader: fs,
        source: source.to_string(),
        filename: filename.to_string(),
        ast: syntax_result.ast,
        project_root: config.project_root.clone(),
        compile_phase: config.target_phase,
    });

    let sources: HashMap<u32, SourceInfo> = semantic_result
        .files
        .iter()
        .map(|(file_id, file)| {
            (
                *file_id,
                SourceInfo {
                    source: file.source.clone(),
                    filename: file.name.clone(),
                },
            )
        })
        .collect();

    let failed = semantic_result.failed();
    let errors = semantic_result.errors.clone();
    let lints = semantic_result.lints.clone();

    if failed || config.target_phase == CompilePhase::Check {
        return CompileResult {
            output: vec![],
            errors,
            lints,
            sources,
        };
    }

    let output = Emitter::emit(
        &semantic_result.into_emit_input(),
        &config.go_module,
        EmitOptions {
            debug: config.debug,
        },
    );

    CompileResult {
        output,
        errors,
        lints,
        sources,
    }
}
