pub mod kahn;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use deps::TypedefLocator;
use syntax::ast::{ImportAlias, Span};
use syntax::program::File;

use crate::analysis::{AnalysisScope, ProjectKind};
use crate::diagnostics::{GoImportSite, emit_for_declaration_status, emit_for_locator_result};
use crate::loader as semantics_loader;
use crate::loader::Loader;
use crate::store::{ENTRY_PACKAGE_ID, Store};
use diagnostics::LocalSink;

pub type PackageId = String;

pub fn root_import_target(name: &str, importer: &str, kind: ProjectKind) -> Option<&'static str> {
    (name == semantics_loader::ROOT_IMPORT
        && kind == ProjectKind::Library
        && semantics_loader::is_external_test_package(importer))
    .then_some(ENTRY_PACKAGE_ID)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyKind {
    Production,
    TestOnly,
}

impl DependencyKind {
    fn merge(self, other: Self) -> Self {
        if self == Self::Production || other == Self::Production {
            Self::Production
        } else {
            Self::TestOnly
        }
    }
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
    span: Span,
}

impl Dependency {
    fn merge(&mut self, other: Self) {
        self.kind = self.kind.merge(other.kind);
        self.usage = self.usage.merge(other.usage);
    }
}

/// One canonical classification for every direct package dependency.
#[derive(Debug, Default)]
pub struct DependencyGraph {
    edges: HashMap<PackageId, HashMap<PackageId, Dependency>>,
}

impl DependencyGraph {
    pub fn contains_package(&self, package_id: &str) -> bool {
        self.edges.contains_key(package_id)
    }

    pub fn contains_dependency(&self, package_id: &str, dependency: &str) -> bool {
        self.edges
            .get(package_id)
            .is_some_and(|dependencies| dependencies.contains_key(dependency))
    }

    pub fn contains_production_dependency(&self, package_id: &str, dependency: &str) -> bool {
        matches!(
            self.edges
                .get(package_id)
                .and_then(|dependencies| dependencies.get(dependency))
                .map(|dependency| dependency.kind),
            Some(DependencyKind::Production)
        )
    }

    pub(crate) fn packages(&self) -> impl Iterator<Item = &PackageId> {
        self.edges.keys()
    }

    pub(crate) fn dependencies(&self, package_id: &str) -> impl Iterator<Item = &PackageId> {
        self.edges
            .get(package_id)
            .into_iter()
            .flat_map(HashMap::keys)
    }

    pub(crate) fn imports(&self, package_id: &str) -> impl Iterator<Item = (&PackageId, Span)> {
        self.edges
            .get(package_id)
            .into_iter()
            .flat_map(|dependencies| {
                dependencies
                    .iter()
                    .map(|(package_id, dependency)| (package_id, dependency.span))
            })
    }

    pub(crate) fn production_dependencies(
        &self,
        package_id: &str,
    ) -> impl Iterator<Item = &PackageId> {
        self.edges
            .get(package_id)
            .into_iter()
            .flat_map(|dependencies| {
                dependencies.iter().filter_map(|(package_id, dependency)| {
                    (dependency.kind == DependencyKind::Production).then_some(package_id)
                })
            })
    }

    pub(crate) fn is_link_only_package(&self, package_id: &str) -> bool {
        let mut uses = self
            .edges
            .values()
            .filter_map(|dependencies| dependencies.get(package_id))
            .map(|dependency| dependency.usage);
        matches!(uses.next(), Some(ImportUse::LinkOnly))
            && uses.all(|usage| usage == ImportUse::LinkOnly)
    }

    pub(crate) fn len(&self) -> usize {
        self.edges.len()
    }

    fn insert(&mut self, package_id: PackageId, dependencies: HashMap<PackageId, Dependency>) {
        self.edges.insert(package_id, dependencies);
    }
}

impl From<HashMap<PackageId, HashSet<PackageId>>> for DependencyGraph {
    fn from(edges: HashMap<PackageId, HashSet<PackageId>>) -> Self {
        Self {
            edges: edges
                .into_iter()
                .map(|(package_id, dependencies)| {
                    let dependencies = dependencies
                        .into_iter()
                        .map(|dependency| {
                            (
                                dependency,
                                Dependency {
                                    kind: DependencyKind::Production,
                                    usage: ImportUse::Referenced,
                                    span: Span::dummy(),
                                },
                            )
                        })
                        .collect();
                    (package_id, dependencies)
                })
                .collect(),
        }
    }
}

/// A package file read and scanned for imports, but not yet parsed.
#[derive(Debug)]
pub struct ScannedFile {
    pub package_id: PackageId,
    pub file_id: u32,
    pub name: String,
    pub display_path: String,
    pub source: String,
    pub imports: Vec<syntax::program::FileImport>,
}

impl ScannedFile {
    pub fn is_d_lis(&self) -> bool {
        self.name.ends_with(".d.lis")
    }

    pub fn is_test(&self) -> bool {
        syntax::program::is_test_file(&self.name)
    }

    pub fn parse(self) -> (File, Vec<syntax::ParseError>) {
        let result = syntax::build_ast(&self.source, self.file_id);
        let file = File {
            id: self.file_id,
            package_id: self.package_id,
            name: self.name,
            display_path: self.display_path,
            source_path: None,
            source: self.source,
            items: result.ast,
            file_comment: result.file_comment,
        };
        (file, result.errors)
    }
}

#[derive(Debug)]
pub struct PackageGraphResult {
    pub order: Vec<PackageId>,
    pub cycles: Vec<kahn::Cycle>,
    pub files: HashMap<PackageId, Vec<ScannedFile>>,
    pub dependencies: DependencyGraph,
    /// Reachable from the primary roots, snapshotted before `additional` runs.
    pub primary_reachable: HashSet<PackageId>,
}

/// `primary` defines the target. `additional` widens what is analyzed.
#[derive(Debug, Default)]
pub struct Roots {
    pub primary: Vec<PackageId>,
    pub additional: Vec<PackageId>,
}

pub struct PackageGraphOptions<'a> {
    pub loader: Option<&'a dyn Loader>,
    pub sink: &'a LocalSink,
    pub scope: &'a AnalysisScope,
    pub locator: &'a TypedefLocator,
    pub include_tests: bool,
    pub project_kind: ProjectKind,
}

pub fn build_package_graph(
    store: &mut Store,
    roots: Roots,
    options: PackageGraphOptions<'_>,
) -> PackageGraphResult {
    let Roots {
        primary,
        additional,
    } = roots;
    let PackageGraphOptions {
        loader,
        sink,
        scope,
        locator,
        include_tests,
        project_kind,
    } = options;
    let mut builder = GraphBuilder {
        store,
        loader,
        sink,
        scope,
        locator,
        include_tests,
        project_kind,
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
    scope: &'a AnalysisScope,
    locator: &'a TypedefLocator,
    include_tests: bool,
    project_kind: ProjectKind,
    dependencies: DependencyGraph,
    visited: HashSet<PackageId>,
    files: HashMap<PackageId, Vec<ScannedFile>>,
    import_spans: HashMap<PackageId, Span>,
}

impl<'a> GraphBuilder<'a> {
    fn visit(&mut self, mut to_visit: Vec<PackageId>) {
        while !to_visit.is_empty() {
            let drained: Vec<PackageId> = std::mem::take(&mut to_visit);
            let mut batch: Vec<PackageId> = Vec::with_capacity(drained.len());
            for package_id in drained {
                if self.visited.insert(package_id.clone()) {
                    batch.push(package_id);
                }
            }
            if batch.is_empty() {
                continue;
            }

            batch.sort();

            let mut scanned = batch_scan_packages(
                &batch,
                self.store,
                self.loader,
                self.sink,
                self.include_tests,
                self.scope.has_project_root(),
            );

            for package_id in &batch {
                let package_files = scanned.remove(package_id).unwrap_or_default();
                let file_imports = if !package_files.is_empty() {
                    classify_scanned_imports(&package_files)
                } else if let Some(package) = self.store.get_package(package_id) {
                    classify_file_imports(package.files.values())
                } else {
                    Vec::new()
                };
                let root_has_production = self
                    .files
                    .get(ENTRY_PACKAGE_ID)
                    .is_some_and(|files| files.iter().any(|f| !f.is_test() && !f.is_d_lis()))
                    || self
                        .store
                        .get_package(ENTRY_PACKAGE_ID)
                        .is_some_and(|m| m.files.values().any(|f| !f.is_test()));
                let imports = process_file_imports(
                    file_imports,
                    ImportContext {
                        sink: self.sink,
                        scope: self.scope,
                        root_has_production,
                        importer: package_id,
                        project_kind: self.project_kind,
                        locator: self.locator,
                    },
                );

                let has_production_file = package_files.iter().any(|file| !file.is_test());
                let package_exists = has_production_file
                    || self.store.has(package_id)
                    || package_id.starts_with("go:")
                    || (self.scope.has_project_root()
                        && semantics_loader::is_external_test_package(package_id));

                if !package_exists {
                    if let Some(span) = self.import_spans.get(package_id) {
                        let is_go_stdlib =
                            stdlib::get_go_stdlib_typedef(package_id, self.locator.target())
                                .is_some();

                        let src_prefix_hint = package_id
                            .strip_prefix("src/")
                            .filter(|stripped| {
                                self.loader
                                    .is_some_and(|fs| !fs.scan_folder(stripped).is_empty())
                            })
                            .map(String::from);

                        let reason = if let Some(stripped) = src_prefix_hint {
                            diagnostics::package_graph::MissingPackageReason::UnnecessarySrcPrefix(
                                stripped,
                            )
                        } else if is_go_stdlib {
                            diagnostics::package_graph::MissingPackageReason::GoStandardLibrary
                        } else if let Some(unit) = self.scope.script_unit() {
                            diagnostics::package_graph::MissingPackageReason::Script {
                                inside_project: unit.inside_project,
                            }
                        } else {
                            diagnostics::package_graph::MissingPackageReason::NotFound
                        };
                        self.sink
                            .push(diagnostics::package_graph::package_not_found(
                                package_id, *span, reason,
                            ));
                    }
                    continue;
                }

                self.files.insert(package_id.clone(), package_files);

                for (import, dependency) in &imports {
                    if !self.visited.contains(import) {
                        to_visit.push(import.clone());
                    }
                    self.import_spans
                        .entry(import.clone())
                        .or_insert(dependency.span);
                }

                self.dependencies.insert(package_id.clone(), imports);
            }
        }
    }

    fn finish(self, primary_reachable: HashSet<PackageId>) -> PackageGraphResult {
        let (order, cycles) = kahn::topological_sort(&self.dependencies);

        PackageGraphResult {
            order,
            cycles,
            files: self.files,
            dependencies: self.dependencies,
            primary_reachable,
        }
    }
}

#[derive(Clone)]
struct ClassifiedImport {
    import: syntax::program::FileImport,
    kind: DependencyKind,
}

fn dependency_kind(is_test: bool) -> DependencyKind {
    if is_test {
        DependencyKind::TestOnly
    } else {
        DependencyKind::Production
    }
}

fn classify_file_imports<'a>(files: impl IntoIterator<Item = &'a File>) -> Vec<ClassifiedImport> {
    files
        .into_iter()
        .flat_map(|file| {
            let kind = dependency_kind(file.is_test());
            file.imports()
                .into_iter()
                .map(move |import| ClassifiedImport { import, kind })
        })
        .collect()
}

fn classify_scanned_imports(files: &[ScannedFile]) -> Vec<ClassifiedImport> {
    files
        .iter()
        .flat_map(|file| {
            let kind = dependency_kind(file.is_test());
            file.imports.iter().map(move |import| ClassifiedImport {
                import: import.clone(),
                kind,
            })
        })
        .collect()
}

struct ScanJob {
    package_id: PackageId,
    file_id: u32,
    filename: String,
    display_path: String,
    source: String,
}

/// Reads and scans every file of the given packages. The parse waits for the cache.
fn batch_scan_packages(
    packages: &[PackageId],
    store: &Store,
    loader: Option<&dyn Loader>,
    sink: &LocalSink,
    include_tests: bool,
    has_project_root: bool,
) -> HashMap<PackageId, Vec<ScannedFile>> {
    let Some(fs) = loader else {
        return HashMap::default();
    };

    const PARALLEL_THRESHOLD: usize = 4;

    let to_read: Vec<&PackageId> = packages.iter().filter(|m| !store.has(m)).collect();
    let folders: Vec<(&PackageId, Vec<(String, semantics_loader::FileContent)>)> =
        if to_read.len() < PARALLEL_THRESHOLD {
            to_read
                .into_iter()
                .map(|m| (m, read_folder(fs, m)))
                .collect()
        } else {
            use rayon::prelude::*;
            to_read
                .into_par_iter()
                .map(|m| (m, read_folder(fs, m)))
                .collect()
        };

    let mut jobs: Vec<ScanJob> = Vec::new();
    for (package_id, entries) in folders {
        let is_external_test =
            has_project_root && semantics_loader::is_external_test_package(package_id);
        for (filename, content) in entries {
            if is_external_test {
                match semantics_loader::external_test_file_issue(&filename) {
                    Some(semantics_loader::ExternalTestFileIssue::WrongSuffix) => {
                        sink.push(diagnostics::package_graph::wrong_test_file_suffix(
                            &content.display_path,
                        ));
                        continue;
                    }
                    Some(semantics_loader::ExternalTestFileIssue::NotATestFile) => {
                        sink.push(diagnostics::package_graph::non_test_file_under_tests(
                            &content.display_path,
                        ));
                        continue;
                    }
                    None => {}
                }
            } else if filename.ends_with("_test.lis") {
                sink.push(diagnostics::package_graph::wrong_test_file_suffix(
                    &content.display_path,
                ));
                continue;
            }
            if filename.ends_with(".test.lis") && !include_tests {
                continue;
            }
            let file_id = store.new_file_id();
            jobs.push(ScanJob {
                package_id: package_id.clone(),
                file_id,
                filename,
                display_path: content.display_path,
                source: content.source,
            });
        }
    }

    let scanned: Vec<ScannedFile> = if jobs.len() < PARALLEL_THRESHOLD {
        jobs.into_iter().map(scan_one).collect()
    } else {
        use rayon::prelude::*;
        jobs.into_par_iter().map(scan_one).collect()
    };

    let mut grouped: HashMap<PackageId, Vec<ScannedFile>> = HashMap::default();
    for file in scanned {
        grouped
            .entry(file.package_id.clone())
            .or_default()
            .push(file);
    }
    grouped
}

fn read_folder(fs: &dyn Loader, package_id: &str) -> Vec<(String, semantics_loader::FileContent)> {
    let mut entries: Vec<(String, semantics_loader::FileContent)> =
        fs.scan_folder(package_id).into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn scan_one(job: ScanJob) -> ScannedFile {
    ScannedFile {
        imports: syntax::imports::scan_imports(&job.source, job.file_id),
        package_id: job.package_id,
        file_id: job.file_id,
        name: job.filename,
        display_path: job.display_path,
        source: job.source,
    }
}

#[derive(Clone, Copy)]
struct ImportContext<'a> {
    sink: &'a LocalSink,
    scope: &'a AnalysisScope,
    root_has_production: bool,
    importer: &'a str,
    project_kind: ProjectKind,
    locator: &'a TypedefLocator,
}

fn process_file_imports(
    file_imports: Vec<ClassifiedImport>,
    ctx: ImportContext<'_>,
) -> HashMap<PackageId, Dependency> {
    let ImportContext {
        sink,
        scope,
        root_has_production,
        importer,
        project_kind,
        locator,
    } = ctx;
    let mut imports = HashMap::default();
    let mut pending_go_imports: HashMap<&str, Dependency> = HashMap::default();
    for classified in &file_imports {
        let file_import = &classified.import;
        if !file_import.name.starts_with("go:") {
            continue;
        }
        let usage = if matches!(file_import.alias, Some(ImportAlias::Blank(_))) {
            ImportUse::LinkOnly
        } else {
            ImportUse::Referenced
        };
        let dependency = Dependency {
            kind: classified.kind,
            usage,
            span: file_import.name_span,
        };
        pending_go_imports
            .entry(&file_import.name)
            .and_modify(|pending| pending.merge(dependency))
            .or_insert(dependency);
    }

    for classified in &file_imports {
        let file_import = &classified.import;
        if file_import.name == "prelude" {
            sink.push(diagnostics::package_graph::cannot_import_prelude(
                file_import.span,
            ));
            continue;
        }

        if file_import.name.starts_with("**") {
            sink.push(diagnostics::package_graph::reserved_package_import(
                file_import.span,
            ));
            continue;
        }

        if file_import.name == ENTRY_PACKAGE_ID {
            sink.push(diagnostics::package_graph::cannot_import_entry(
                file_import.name_span,
            ));
            continue;
        }

        if scope.has_project_root() && semantics_loader::is_external_test_package(&file_import.name)
        {
            sink.push(diagnostics::package_graph::cannot_import_external_tests(
                file_import.name_span,
            ));
            continue;
        }

        if scope.has_project_root() && file_import.name == semantics_loader::ROOT_IMPORT {
            if let Some(ImportAlias::Blank(span)) = &file_import.alias {
                sink.push(diagnostics::infer::blank_import_non_go(*span));
            } else {
                match root_import_target(&file_import.name, importer, project_kind) {
                    Some(_) if !root_has_production => {
                        sink.push(
                            diagnostics::package_graph::cannot_import_root_without_source(
                                file_import.name_span,
                            ),
                        );
                    }
                    Some(entry) => {
                        let dependency = Dependency {
                            kind: classified.kind,
                            usage: ImportUse::Referenced,
                            span: file_import.name_span,
                        };
                        imports
                            .entry(entry.to_string())
                            .and_modify(|existing: &mut Dependency| existing.merge(dependency))
                            .or_insert(dependency);
                    }
                    None if project_kind == ProjectKind::Binary => {
                        sink.push(diagnostics::package_graph::cannot_import_root_in_binary(
                            file_import.name_span,
                        ));
                    }
                    None => {
                        sink.push(diagnostics::package_graph::cannot_import_root_from_src(
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
                            script: scope.script_unit(),
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
                            script: scope.script_unit(),
                            replace_importer: None,
                        },
                        sink,
                    )
                }
            };
            if ok {
                imports.insert(file_import.name.to_string(), pending);
            }
            continue;
        }

        let blank_span = match &file_import.alias {
            Some(ImportAlias::Blank(span)) => Some(*span),
            _ => None,
        };
        let is_dotted = file_import.name.contains('.');

        if is_dotted && locator.is_declared_go_dep(&file_import.name) {
            sink.push(diagnostics::package_graph::missing_go_prefix(
                &file_import.name,
                file_import.name_span,
                blank_span.is_some(),
            ));
            continue;
        }

        if is_dotted {
            sink.push(diagnostics::package_graph::invalid_package_path(
                &file_import.name,
                file_import.name_span,
                blank_span.is_some(),
            ));
        }
        if let Some(span) = blank_span
            && !diagnostics::package_graph::is_go_package_shaped(&file_import.name)
        {
            sink.push(diagnostics::infer::blank_import_non_go(span));
        }
        if is_dotted || blank_span.is_some() {
            continue;
        }

        let dependency = Dependency {
            kind: classified.kind,
            usage: ImportUse::Referenced,
            span: file_import.name_span,
        };
        imports
            .entry(file_import.name.to_string())
            .and_modify(|existing: &mut Dependency| existing.merge(dependency))
            .or_insert(dependency);
    }

    imports
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::program::FileImport;

    const DIRECTORY_SCOPE: AnalysisScope = AnalysisScope::Directory;
    const PROJECT_SCOPE: AnalysisScope = AnalysisScope::Project(std::path::PathBuf::new());

    fn go_import(is_blank: bool, offset: u32) -> FileImport {
        let span = Span::new(0, offset, 1);
        FileImport {
            name: "go:fmt".into(),
            name_span: span,
            alias: is_blank.then_some(ImportAlias::Blank(span)),
            span,
        }
    }

    fn classified(imports: Vec<FileImport>, kind: DependencyKind) -> Vec<ClassifiedImport> {
        imports
            .into_iter()
            .map(|import| ClassifiedImport { import, kind })
            .collect()
    }

    fn is_link_only(imports: Vec<FileImport>) -> bool {
        let sink = LocalSink::new();
        let resolved = process_file_imports(
            classified(imports, DependencyKind::Production),
            ImportContext {
                sink: &sink,
                scope: &DIRECTORY_SCOPE,
                root_has_production: false,
                importer: "caller",
                project_kind: ProjectKind::Binary,
                locator: &TypedefLocator::default(),
            },
        );

        assert!(!sink.has_errors());
        resolved
            .get("go:fmt")
            .is_some_and(|dependency| dependency.usage == ImportUse::LinkOnly)
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
                classified(
                    vec![FileImport {
                        name: name.into(),
                        name_span: span,
                        alias: None,
                        span,
                    }],
                    DependencyKind::TestOnly,
                ),
                ImportContext {
                    sink: &sink,
                    scope: &PROJECT_SCOPE,
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
            classified(
                vec![FileImport {
                    name: "tests".into(),
                    name_span: span,
                    alias: None,
                    span,
                }],
                DependencyKind::Production,
            ),
            ImportContext {
                sink: &sink,
                scope: &DIRECTORY_SCOPE,
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
            Some(ENTRY_PACKAGE_ID)
        );
        assert_eq!(
            root_import_target("root", "tests/integration", ProjectKind::Library),
            Some(ENTRY_PACKAGE_ID)
        );
        assert_eq!(
            root_import_target("root", "geometry", ProjectKind::Library),
            None,
            "src packages cannot import the root"
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
    fn referenced_edge_wins_across_importing_packages() {
        let dependency = |usage| Dependency {
            kind: DependencyKind::Production,
            usage,
            span: Span::dummy(),
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

        assert!(!graph.is_link_only_package("go:fmt"));
    }
}
