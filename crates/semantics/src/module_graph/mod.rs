pub mod kahn;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use deps::TypedefLocator;
use syntax::ast::{ImportAlias, Span};
use syntax::program::File;

use crate::diagnostics::{GoImportSite, emit_for_declaration_status, emit_for_locator_result};
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
    /// Direct dependencies of each module, test-file imports included. Drives
    /// reachability, topological order, and a module's own cache validity.
    pub edges: HashMap<ModuleId, HashSet<ModuleId>>,
    /// `edges` minus imports that appear only in `.test.lis` files. Drives the
    /// `module_hash` propagated to dependents, so a test-only import never
    /// invalidates production importers.
    pub production_edges: HashMap<ModuleId, HashSet<ModuleId>>,
    /// `go:` modules that are only ever blank-imported in the visited file set.
    pub(crate) link_only_modules: HashSet<ModuleId>,
    /// Reachable from the primary roots, snapshotted before `additional` runs.
    pub primary_reachable: HashSet<ModuleId>,
}

/// `primary` defines the target. `additional` widens what is analyzed.
#[derive(Debug, Default)]
pub struct Roots {
    pub primary: Vec<ModuleId>,
    pub additional: Vec<ModuleId>,
}

pub struct ModuleGraphOptions<'a> {
    pub loader: Option<&'a dyn Loader>,
    pub sink: &'a LocalSink,
    pub standalone_mode: bool,
    pub locator: &'a TypedefLocator,
    pub include_tests: bool,
}

pub fn build_module_graph(
    store: &mut Store,
    roots: Roots,
    options: ModuleGraphOptions<'_>,
) -> ModuleGraphResult {
    let Roots {
        primary,
        additional,
    } = roots;
    let ModuleGraphOptions {
        loader,
        sink,
        standalone_mode,
        locator,
        include_tests,
    } = options;
    let mut builder = GraphBuilder {
        store,
        loader,
        sink,
        standalone_mode,
        locator,
        include_tests,
        edges: HashMap::default(),
        production_edges: HashMap::default(),
        visited: HashSet::default(),
        files: HashMap::default(),
        import_spans: HashMap::default(),
        blank_tracker: BlankTracker::default(),
    };
    builder.visit(primary);
    let primary_reachable = builder.visited.clone();
    builder.visit(additional);
    builder.finish(primary_reachable)
}

struct GraphBuilder<'a> {
    store: &'a mut Store,
    loader: Option<&'a dyn Loader>,
    sink: &'a LocalSink,
    standalone_mode: bool,
    locator: &'a TypedefLocator,
    include_tests: bool,
    edges: HashMap<ModuleId, HashSet<ModuleId>>,
    production_edges: HashMap<ModuleId, HashSet<ModuleId>>,
    visited: HashSet<ModuleId>,
    files: HashMap<ModuleId, Vec<File>>,
    import_spans: HashMap<ModuleId, Span>,
    blank_tracker: BlankTracker,
}

impl<'a> GraphBuilder<'a> {
    fn visit(&mut self, mut to_visit: Vec<ModuleId>) {
        while !to_visit.is_empty() {
            let drained: Vec<ModuleId> = std::mem::take(&mut to_visit);
            let mut batch: Vec<ModuleId> = Vec::with_capacity(drained.len());
            for module_id in drained {
                if self.visited.insert(module_id.clone()) {
                    batch.push(module_id);
                }
            }
            if batch.is_empty() {
                continue;
            }

            batch.sort();

            let mut parsed = batch_parse_modules(
                &batch,
                self.store,
                self.loader,
                self.sink,
                self.include_tests,
            );

            for module_id in &batch {
                let module_files = parsed.remove(module_id).unwrap_or_default();
                let file_imports: Vec<_> = if !module_files.is_empty() {
                    module_files.iter().flat_map(|f| f.imports()).collect()
                } else if let Some(module) = self.store.get_module(module_id) {
                    module.all_imports()
                } else {
                    Vec::new()
                };
                let has_parsed_files = !module_files.is_empty();
                let production_import_names: HashSet<String> = module_files
                    .iter()
                    .filter(|f| !f.is_test())
                    .flat_map(|f| f.imports())
                    .map(|import| import.name.to_string())
                    .collect();
                let imports_with_spans = process_file_imports(
                    file_imports,
                    self.sink,
                    self.standalone_mode,
                    self.locator,
                    &mut self.blank_tracker,
                );

                let has_production_file = module_files.iter().any(|file| !file.is_test());
                let module_exists = has_production_file
                    || self.store.has(module_id)
                    || module_id.starts_with("go:");

                if !module_exists {
                    if let Some(span) = self.import_spans.get(module_id) {
                        let is_go_stdlib =
                            stdlib::get_go_stdlib_typedef(module_id, self.locator.target())
                                .is_some();

                        let src_prefix_hint = module_id
                            .strip_prefix("src/")
                            .filter(|stripped| {
                                self.loader
                                    .is_some_and(|fs| !fs.scan_folder(stripped).is_empty())
                            })
                            .map(String::from);

                        let reason = if let Some(stripped) = src_prefix_hint {
                            diagnostics::module_graph::MissingModuleReason::UnnecessarySrcPrefix(
                                stripped,
                            )
                        } else if is_go_stdlib {
                            diagnostics::module_graph::MissingModuleReason::GoStandardLibrary
                        } else if self.standalone_mode {
                            diagnostics::module_graph::MissingModuleReason::Standalone
                        } else {
                            diagnostics::module_graph::MissingModuleReason::NotFound
                        };
                        self.sink.push(diagnostics::module_graph::module_not_found(
                            module_id, *span, reason,
                        ));
                    }
                    continue;
                }

                self.files.insert(module_id.clone(), module_files);

                let imports: HashSet<_> = imports_with_spans.keys().cloned().collect();

                for (import, span) in imports_with_spans {
                    if !self.visited.contains(&import) {
                        to_visit.push(import.clone());
                    }
                    self.import_spans.entry(import).or_insert(span);
                }

                let production_edge_set: HashSet<ModuleId> = if has_parsed_files {
                    imports
                        .iter()
                        .filter(|import| production_import_names.contains(import.as_str()))
                        .cloned()
                        .collect()
                } else {
                    imports.clone()
                };
                self.production_edges
                    .insert(module_id.clone(), production_edge_set);
                self.edges.insert(module_id.clone(), imports);
            }
        }
    }

    fn finish(self, primary_reachable: HashSet<ModuleId>) -> ModuleGraphResult {
        let (order, cycles) = kahn::topological_sort(&self.edges);

        ModuleGraphResult {
            order,
            cycles,
            files: self.files,
            edges: self.edges,
            production_edges: self.production_edges,
            link_only_modules: self.blank_tracker.into_link_only_modules(),
            primary_reachable,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportUse {
    LinkOnly,
    Referenced,
}

#[derive(Default)]
struct BlankTracker {
    modules: HashMap<ModuleId, ImportUse>,
}

impl BlankTracker {
    fn record(&mut self, module_id: &str, is_blank: bool) {
        let use_kind = if is_blank {
            ImportUse::LinkOnly
        } else {
            ImportUse::Referenced
        };
        self.modules
            .entry(module_id.to_string())
            .and_modify(|prior| {
                if use_kind == ImportUse::Referenced {
                    *prior = ImportUse::Referenced;
                }
            })
            .or_insert(use_kind);
    }

    fn into_link_only_modules(self) -> HashSet<ModuleId> {
        self.modules
            .into_iter()
            .filter_map(|(module_id, use_kind)| {
                (use_kind == ImportUse::LinkOnly).then_some(module_id)
            })
            .collect()
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
    include_tests: bool,
) -> HashMap<ModuleId, Vec<File>> {
    let Some(fs) = loader else {
        return HashMap::default();
    };

    const PARALLEL_THRESHOLD: usize = 4;

    let to_scan: Vec<&ModuleId> = modules.iter().filter(|m| !store.has(m)).collect();
    let scanned: Vec<(&ModuleId, Vec<(String, semantics_loader::FileContent)>)> =
        if to_scan.len() < PARALLEL_THRESHOLD {
            to_scan.into_iter().map(|m| (m, scan_one(fs, m))).collect()
        } else {
            use rayon::prelude::*;
            to_scan
                .into_par_iter()
                .map(|m| (m, scan_one(fs, m)))
                .collect()
        };

    let mut jobs: Vec<ParseJob> = Vec::new();
    for (module_id, entries) in scanned {
        for (filename, content) in entries {
            if filename.ends_with("_test.lis") {
                sink.push(diagnostics::module_graph::wrong_test_file_suffix(
                    &content.display_path,
                ));
                continue;
            }
            if filename.ends_with(".test.lis") && !include_tests {
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

fn scan_one(fs: &dyn Loader, module_id: &str) -> Vec<(String, semantics_loader::FileContent)> {
    let mut entries: Vec<(String, semantics_loader::FileContent)> =
        fs.scan_folder(module_id).into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn parse_one(job: ParseJob) -> (ModuleId, File, Vec<syntax::ParseError>) {
    let result = syntax::build_ast(&job.source, job.file_id);
    let file = File::new(
        &job.module_id,
        &job.filename,
        &job.display_path,
        &job.source,
        result.ast,
        result.file_comment,
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
    let referenced_go_imports: HashSet<&str> = file_imports
        .iter()
        .filter(|import| {
            import.name.starts_with("go:") && !matches!(import.alias, Some(ImportAlias::Blank(_)))
        })
        .map(|import| import.name.as_str())
        .collect();
    let mut go_import_results: HashMap<&str, bool> = HashMap::default();

    for file_import in &file_imports {
        if file_import.name == "prelude" {
            sink.push(diagnostics::module_graph::cannot_import_prelude(
                file_import.span,
            ));
            continue;
        }

        if file_import.name.starts_with("**") {
            sink.push(diagnostics::module_graph::reserved_module_import(
                file_import.span,
            ));
            continue;
        }

        if let Some(go_pkg) = file_import.name.strip_prefix("go:") {
            let is_blank = matches!(file_import.alias, Some(ImportAlias::Blank(_)));
            let ok = *go_import_results
                .entry(file_import.name.as_str())
                .or_insert_with(|| {
                    if referenced_go_imports.contains(file_import.name.as_str()) {
                        let result = locator.find_typedef_content(go_pkg);
                        emit_for_locator_result(
                            &result,
                            &GoImportSite {
                                import_name: &file_import.name,
                                go_pkg,
                                name_span: Some(file_import.name_span),
                                target: locator.target(),
                                standalone_mode,
                                replace_importer: None,
                            },
                            sink,
                        )
                    } else {
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
                    }
                });
            if ok {
                blank_tracker.record(&file_import.name, is_blank);
                imports
                    .entry(file_import.name.to_string())
                    .or_insert(file_import.name_span);
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

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::program::FileImport;

    fn go_import(is_blank: bool, offset: u32) -> FileImport {
        let span = Span::new(0, offset, 1);
        FileImport {
            name: "go:fmt".into(),
            name_span: span,
            alias: is_blank.then_some(ImportAlias::Blank(span)),
            span,
        }
    }

    fn is_link_only(imports: Vec<FileImport>) -> bool {
        let sink = LocalSink::new();
        let mut tracker = BlankTracker::default();
        let resolved = process_file_imports(
            imports,
            &sink,
            false,
            &TypedefLocator::default(),
            &mut tracker,
        );

        assert!(!sink.has_errors());
        assert!(resolved.contains_key("go:fmt"));
        tracker.into_link_only_modules().contains("go:fmt")
    }

    #[test]
    fn blank_only_import_is_link_only() {
        assert!(is_link_only(vec![go_import(true, 0)]));
    }

    #[test]
    fn referenced_import_wins_regardless_of_order() {
        assert!(!is_link_only(
            vec![go_import(true, 0), go_import(false, 1),]
        ));
        assert!(!is_link_only(
            vec![go_import(false, 0), go_import(true, 1),]
        ));
    }
}
