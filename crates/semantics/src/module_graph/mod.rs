pub mod kahn;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use deps::TypedefLocator;
use syntax::ast::{ImportAlias, Span};
use syntax::program::File;

use crate::diagnostics::{emit_for_declaration_status, emit_for_locator_result};
use crate::loader as semantics_loader;
use crate::loader::Loader;
use crate::store::Store;
use diagnostics::LocalSink;

pub type ModuleId = String;

#[derive(Debug)]
pub struct ModuleGraphResult {
    pub order: Vec<ModuleId>,
    pub cycles: Vec<Vec<ModuleId>>,
    pub files: HashMap<ModuleId, Vec<File>>,
    /// Direct dependencies of each module (module_id -> set of dependency module_ids).
    /// Used for transitive cache invalidation.
    pub edges: HashMap<ModuleId, HashSet<ModuleId>>,
    /// `go:` modules that are only ever blank-imported in the visited file set.
    pub link_only_modules: HashSet<ModuleId>,
}

pub fn build_module_graph(
    store: &mut Store,
    loader: Option<&dyn Loader>,
    entry_module: &str,
    sink: &LocalSink,
    standalone_mode: bool,
    locator: &TypedefLocator,
) -> ModuleGraphResult {
    let mut edges: HashMap<ModuleId, HashSet<ModuleId>> = HashMap::default();
    let mut to_visit = vec![entry_module.to_string()];
    let mut visited: HashSet<ModuleId> = HashSet::default();
    let mut files: HashMap<ModuleId, Vec<File>> = HashMap::default();
    let mut import_spans: HashMap<ModuleId, Span> = HashMap::default();
    let mut blank_tracker = BlankTracker::default();

    while !to_visit.is_empty() {
        let drained: Vec<ModuleId> = std::mem::take(&mut to_visit);
        let mut batch: Vec<ModuleId> = Vec::with_capacity(drained.len());
        for module_id in drained {
            if visited.insert(module_id.clone()) {
                batch.push(module_id);
            }
        }
        if batch.is_empty() {
            continue;
        }

        batch.sort();

        let mut parsed = batch_parse_modules(&batch, store, loader, sink);

        for module_id in &batch {
            let module_files = parsed.remove(module_id).unwrap_or_default();
            let file_imports: Vec<_> = if !module_files.is_empty() {
                module_files.iter().flat_map(|f| f.imports()).collect()
            } else if let Some(module) = store.get_module(module_id) {
                module.all_imports()
            } else {
                Vec::new()
            };
            let imports_with_spans = process_file_imports(
                file_imports,
                sink,
                standalone_mode,
                locator,
                &mut blank_tracker,
            );

            let module_exists = !module_files.is_empty()
                || store.has(module_id)
                || module_id == entry_module
                || module_id.starts_with("go:");

            if !module_exists {
                if let Some(span) = import_spans.get(module_id) {
                    let is_go_stdlib =
                        stdlib::get_go_stdlib_typedef(module_id, locator.target()).is_some();

                    let src_prefix_hint = module_id
                        .strip_prefix("src/")
                        .filter(|stripped| {
                            loader.is_some_and(|fs| !fs.scan_folder(stripped).is_empty())
                        })
                        .map(String::from);

                    sink.push(diagnostics::module_graph::module_not_found(
                        module_id,
                        *span,
                        is_go_stdlib,
                        standalone_mode,
                        src_prefix_hint,
                    ));
                }
                continue;
            }

            files.insert(module_id.clone(), module_files);

            let imports: HashSet<_> = imports_with_spans.keys().cloned().collect();

            for (import, span) in imports_with_spans {
                if !visited.contains(&import) {
                    to_visit.push(import.clone());
                }
                import_spans.entry(import).or_insert(span);
            }

            edges.insert(module_id.clone(), imports);
        }
    }

    let (order, cycles) = kahn::topological_sort(&edges);

    ModuleGraphResult {
        order,
        cycles,
        files,
        edges,
        link_only_modules: blank_tracker.into_link_only_modules(),
    }
}

#[derive(Clone, Copy)]
enum SeenLookup {
    OkBlank,
    OkNonBlank,
    Errored,
}

#[derive(Default)]
struct BlankTracker {
    blank: HashSet<ModuleId>,
    non_blank: HashSet<ModuleId>,
}

impl BlankTracker {
    fn record_blank(&mut self, module_id: &str) {
        self.blank.insert(module_id.to_string());
    }

    fn record_non_blank(&mut self, module_id: &str) {
        self.non_blank.insert(module_id.to_string());
    }

    fn into_link_only_modules(self) -> HashSet<ModuleId> {
        self.blank.difference(&self.non_blank).cloned().collect()
    }
}

struct ParseJob {
    module_id: ModuleId,
    file_id: u32,
    filename: String,
    display_path: String,
    source: String,
}

fn batch_parse_modules(
    modules: &[ModuleId],
    store: &Store,
    loader: Option<&dyn Loader>,
    sink: &LocalSink,
) -> HashMap<ModuleId, Vec<File>> {
    let Some(fs) = loader else {
        return HashMap::default();
    };

    let mut jobs: Vec<ParseJob> = Vec::new();
    for module_id in modules {
        if store.has(module_id) {
            continue;
        }
        let mut entries: Vec<(String, semantics_loader::FileContent)> =
            fs.scan_folder(module_id).into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (filename, content) in entries {
            if filename.ends_with("_test.lis") {
                sink.push(diagnostics::module_graph::test_file_not_supported(
                    &content.display_path,
                ));
                continue;
            }
            let file_id = store.new_file_id();
            jobs.push(ParseJob {
                module_id: module_id.clone(),
                file_id,
                filename,
                display_path: content.display_path,
                source: content.source,
            });
        }
    }

    const PARALLEL_THRESHOLD: usize = 4;
    let parsed: Vec<(ModuleId, File, Vec<syntax::ParseError>)> = if jobs.len() < PARALLEL_THRESHOLD
    {
        jobs.into_iter().map(parse_one).collect()
    } else {
        use rayon::prelude::*;
        jobs.into_par_iter().map(parse_one).collect()
    };

    let mut grouped: HashMap<ModuleId, Vec<File>> = HashMap::default();
    for (module_id, file, errors) in parsed {
        sink.extend_parse_errors(errors);
        grouped.entry(module_id).or_default().push(file);
    }
    grouped
}

fn parse_one(job: ParseJob) -> (ModuleId, File, Vec<syntax::ParseError>) {
    let result = syntax::build_ast(&job.source, job.file_id);
    let file = File::new(
        &job.module_id,
        &job.filename,
        &job.display_path,
        &job.source,
        result.ast,
        job.file_id,
    );
    (job.module_id, file, result.errors)
}

fn process_file_imports(
    file_imports: Vec<syntax::program::FileImport>,
    sink: &LocalSink,
    standalone_mode: bool,
    locator: &TypedefLocator,
    blank_tracker: &mut BlankTracker,
) -> HashMap<ModuleId, Span> {
    let mut imports = HashMap::default();
    let mut seen_go_imports: HashMap<String, SeenLookup> = HashMap::default();

    for file_import in file_imports {
        if file_import.name == "prelude" {
            sink.push(diagnostics::module_graph::cannot_import_prelude(
                file_import.span,
            ));
            continue;
        }

        if let Some(go_pkg) = file_import.name.strip_prefix("go:") {
            let is_blank = matches!(file_import.alias, Some(ImportAlias::Blank(_)));

            let prior = seen_go_imports.get(file_import.name.as_str()).copied();
            let needs_lookup = match (prior, is_blank) {
                (None, _) => true,
                (Some(SeenLookup::Errored), _) => false,
                (Some(SeenLookup::OkNonBlank), _) => false,
                (Some(SeenLookup::OkBlank), true) => false,
                (Some(SeenLookup::OkBlank), false) => true,
            };

            if !needs_lookup {
                if matches!(prior, Some(SeenLookup::OkBlank | SeenLookup::OkNonBlank)) {
                    let module_key = file_import.name.to_string();
                    if is_blank {
                        blank_tracker.record_blank(&module_key);
                    } else {
                        blank_tracker.record_non_blank(&module_key);
                    }
                    imports.insert(module_key, file_import.name_span);
                }
                continue;
            }

            let ok = if is_blank {
                let status = locator.validate_declaration(go_pkg);
                emit_for_declaration_status(
                    &status,
                    &file_import.name,
                    go_pkg,
                    file_import.name_span,
                    locator.target(),
                    standalone_mode,
                    sink,
                )
            } else {
                let result = locator.find_typedef_content(go_pkg);
                emit_for_locator_result(
                    &result,
                    &file_import.name,
                    go_pkg,
                    Some(file_import.name_span),
                    locator.target(),
                    standalone_mode,
                    sink,
                )
            };

            seen_go_imports.insert(
                file_import.name.to_string(),
                if !ok {
                    SeenLookup::Errored
                } else if is_blank {
                    SeenLookup::OkBlank
                } else {
                    SeenLookup::OkNonBlank
                },
            );
            if ok {
                let module_key = file_import.name.to_string();
                if is_blank {
                    blank_tracker.record_blank(&module_key);
                } else {
                    blank_tracker.record_non_blank(&module_key);
                }
                imports.insert(module_key, file_import.name_span);
            }
            continue;
        }

        let blank_span = match &file_import.alias {
            Some(ImportAlias::Blank(span)) => Some(*span),
            _ => None,
        };
        let is_dotted = file_import.name.contains('.');

        if is_dotted && locator.is_declared_go_dep(&file_import.name) {
            sink.push(diagnostics::module_graph::missing_go_prefix(
                &file_import.name,
                file_import.name_span,
                blank_span.is_some(),
            ));
            continue;
        }

        if is_dotted {
            sink.push(diagnostics::module_graph::invalid_module_path(
                &file_import.name,
                file_import.name_span,
            ));
        }
        if let Some(span) = blank_span {
            sink.push(diagnostics::infer::blank_import_non_go(span));
        }
        if is_dotted || blank_span.is_some() {
            continue;
        }

        imports
            .entry(file_import.name.to_string())
            .or_insert(file_import.name_span);
    }

    imports
}
