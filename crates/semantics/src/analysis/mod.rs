//! Coordinates entry parsing, module discovery, caching, registration, and inference.

mod entry;
mod modules;

use rustc_hash::FxHashSet as HashSet;

use entry::{compute_roots, find_unreachable_modules, load_sibling_files, register_entry_file};
use modules::{ModuleInferenceInput, infer_all_modules};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diagnostics::LocalSink;
use syntax::ParseError;
use syntax::lex::Lexer;
use syntax::parse::{ParseResult, Parser};
use syntax::program::{File, Module};

use deps::TypedefLocator;

use crate::cache::{
    CompiledModule, ModuleInterface, build_cached_module, compute_emit_artifact_hash,
    compute_module_hash, get_dependency_module_hashes,
    go_stdlib::{self, load_cached_go_module},
    hash_module_source_pair, hash_module_source_pair_refs, is_cache_disabled,
    prelude as prelude_cache, try_load_cache,
};
use crate::checker::infer::{FileInferenceInput, InferCtx};
use crate::checker::{TaskOutput, TaskState};
use crate::diagnostics::{GoImportSite, emit_for_locator_result};
use crate::facts::Facts;
use crate::loader::{DiscoveredModules, Loader};
use crate::module_graph::{DependencyGraph, ModuleGraphOptions, Roots, build_module_graph};
use crate::prelude::{parse_and_register_prelude, parse_and_register_test_prelude};
use crate::store::{ENTRY_FILE_ID, ENTRY_MODULE_ID, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilePhase {
    #[default]
    Check,
    Emit,
    Test,
}

impl CompilePhase {
    fn includes_tests(self) -> bool {
        matches!(self, Self::Check | Self::Test)
    }

    fn emits(self) -> bool {
        matches!(self, Self::Emit | Self::Test)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectKind {
    #[default]
    Binary,
    Library,
}

/// The filesystem context in which analysis runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisScope {
    Standalone,
    Directory,
    Project(PathBuf),
}

impl AnalysisScope {
    pub(crate) fn is_standalone(&self) -> bool {
        matches!(self, Self::Standalone)
    }

    pub(crate) fn has_project_root(&self) -> bool {
        matches!(self, Self::Project(_))
    }

    fn project_root(&self) -> Option<&Path> {
        match self {
            Self::Project(root) => Some(root),
            Self::Standalone | Self::Directory => None,
        }
    }
}

pub struct EntryFile {
    source: String,
    filename: String,
    display_path: String,
    parse_mode: EntryParseMode,
}

impl EntryFile {
    /// Creates an entry whose syntax errors stop analysis.
    pub fn new(source: String, filename: String, display_path: String) -> Self {
        Self {
            source,
            filename,
            display_path,
            parse_mode: EntryParseMode::Strict,
        }
    }

    /// Creates an entry whose parser errors retain the valid partial AST.
    /// Lexer errors remain fatal.
    pub fn recovering(source: String, filename: String, display_path: String) -> Self {
        Self {
            source,
            filename,
            display_path,
            parse_mode: EntryParseMode::Recover,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryParseMode {
    Strict,
    Recover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntryParseStatus {
    #[default]
    Clean,
    Recovered,
    Failed,
}

pub enum EntryParseOutcome {
    Clean,
    Recovered(Vec<ParseError>),
    Failed(Vec<ParseError>),
}

impl EntryParseOutcome {
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Clean)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    pub fn status(&self) -> EntryParseStatus {
        match self {
            Self::Clean => EntryParseStatus::Clean,
            Self::Recovered(_) => EntryParseStatus::Recovered,
            Self::Failed(_) => EntryParseStatus::Failed,
        }
    }

    pub fn into_errors(self) -> Vec<ParseError> {
        match self {
            Self::Clean => Vec::new(),
            Self::Recovered(errors) | Self::Failed(errors) => errors,
        }
    }
}

pub struct AnalyzeInput<'a> {
    pub load_siblings: bool,
    pub scope: AnalysisScope,
    pub loader: &'a dyn Loader,
    /// An explicitly supplied file, when the caller has one. Project files not
    /// supplied here are discovered through the loader.
    pub entry: Option<EntryFile>,
    pub compile_phase: CompilePhase,
    pub project_kind: ProjectKind,
    pub locator: &'a TypedefLocator,
    /// Go module path (from `lisette.toml`); folded into the cache emit-artifact
    /// hash so a project rename invalidates Go outputs.
    pub go_module: &'a str,
    /// When true, `analyze` skips both cache load and save. Set by the CLI for
    /// `--sourcemap` Emit so cwd-decorated Go files are not reused across cwds.
    pub disable_cache: bool,
}

pub const PARALLEL_THRESHOLD: usize = 4;

enum LazyGoStdlibCache {
    Unloaded,
    Missing,
    Loaded(go_stdlib::GoStdlibCache),
}

impl LazyGoStdlibCache {
    fn new() -> Self {
        Self::Unloaded
    }

    fn get_or_load(&mut self, target: stdlib::Target) -> Option<&go_stdlib::GoStdlibCache> {
        if matches!(self, Self::Unloaded) {
            *self = match go_stdlib::try_load_go_stdlib_cache(target) {
                Some(cache) => Self::Loaded(cache),
                None => Self::Missing,
            };
        }
        match self {
            Self::Loaded(cache) => Some(cache),
            Self::Unloaded | Self::Missing => None,
        }
    }

    fn module_ids(&self) -> Option<HashSet<String>> {
        match self {
            Self::Loaded(cache) => Some(cache.modules.keys().cloned().collect()),
            Self::Unloaded | Self::Missing => None,
        }
    }
}

pub struct InferenceOutput {
    pub store: Store,
    pub facts: Facts,
    pub sink: LocalSink,
    pub has_pre_check_errors: bool,
    pub compiled_modules: Vec<CompiledModule>,
    pub cached_modules: HashSet<String>,
    pub cache_root: Option<PathBuf>,
    pub unreachable_modules: Vec<String>,
    pub entry_parse: EntryParseOutcome,
}

#[derive(Clone, Copy)]
enum PreludeCacheState {
    Hit,
    Miss,
}

enum CacheState<'a> {
    Disabled,
    Enabled {
        module_root: Option<&'a Path>,
        prelude: PreludeCacheState,
        go_stdlib: LazyGoStdlibCache,
    },
}

impl<'a> CacheState<'a> {
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    fn module_root(&self) -> Option<&'a Path> {
        match self {
            Self::Disabled => None,
            Self::Enabled { module_root, .. } => *module_root,
        }
    }

    fn should_save_prelude(&self) -> bool {
        matches!(
            self,
            Self::Enabled {
                prelude: PreludeCacheState::Miss,
                ..
            }
        )
    }

    fn go_stdlib_mut(&mut self) -> Option<&mut LazyGoStdlibCache> {
        match self {
            Self::Disabled => None,
            Self::Enabled { go_stdlib, .. } => Some(go_stdlib),
        }
    }

    fn go_module_ids(&self) -> Option<HashSet<String>> {
        match self {
            Self::Disabled => None,
            Self::Enabled { go_stdlib, .. } => go_stdlib.module_ids(),
        }
    }
}

/// Loads the prelude from cache when possible, else parses and registers it fresh.
fn load_prelude<'a>(
    store: &mut Store,
    sink: &LocalSink,
    cache_disabled: bool,
    module_root: Option<&'a Path>,
) -> CacheState<'a> {
    if cache_disabled {
        parse_and_register_prelude(store, sink);
        return CacheState::Disabled;
    }

    let hit = prelude_cache::try_load_prelude_cache().is_some_and(|cached| {
        prelude_cache::register_cached_prelude(store, cached);
        true
    });
    let prelude = if hit {
        PreludeCacheState::Hit
    } else {
        parse_and_register_prelude(store, sink);
        PreludeCacheState::Miss
    };
    CacheState::Enabled {
        module_root,
        prelude,
        go_stdlib: LazyGoStdlibCache::new(),
    }
}

/// Loads, registers, and infers every module, returning the artifacts the
/// post-inference passes consume. Internal, unstable API.
pub fn run_inference(input: AnalyzeInput) -> InferenceOutput {
    let mut store = Store::new();
    let sink = LocalSink::new();
    let include_tests = input.compile_phase.includes_tests();

    store.init_entry_module();
    let entry = register_entry_file(&mut store, &sink, input.entry, include_tests);
    if entry.parse.is_failed() {
        let checker = TaskState::with_sink(sink, input.project_kind);
        return InferenceOutput {
            store,
            facts: checker.facts,
            sink: checker.sink,
            has_pre_check_errors: true,
            compiled_modules: Vec::new(),
            cached_modules: HashSet::default(),
            cache_root: None,
            unreachable_modules: Vec::new(),
            entry_parse: entry.parse,
        };
    }
    if input.load_siblings {
        load_sibling_files(
            &mut store,
            &sink,
            input.loader,
            entry.filename.as_deref(),
            include_tests,
        );
    }

    let entry_module = store.entry_module_id().to_string();
    let discovered = if input.scope.has_project_root() {
        input.loader.discover_modules()
    } else {
        DiscoveredModules::default()
    };

    let roots = compute_roots(
        input.project_kind,
        input.compile_phase,
        &discovered,
        entry_module,
    );

    let graph_result = build_module_graph(
        &mut store,
        roots,
        ModuleGraphOptions {
            loader: Some(input.loader),
            sink: &sink,
            scope: &input.scope,
            project_kind: input.project_kind,
            locator: input.locator,
            include_tests,
        },
    );

    for cycle in &graph_result.cycles {
        sink.push(diagnostics::module_graph::import_cycle(cycle));
    }
    let unreachable_modules = find_unreachable_modules(&discovered, &graph_result);

    let has_pre_check_errors = sink.has_errors();

    let cache_disabled = input.disable_cache || is_cache_disabled();
    let cache = load_prelude(
        &mut store,
        &sink,
        cache_disabled,
        input.scope.project_root(),
    );
    parse_and_register_test_prelude(&mut store, &sink);

    let cache_root = cache.module_root().map(Path::to_path_buf);
    let module_output = infer_all_modules(
        &mut store,
        ModuleInferenceInput {
            graph_result,
            sink,
            compile_phase: input.compile_phase,
            go_module: input.go_module,
            cache,
            locator: input.locator,
            scope: &input.scope,
            project_kind: input.project_kind,
        },
    );

    InferenceOutput {
        store,
        facts: module_output.facts,
        sink: module_output.sink,
        has_pre_check_errors,
        compiled_modules: module_output.compiled_modules,
        cached_modules: module_output.cached_modules,
        cache_root,
        unreachable_modules,
        entry_parse: entry.parse,
    }
}
