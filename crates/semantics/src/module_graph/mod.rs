pub mod kahn;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use deps::TypedefLocator;
use syntax::ast::{ImportAlias, Span};
use syntax::program::File;

use crate::diagnostics::{emit_for_declaration_status, emit_for_locator_result};
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
    let mut visited = HashSet::default();
    let mut files: HashMap<ModuleId, Vec<File>> = HashMap::default();
    let mut import_spans: HashMap<ModuleId, Span> = HashMap::default();
    let mut blank_tracker = BlankTracker::default();

    while let Some(module_id) = to_visit.pop() {
        if visited.contains(&module_id) {
            continue;
        }
        visited.insert(module_id.clone());

        let (imports_with_spans, module_files) = collect_imports(
            &module_id,
            store,
            loader,
            sink,
            standalone_mode,
            locator,
            &mut blank_tracker,
        );

        let module_exists = !module_files.is_empty()
            || store.has(&module_id)
            || module_id == entry_module
            || module_id.starts_with("go:"); // go modules are virtual

        if !module_exists {
            if let Some(span) = import_spans.get(&module_id) {
                let is_go_stdlib =
                    stdlib::get_go_stdlib_typedef(&module_id, locator.target()).is_some();

                let src_prefix_hint = module_id
                    .strip_prefix("src/")
                    .filter(|stripped| {
                        loader.is_some_and(|fs| !fs.scan_folder(stripped).is_empty())
                    })
                    .map(String::from);

                sink.push(diagnostics::module_graph::module_not_found(
                    &module_id,
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

        edges.insert(module_id, imports);
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

fn parse_module_files(
    module_id: &ModuleId,
    store: &mut Store,
    loader: Option<&dyn Loader>,
    sink: &LocalSink,
) -> Vec<File> {
    let Some(fs) = loader else {
        return vec![];
    };
    let mut files = Vec::new();
    for (filename, source) in fs.scan_folder(module_id) {
        if filename.ends_with("_test.lis") {
            sink.push(diagnostics::module_graph::test_file_not_supported(
                &filename,
            ));
            continue;
        }
        // Ensure the module exists in the store before adding the first file
        if files.is_empty() {
            store.add_module(module_id);
        }
        let file_id = store.new_file_id();
        let result = syntax::build_ast(&source, file_id);
        sink.extend_parse_errors(result.errors);
        let file = File::new(module_id, &filename, &source, result.ast, file_id);
        // Register the file immediately so diagnostic rendering works
        store.store_file(module_id, file.clone());
        files.push(file);
    }
    files
}

fn collect_imports(
    module_id: &ModuleId,
    store: &mut Store,
    loader: Option<&dyn Loader>,
    sink: &LocalSink,
    standalone_mode: bool,
    locator: &TypedefLocator,
    blank_tracker: &mut BlankTracker,
) -> (HashMap<ModuleId, Span>, Vec<File>) {
    let mut imports = HashMap::default();
    let mut seen_go_imports: HashMap<String, SeenLookup> = HashMap::default();

    let (files, file_imports): (Vec<File>, Vec<_>) =
        if let Some(module) = store.get_module(module_id) {
            // Module already in store (entry module or prelude): get imports from stored files
            let lis_imports = module.files.values().flat_map(|f| f.imports());
            let typedef_imports = module.all_typedefs().flat_map(|f| f.imports());
            let all_imports: Vec<_> = lis_imports.chain(typedef_imports).collect();
            (vec![], all_imports)
        } else {
            // Module not in store: parse from filesystem
            let parsed = parse_module_files(module_id, store, loader, sink);
            let file_imports = parsed.iter().flat_map(|f| f.imports()).collect();
            (parsed, file_imports)
        };

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

        if file_import.name.contains('.') {
            if locator.is_declared_go_dep(&file_import.name) {
                sink.push(diagnostics::module_graph::missing_go_prefix(
                    &file_import.name,
                    file_import.name_span,
                ));
            } else {
                sink.push(diagnostics::module_graph::invalid_module_path(
                    &file_import.name,
                    file_import.name_span,
                ));
            }
            continue;
        }

        imports
            .entry(file_import.name.to_string())
            .or_insert(file_import.name_span);
    }

    (imports, files)
}
