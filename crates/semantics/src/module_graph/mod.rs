pub mod kahn;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use deps::TypedefLocator;
use syntax::ast::{ImportAlias, Span};
use syntax::program::File;

use crate::diagnostics::{GoImportSite, emit_for_declaration_status, emit_for_locator_result};
use crate::inference::ProjectKind;
use crate::loader as semantics_loader;
use crate::loader::Loader;
use crate::store::{ENTRY_MODULE_ID, Store};
use diagnostics::LocalSink;

pub type ModuleId = String;

pub fn root_import_target(name: &str, importer: &str, kind: ProjectKind) -> Option<&'static str> {
    (name == semantics_loader::ROOT_IMPORT
        && kind == ProjectKind::Library
        && semantics_loader::is_external_test_module(importer))
    .then_some(ENTRY_MODULE_ID)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyKind {
    Production,
    TestOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportUse {
    LinkOnly,
    Referenced,
}

impl ImportUse {
    fn merge(self, other: Self) -> Self {
        if self == Self::Referenced || other == Self::Referenced {
            Self::Referenced
        } else {
            Self::LinkOnly
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Dependency {
    kind: DependencyKind,
    usage: ImportUse,
}

/// One canonical classification for every direct module dependency.
#[derive(Debug, Default)]
pub struct DependencyGraph {
    edges: HashMap<ModuleId, HashMap<ModuleId, Dependency>>,
}

impl DependencyGraph {
    pub fn contains_module(&self, module_id: &str) -> bool {
        self.edges.contains_key(module_id)
    }

    pub fn contains_dependency(&self, module_id: &str, dependency: &str) -> bool {
        self.edges
            .get(module_id)
            .is_some_and(|dependencies| dependencies.contains_key(dependency))
    }

    pub fn contains_production_dependency(&self, module_id: &str, dependency: &str) -> bool {
        matches!(
            self.edges
                .get(module_id)
                .and_then(|dependencies| dependencies.get(dependency))
                .map(|dependency| dependency.kind),
            Some(DependencyKind::Production)
        )
    }

    pub(crate) fn modules(&self) -> impl Iterator<Item = &ModuleId> {
        self.edges.keys()
    }

    pub(crate) fn dependencies(&self, module_id: &str) -> impl Iterator<Item = &ModuleId> {
        self.edges
            .get(module_id)
            .into_iter()
            .flat_map(HashMap::keys)
    }

    pub(crate) fn production_dependencies(
        &self,
        module_id: &str,
    ) -> impl Iterator<Item = &ModuleId> {
        self.edges
            .get(module_id)
            .into_iter()
            .flat_map(|dependencies| {
                dependencies.iter().filter_map(|(module_id, dependency)| {
                    (dependency.kind == DependencyKind::Production).then_some(module_id)
                })
            })
    }

    pub(crate) fn is_link_only_module(&self, module_id: &str) -> bool {
        let mut uses = self
            .edges
            .values()
            .filter_map(|dependencies| dependencies.get(module_id))
            .map(|dependency| dependency.usage);
        matches!(uses.next(), Some(ImportUse::LinkOnly))
            && uses.all(|usage| usage == ImportUse::LinkOnly)
    }

    pub(crate) fn len(&self) -> usize {
        self.edges.len()
    }

    fn insert(&mut self, module_id: ModuleId, dependencies: HashMap<ModuleId, Dependency>) {
        self.edges.insert(module_id, dependencies);
    }
}

impl From<HashMap<ModuleId, HashSet<ModuleId>>> for DependencyGraph {
    fn from(edges: HashMap<ModuleId, HashSet<ModuleId>>) -> Self {
        Self {
            edges: edges
                .into_iter()
                .map(|(module_id, dependencies)| {
                    let dependencies = dependencies
                        .into_iter()
                        .map(|dependency| {
                            (
                                dependency,
                                Dependency {
                                    kind: DependencyKind::Production,
                                    usage: ImportUse::Referenced,
                                },
                            )
                        })
                        .collect();
                    (module_id, dependencies)
                })
                .collect(),
        }
    }
}

#[derive(Debug)]
pub struct ModuleGraphResult {
    pub order: Vec<ModuleId>,
    pub cycles: Vec<Vec<ModuleId>>,
    pub files: HashMap<ModuleId, Vec<File>>,
    pub dependencies: DependencyGraph,
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
    pub has_project_root: bool,
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
        has_project_root,
        locator,
        include_tests,
    } = options;
    let mut builder = GraphBuilder {
        store,
        loader,
        sink,
        standalone_mode,
        has_project_root,
        locator,
        include_tests,
        dependencies: DependencyGraph::default(),
        visited: HashSet::default(),
        files: HashMap::default(),
        import_spans: HashMap::default(),
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
    has_project_root: bool,
    locator: &'a TypedefLocator,
    include_tests: bool,
    dependencies: DependencyGraph,
    visited: HashSet<ModuleId>,
    files: HashMap<ModuleId, Vec<File>>,
    import_spans: HashMap<ModuleId, Span>,
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
                self.has_project_root,
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
                let root_has_production = self
                    .files
                    .get(ENTRY_MODULE_ID)
                    .is_some_and(|files| files.iter().any(|f| !f.is_test() && !f.is_d_lis()))
                    || self
                        .store
                        .get_module(ENTRY_MODULE_ID)
                        .is_some_and(|m| m.files.values().any(|f| !f.is_test()));
                let imports = process_file_imports(
                    file_imports,
                    ImportContext {
                        sink: self.sink,
                        standalone_mode: self.standalone_mode,
                        has_project_root: self.has_project_root,
                        root_has_production,
                        importer: module_id,
                        project_kind: self.store.project_kind,
                        locator: self.locator,
                    },
                );

                let has_production_file = module_files.iter().any(|file| !file.is_test());
                let module_exists = has_production_file
                    || self.store.has(module_id)
                    || module_id.starts_with("go:")
                    || (self.has_project_root
                        && semantics_loader::is_external_test_module(module_id));

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

                for (import, resolved) in &imports {
                    if !self.visited.contains(import) {
                        to_visit.push(import.clone());
                    }
                    self.import_spans
                        .entry(import.clone())
                        .or_insert(resolved.span);
                }

                let dependencies: HashMap<ModuleId, Dependency> = if has_parsed_files {
                    imports
                        .into_iter()
                        .map(|(import, resolved)| {
                            let kind = if production_import_names.contains(import.as_str()) {
                                DependencyKind::Production
                            } else {
                                DependencyKind::TestOnly
                            };
                            (
                                import,
                                Dependency {
                                    kind,
                                    usage: resolved.usage,
                                },
                            )
                        })
                        .collect()
                } else {
                    imports
                        .into_iter()
                        .map(|(import, resolved)| {
                            (
                                import,
                                Dependency {
                                    kind: DependencyKind::Production,
                                    usage: resolved.usage,
                                },
                            )
                        })
                        .collect()
                };
                self.dependencies.insert(module_id.clone(), dependencies);
            }
        }
    }

    fn finish(self, primary_reachable: HashSet<ModuleId>) -> ModuleGraphResult {
        let (order, cycles) = kahn::topological_sort(&self.dependencies);

        ModuleGraphResult {
            order,
            cycles,
            files: self.files,
            dependencies: self.dependencies,
            primary_reachable,
        }
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
    has_project_root: bool,
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
        let is_external_test =
            has_project_root && semantics_loader::is_external_test_module(module_id);
        for (filename, content) in entries {
            if is_external_test {
                match semantics_loader::external_test_file_issue(&filename) {
                    Some(semantics_loader::ExternalTestFileIssue::WrongSuffix) => {
                        sink.push(diagnostics::module_graph::wrong_test_file_suffix(
                            &content.display_path,
                        ));
                        continue;
                    }
                    Some(semantics_loader::ExternalTestFileIssue::NotATestFile) => {
                        sink.push(diagnostics::module_graph::non_test_file_under_tests(
                            &content.display_path,
                        ));
                        continue;
                    }
                    None => {}
                }
            } else if filename.ends_with("_test.lis") {
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

    let parsed: Vec<(File, Vec<syntax::ParseError>)> = if jobs.len() < PARALLEL_THRESHOLD {
        jobs.into_iter().map(parse_one).collect()
    } else {
        use rayon::prelude::*;
        jobs.into_par_iter().map(parse_one).collect()
    };

    let mut grouped: HashMap<ModuleId, Vec<File>> = HashMap::default();
    for (file, errors) in parsed {
        sink.extend_parse_errors(errors);
        grouped
            .entry(file.module_id.clone())
            .or_default()
            .push(file);
    }
    grouped
}

fn scan_one(fs: &dyn Loader, module_id: &str) -> Vec<(String, semantics_loader::FileContent)> {
    let mut entries: Vec<(String, semantics_loader::FileContent)> =
        fs.scan_folder(module_id).into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn parse_one(job: ParseJob) -> (File, Vec<syntax::ParseError>) {
    let result = syntax::build_ast(&job.source, job.file_id);
    let file = File {
        id: job.file_id,
        module_id: job.module_id,
        name: job.filename,
        display_path: job.display_path,
        source_path: None,
        source: job.source,
        items: result.ast,
        file_comment: result.file_comment,
    };
    (file, result.errors)
}

#[derive(Clone, Copy)]
struct ResolvedImport {
    span: Span,
    usage: ImportUse,
}

#[derive(Clone, Copy)]
struct ImportContext<'a> {
    sink: &'a LocalSink,
    standalone_mode: bool,
    has_project_root: bool,
    root_has_production: bool,
    importer: &'a str,
    project_kind: ProjectKind,
    locator: &'a TypedefLocator,
}

fn process_file_imports(
    file_imports: Vec<syntax::program::FileImport>,
    ctx: ImportContext<'_>,
) -> HashMap<ModuleId, ResolvedImport> {
    let ImportContext {
        sink,
        standalone_mode,
        has_project_root,
        root_has_production,
        importer,
        project_kind,
        locator,
    } = ctx;
    let mut imports = HashMap::default();
    let mut pending_go_imports: HashMap<&str, ResolvedImport> = HashMap::default();
    for file_import in &file_imports {
        if !file_import.name.starts_with("go:") {
            continue;
        }
        let usage = if matches!(file_import.alias, Some(ImportAlias::Blank(_))) {
            ImportUse::LinkOnly
        } else {
            ImportUse::Referenced
        };
        pending_go_imports
            .entry(&file_import.name)
            .and_modify(|pending| pending.usage = pending.usage.merge(usage))
            .or_insert(ResolvedImport {
                span: file_import.name_span,
                usage,
            });
    }

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

        if file_import.name == ENTRY_MODULE_ID {
            sink.push(diagnostics::module_graph::cannot_import_entry(
                file_import.name_span,
            ));
            continue;
        }

        if has_project_root && semantics_loader::is_external_test_module(&file_import.name) {
            sink.push(diagnostics::module_graph::cannot_import_external_tests(
                file_import.name_span,
            ));
            continue;
        }

        if has_project_root && file_import.name == semantics_loader::ROOT_IMPORT {
            if let Some(ImportAlias::Blank(span)) = &file_import.alias {
                sink.push(diagnostics::infer::blank_import_non_go(*span));
            } else {
                match root_import_target(&file_import.name, importer, project_kind) {
                    Some(_) if !root_has_production => {
                        sink.push(
                            diagnostics::module_graph::cannot_import_root_without_source(
                                file_import.name_span,
                            ),
                        );
                    }
                    Some(entry) => {
                        imports.entry(entry.to_string()).or_insert(ResolvedImport {
                            span: file_import.name_span,
                            usage: ImportUse::Referenced,
                        });
                    }
                    None if project_kind == ProjectKind::Binary => {
                        sink.push(diagnostics::module_graph::cannot_import_root_in_binary(
                            file_import.name_span,
                        ));
                    }
                    None => {
                        sink.push(diagnostics::module_graph::cannot_import_root_from_src(
                            file_import.name_span,
                        ));
                    }
                }
            }
            continue;
        }

        if let Some(go_pkg) = file_import.name.strip_prefix("go:") {
            let Some(pending) = pending_go_imports.remove(file_import.name.as_str()) else {
                continue;
            };
            let ok = match pending.usage {
                ImportUse::Referenced => {
                    let result = locator.find_typedef_content(go_pkg);
                    emit_for_locator_result(
                        &result,
                        &GoImportSite {
                            import_name: &file_import.name,
                            go_pkg,
                            name_span: Some(pending.span),
                            target: locator.target(),
                            standalone_mode,
                            replace_importer: None,
                        },
                        sink,
                    )
                }
                ImportUse::LinkOnly => {
                    let status = locator.validate_declaration(go_pkg);
                    emit_for_declaration_status(
                        &status,
                        &GoImportSite {
                            import_name: &file_import.name,
                            go_pkg,
                            name_span: Some(pending.span),
                            target: locator.target(),
                            standalone_mode,
                            replace_importer: None,
                        },
                        sink,
                    )
                }
            };
            if ok {
                imports.insert(
                    file_import.name.to_string(),
                    ResolvedImport {
                        span: pending.span,
                        usage: pending.usage,
                    },
                );
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
            .or_insert(ResolvedImport {
                span: file_import.name_span,
                usage: ImportUse::Referenced,
            });
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
        let resolved = process_file_imports(
            imports,
            ImportContext {
                sink: &sink,
                standalone_mode: false,
                has_project_root: false,
                root_has_production: false,
                importer: "caller",
                project_kind: ProjectKind::Binary,
                locator: &TypedefLocator::default(),
            },
        );

        assert!(!sink.has_errors());
        resolved
            .get("go:fmt")
            .is_some_and(|resolved| resolved.usage == ImportUse::LinkOnly)
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

    #[test]
    fn external_test_imports_are_rejected() {
        for name in ["tests", "tests/integration"] {
            let span = Span::new(0, 0, 1);
            let sink = LocalSink::new();
            let resolved = process_file_imports(
                vec![FileImport {
                    name: name.into(),
                    name_span: span,
                    alias: None,
                    span,
                }],
                ImportContext {
                    sink: &sink,
                    standalone_mode: false,
                    has_project_root: true,
                    root_has_production: true,
                    importer: "caller",
                    project_kind: ProjectKind::Binary,
                    locator: &TypedefLocator::default(),
                },
            );

            assert!(sink.has_errors(), "`import \"{name}\"` should be rejected");
            assert!(resolved.is_empty());
        }
    }

    #[test]
    fn external_test_reservation_is_project_only() {
        let span = Span::new(0, 0, 1);
        let sink = LocalSink::new();
        let resolved = process_file_imports(
            vec![FileImport {
                name: "tests".into(),
                name_span: span,
                alias: None,
                span,
            }],
            ImportContext {
                sink: &sink,
                standalone_mode: false,
                has_project_root: false,
                root_has_production: false,
                importer: "caller",
                project_kind: ProjectKind::Binary,
                locator: &TypedefLocator::default(),
            },
        );

        assert!(
            !sink.has_errors(),
            "a non-project check has no `tests/` to reserve"
        );
        assert!(resolved.contains_key("tests"));
    }

    #[test]
    fn root_import_resolves_only_for_library_external_tests() {
        assert_eq!(
            root_import_target("root", "tests", ProjectKind::Library),
            Some(ENTRY_MODULE_ID)
        );
        assert_eq!(
            root_import_target("root", "tests/integration", ProjectKind::Library),
            Some(ENTRY_MODULE_ID)
        );
        assert_eq!(
            root_import_target("root", "geometry", ProjectKind::Library),
            None,
            "src modules cannot import the root"
        );
        assert_eq!(
            root_import_target("root", "tests", ProjectKind::Binary),
            None,
            "a binary has no importable root"
        );
        assert_eq!(
            root_import_target("routes", "tests", ProjectKind::Library),
            None
        );
    }

    #[test]
    fn referenced_edge_wins_across_importing_modules() {
        let dependency = |usage| Dependency {
            kind: DependencyKind::Production,
            usage,
        };
        let mut graph = DependencyGraph::default();
        graph.insert(
            "blank_importer".into(),
            HashMap::from_iter([("go:fmt".into(), dependency(ImportUse::LinkOnly))]),
        );
        graph.insert(
            "referencing_importer".into(),
            HashMap::from_iter([("go:fmt".into(), dependency(ImportUse::Referenced))]),
        );

        assert!(!graph.is_link_only_module("go:fmt"));
    }
}
