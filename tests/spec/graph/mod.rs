use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::LocalSink;
use semantics::AnalysisScope;
use semantics::package_graph::kahn::topological_sort;
use semantics::package_graph::{DependencyGraph, PackageGraphOptions, Roots, build_package_graph};
use semantics::store::Store;

use crate::_harness::filesystem::MockFileSystem;

const PROJECT_SCOPE: AnalysisScope = AnalysisScope::Project(std::path::PathBuf::new());
const SCRIPT_SCOPE: AnalysisScope = AnalysisScope::Script {
    inside_project: false,
};

fn default_resolver() -> deps::TypedefLocator {
    deps::TypedefLocator::default()
}

fn roots(entry: &str) -> Roots {
    Roots {
        primary: vec![entry.to_string()],
        additional: vec![],
    }
}

fn graph_options<'a>(
    loader: &'a MockFileSystem,
    sink: &'a LocalSink,
    locator: &'a deps::TypedefLocator,
    scope: &'a AnalysisScope,
) -> PackageGraphOptions<'a> {
    PackageGraphOptions {
        loader: Some(loader),
        sink,
        scope,
        locator,
        include_tests: true,
        project_kind: semantics::ProjectKind::Binary,
    }
}

fn host_module_cache_dir(project_root: &std::path::Path, module: &str) -> std::path::PathBuf {
    deps::typedef_cache_dir(project_root)
        .join(stdlib::Target::host().cache_segment())
        .join(module)
}

fn has_diagnostic_code(sink: &LocalSink, code: &str) -> bool {
    sink.any(|diagnostic| diagnostic.code_str() == Some(code))
}

#[test]
fn kahn_simple_dependency_chain() {
    let mut edges = HashMap::default();
    edges.insert("a".to_string(), HashSet::from_iter(["b".to_string()]));
    edges.insert("b".to_string(), HashSet::from_iter(["c".to_string()]));
    edges.insert("c".to_string(), HashSet::default());

    let (order, cycles) = topological_sort(&DependencyGraph::from(edges));

    assert!(cycles.is_empty());
    let pos_a = order.iter().position(|x| x == "a").unwrap();
    let pos_b = order.iter().position(|x| x == "b").unwrap();
    let pos_c = order.iter().position(|x| x == "c").unwrap();
    assert!(pos_c < pos_b);
    assert!(pos_b < pos_a);
}

#[test]
fn kahn_diamond_dependency() {
    let mut edges = HashMap::default();
    edges.insert(
        "a".to_string(),
        HashSet::from_iter(["b".to_string(), "c".to_string()]),
    );
    edges.insert("b".to_string(), HashSet::from_iter(["d".to_string()]));
    edges.insert("c".to_string(), HashSet::from_iter(["d".to_string()]));
    edges.insert("d".to_string(), HashSet::default());

    let (order, cycles) = topological_sort(&DependencyGraph::from(edges));

    assert!(cycles.is_empty());
    let pos_a = order.iter().position(|x| x == "a").unwrap();
    let pos_b = order.iter().position(|x| x == "b").unwrap();
    let pos_c = order.iter().position(|x| x == "c").unwrap();
    let pos_d = order.iter().position(|x| x == "d").unwrap();
    assert!(pos_d < pos_b);
    assert!(pos_d < pos_c);
    assert!(pos_b < pos_a);
    assert!(pos_c < pos_a);
}

#[test]
fn kahn_simple_cycle() {
    let mut edges = HashMap::default();
    edges.insert("a".to_string(), HashSet::from_iter(["b".to_string()]));
    edges.insert("b".to_string(), HashSet::from_iter(["c".to_string()]));
    edges.insert("c".to_string(), HashSet::from_iter(["a".to_string()]));

    let (_, cycles) = topological_sort(&DependencyGraph::from(edges));

    assert!(!cycles.is_empty());
}

#[test]
fn kahn_no_dependencies() {
    let mut edges = HashMap::default();
    edges.insert("a".to_string(), HashSet::default());
    edges.insert("b".to_string(), HashSet::default());
    edges.insert("c".to_string(), HashSet::default());

    let (order, cycles) = topological_sort(&DependencyGraph::from(edges));

    assert!(cycles.is_empty());
    assert_eq!(order.len(), 3);
}

#[test]
fn test_only_imports_excluded_from_production_edges() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "math""#);
    fs.add_file("math", "core.lis", "pub fn add() -> int { 1 }");
    fs.add_file("math", "core.test.lis", r#"import "fixture""#);
    fs.add_file("fixture", "fixture.lis", "pub fn sample() -> int { 2 }");

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(
        result.dependencies.contains_dependency("math", "fixture"),
        "a test-file import must still be a graph edge for reachability"
    );
    assert!(
        !result
            .dependencies
            .contains_production_dependency("math", "fixture"),
        "a test-only import must not enter production edges that importers key on"
    );
    assert!(
        !result
            .dependencies
            .contains_production_dependency("main", "fixture"),
        "the test-only dependency must not propagate to production importers"
    );
}

#[test]
fn production_import_classification_wins_when_tests_import_the_same_package() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "fixture""#);
    fs.add_file("main", "main.test.lis", r#"import "fixture""#);
    fs.add_file("fixture", "fixture.lis", "pub fn sample() -> int { 2 }");

    let mut store = Store::new();
    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(
        result
            .dependencies
            .contains_production_dependency("main", "fixture")
    );
}

#[test]
fn graph_simple_dependency() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "lib""#);
    fs.add_file("lib", "lib.lis", "fn foo() { 1 }");

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(result.cycles.is_empty());
    assert!(!sink.has_errors());

    let pos_main = result.order.iter().position(|x| x == "main");
    let pos_lib = result.order.iter().position(|x| x == "lib");

    assert!(pos_lib.is_some());
    assert!(pos_main.is_some());
    assert!(pos_lib.unwrap() < pos_main.unwrap());
}

#[test]
fn graph_missing_package() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "missing""#);

    let mut store = Store::new();

    let sink = LocalSink::new();
    let _result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(sink.has_errors());
}

#[test]
fn graph_cycle_detection() {
    let mut fs = MockFileSystem::new();
    fs.add_file("a", "a.lis", r#"import "b""#);
    fs.add_file("b", "b.lis", r#"import "c""#);
    fs.add_file("c", "c.lis", r#"import "a""#);

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        roots("a"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(!result.cycles.is_empty());
}

#[test]
fn graph_cycle_carries_the_span_of_every_hop() {
    let mut fs = MockFileSystem::new();
    fs.add_file("a", "a.lis", "import \"b\"\n");
    fs.add_file("b", "b.lis", "import \"a\"\n");

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        roots("a"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    let cycle = result.cycles.first().expect("the cycle must be detected");
    let packages: Vec<&str> = cycle.iter().map(|hop| hop.package.as_str()).collect();
    assert_eq!(
        packages,
        vec!["a", "b"],
        "a cycle starts at its first package"
    );
    for hop in cycle {
        assert_eq!(
            hop.span.byte_offset, 7,
            "each hop points at the name of its own `import`"
        );
    }
    assert_ne!(
        cycle[0].span.file_id, cycle[1].span.file_id,
        "each hop belongs to the file that declares its import"
    );
}

/// Every way an importer can name something in a package the cycle dropped.
fn analyze_importer_of_cycle(entry_source: &str) -> passes::Analysis {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();

    let mut fs = MemoryLoader::new();
    fs.add_file(
        "alpha",
        "alpha.lis",
        "import \"beta\"\n\
         pub struct Node { pub id: int }\n\
         pub enum Color { Red, Green }\n\
         pub enum Shape { Circle(int), Square(int) }\n\
         pub const LIMIT: int = 10\n\
         pub fn describe(n: Node) -> string { beta.render(n.id) }\n\
         pub fn mk() -> Node { Node { id: 1 } }\n",
    );
    fs.add_file(
        "beta",
        "beta.lis",
        "import \"alpha\"\npub fn render(_id: int) -> string { \"node\" }\n",
    );

    analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Project(tmp.path().to_path_buf()),
        loader: &fs,
        entry: Some(EntryFile::new(
            entry_source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: true,
    })
}

fn error_codes(analysis: &passes::Analysis) -> Vec<&str> {
    analysis
        .errors()
        .iter()
        .filter_map(|error| error.code_str())
        .collect()
}

#[test]
fn cycle_suppresses_the_name_errors_it_causes_in_importers() {
    let output = analyze_importer_of_cycle(
        "import \"alpha\"\n\
         \n\
         fn main() {\n\
         \x20 let a = alpha.Node { id: 1 }\n\
         \x20 let b: alpha.Node = alpha.mk()\n\
         \x20 let c = alpha.describe(a)\n\
         \x20 let d = alpha.LIMIT\n\
         \x20 let e = alpha.Color.Red\n\
         \x20 let f: Slice<alpha.Node> = []\n\
         \x20 let g = alpha.Node { ..a }\n\
         \x20 match alpha.mk() {\n\
         \x20   alpha.Node { id } => { let _ = id },\n\
         \x20 }\n\
         \x20 match e {\n\
         \x20   alpha.Color.Red => {},\n\
         \x20   alpha.Color.Green => {},\n\
         \x20 }\n\
         \x20 match alpha.Shape.Circle(1) {\n\
         \x20   alpha.Shape.Circle(r) => { let _ = r },\n\
         \x20   alpha.Shape.Square(s) => { let _ = s },\n\
         \x20 }\n\
         }\n",
    );

    assert_eq!(
        error_codes(&output),
        vec!["resolve.import_cycle"],
        "every one of those names is declared and `pub` in `alpha`, and unresolvable \
         only because the cycle dropped it"
    );
}

/// A name the dropped package never declared is the caller's own mistake.
#[test]
fn cycle_still_reports_names_the_dropped_package_never_declared() {
    let output = analyze_importer_of_cycle(
        "import \"alpha\"\n\
         \n\
         fn main() {\n\
         \x20 let _ = alpha.missing()\n\
         \x20 let _ = alpha.LIMITT\n\
         \x20 let _ = alpha.Nope { x: 1 }\n\
         \x20 let _: alpha.NoType = 1\n\
         \x20 match alpha.Shape.Circle(1) {\n\
         \x20   alpha.Shape.Triangle(r) => { let _ = r },\n\
         \x20   _ => {},\n\
         \x20 }\n\
         }\n",
    );

    let mut codes = error_codes(&output);
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes,
        vec![
            "resolve.import_cycle",
            "resolve.name_not_found",
            "resolve.not_found_in_package",
            "resolve.struct_not_found",
            "resolve.type_not_found",
            "resolve.variant_not_found",
        ],
    );
}

/// A typedef exports every declaration, `pub` or not.
#[test]
fn cycle_reads_the_exports_of_a_declaration_only_module() {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();

    let mut fs = MemoryLoader::new();
    fs.add_file(
        "decl",
        "decl.d.lis",
        "import \"beta\"\n\
         pub var Counter: int\n\
         var Hidden: int\n\
         pub fn helper() -> int\n\
         enum Mode { Fast, Slow }\n",
    );
    fs.add_file(
        "beta",
        "beta.lis",
        "import \"decl\"\npub fn g() -> int { decl.helper() }\n",
    );

    let source = "import \"decl\"\n\
                  \n\
                  fn main() {\n\
                  \x20 let _ = decl.Counter\n\
                  \x20 let _ = decl.Hidden\n\
                  \x20 let _ = decl.helper()\n\
                  \x20 let _ = decl.Mode.Fast\n\
                  }\n";
    let output = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Project(tmp.path().to_path_buf()),
        loader: &fs,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: true,
    });

    assert_eq!(error_codes(&output), vec!["resolve.import_cycle"]);
}

/// Suppression follows the reference, not the file.
#[test]
fn cycle_keeps_the_unrelated_errors_of_an_importer() {
    let output = analyze_importer_of_cycle(
        "import \"alpha\"\n\
         import \"alpha\"\n\
         \n\
         fn main() {\n\
         \x20 let _ = alpha.mk()\n\
         \x20 let _ = undefined_local\n\
         \x20 let _: Undeclared = 1\n\
         \x20 let _ = Nonexistent { x: 1 }\n\
         }\n",
    );

    let mut codes = error_codes(&output);
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes,
        vec![
            "resolve.duplicate_import",
            "resolve.import_cycle",
            "resolve.name_not_found",
            "resolve.struct_not_found",
            "resolve.type_not_found",
        ],
    );
}

/// An unregistered `go:` package would reach the shared stdlib cache empty.
#[test]
fn cycle_keeps_unregistered_go_modules_out_of_the_store() {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();

    let mut fs = MemoryLoader::new();
    fs.add_file(
        "alpha",
        "alpha.lis",
        "import \"beta\"\nimport \"go:fmt\"\npub fn f() -> string { fmt.Sprintf(\"%d\", beta.g()) }\n",
    );
    fs.add_file(
        "beta",
        "beta.lis",
        "import \"alpha\"\npub fn g() -> int { 2 }\n",
    );

    let source = "import \"alpha\"\n\nfn main() {\n  let _ = alpha.f()\n}\n";
    let output = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Project(tmp.path().to_path_buf()),
        loader: &fs,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: true,
    });

    assert!(
        output.emit_input.go_package_ids.is_empty(),
        "`go:fmt` is imported only by a package the cycle dropped, so it is never \
         registered and must not reach the store: {:?}",
        output.emit_input.go_package_ids,
    );
    assert!(
        output
            .errors()
            .iter()
            .any(|error| error.code_str() == Some("resolve.import_cycle")),
    );
}

#[test]
fn additional_roots_widen_graph_but_not_reachable_set() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "lib""#);
    fs.add_file("lib", "lib.lis", "pub fn f() -> int { 1 }");
    fs.add_file("orphan", "orphan.lis", "pub fn g() -> int { 2 }");

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        Roots {
            primary: vec!["main".to_string()],
            additional: vec!["orphan".to_string()],
        },
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(result.order.iter().any(|m| m == "orphan"));
    assert!(result.dependencies.contains_package("orphan"));
    assert!(result.files.contains_key("orphan"));
    assert!(
        !result.primary_reachable.contains("orphan"),
        "orphan is outside the primary reachable set"
    );
    assert!(result.primary_reachable.contains("main"));
    assert!(result.primary_reachable.contains("lib"));
}

#[test]
fn empty_additional_leaves_orphan_out_of_graph() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "lib""#);
    fs.add_file("lib", "lib.lis", "pub fn f() -> int { 1 }");
    fs.add_file("orphan", "orphan.lis", "pub fn g() -> int { 2 }");

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(!result.order.iter().any(|m| m == "orphan"));
}

#[test]
fn zero_primary_roots_begins_with_additional() {
    let mut fs = MockFileSystem::new();
    fs.add_file("lib", "lib.lis", "pub fn f() -> int { 1 }");

    let mut store = Store::new();

    let sink = LocalSink::new();
    let result = build_package_graph(
        &mut store,
        Roots {
            primary: vec![],
            additional: vec!["lib".to_string()],
        },
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(result.primary_reachable.is_empty());
    assert!(result.order.iter().any(|m| m == "lib"));
}

#[test]
fn check_analyzes_orphan_and_surfaces_its_error() {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();

    let mut fs = MemoryLoader::new();
    fs.add_file("lib", "lib.lis", "pub fn f() -> int { 1 }");
    fs.add_file(
        "orphan",
        "orphan.lis",
        "pub fn broken(x: int) -> int { x + \"boom\" }",
    );

    let source = "import \"lib\"\n\nfn main() {\n  let _ = lib.f()\n}\n";
    let output = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Project(tmp.path().to_path_buf()),
        loader: &fs,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: true,
    });

    assert!(
        output.unreachable_packages.iter().any(|m| m == "orphan"),
        "orphan must be reported as unreachable, got: {:?}",
        output.unreachable_packages
    );
    assert!(
        output
            .errors()
            .iter()
            .any(|e| e.code_str() == Some("infer.type_mismatch")),
        "check must analyze the orphan and surface its error: {:?}",
        output.errors()
    );
}

#[test]
fn check_analyzes_tests_in_declaration_only_package() {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();

    let mut fs = MemoryLoader::new();
    fs.add_file("decl", "decl.d.lis", "pub fn ext() -> int\n");
    fs.add_file(
        "decl",
        "decl.test.lis",
        "#[test]\nfn broken() {\n  let _ = 1 + \"x\"\n}\n",
    );

    let source = "fn main() {}\n";
    let output = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Project(tmp.path().to_path_buf()),
        loader: &fs,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: true,
    });

    assert!(
        output
            .errors()
            .iter()
            .any(|e| e.code_str() == Some("infer.type_mismatch")),
        "a test in a declaration-plus-test package must be checked: {:?}",
        output.errors()
    );
}

#[test]
fn graph_script_third_party_go_import_uses_package_not_found() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "go:github.com/gorilla/mux""#);

    let mut store = Store::new();

    let sink = LocalSink::new();
    let _result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &SCRIPT_SCOPE),
    );

    assert!(sink.has_errors());
    assert!(has_diagnostic_code(&sink, "resolve.package_not_found"));
}

#[test]
fn graph_project_third_party_go_import_undeclared() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "go:github.com/gorilla/mux""#);

    let mut store = Store::new();

    let sink = LocalSink::new();
    let _result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &default_resolver(), &PROJECT_SCOPE),
    );

    assert!(sink.has_errors());
    assert!(has_diagnostic_code(&sink, "resolve.undeclared_go_import"));
}

#[test]
fn graph_declared_dep_missing_typedef() {
    use std::collections::BTreeMap;

    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "go:github.com/gorilla/mux""#);

    let mut store = Store::new();

    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency::Remote {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(go_deps, None, stdlib::Target::host());

    let sink = LocalSink::new();
    let _result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &resolver, &PROJECT_SCOPE),
    );

    assert!(sink.has_errors());

    let diags = sink.into_diagnostics();
    let missing = diags
        .iter()
        .find(|d| d.code_str() == Some("resolve.missing_go_typedef"))
        .expect("missing_go_typedef diagnostic");
    let help = missing.plain_help().unwrap_or("");
    assert!(
        help.contains("lis check"),
        "help should suggest `lis check` to regenerate all typedefs, got: {help}",
    );
    assert!(
        help.contains("lis add github.com/gorilla/mux@v1.8.0"),
        "help should suggest `lis add <module>@<version>` for targeted regen, got: {help}",
    );
}

#[test]
fn graph_subpackage_missing_typedef_points_at_add() {
    use std::collections::BTreeMap;

    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "go:k8s.io/api/core/v1""#);

    let mut store = Store::new();

    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "k8s.io/api".to_string(),
        deps::GoDependency::Remote {
            version: "v0.30.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(go_deps, None, stdlib::Target::host());

    let sink = LocalSink::new();
    let _result = build_package_graph(
        &mut store,
        roots("main"),
        graph_options(&fs, &sink, &resolver, &PROJECT_SCOPE),
    );

    assert!(sink.has_errors());

    let diags = sink.into_diagnostics();
    let missing = diags
        .iter()
        .find(|d| d.code_str() == Some("resolve.missing_go_typedef"))
        .expect("missing_go_typedef diagnostic");
    let help = missing.plain_help().unwrap_or("");
    assert!(
        help.contains("Subpackage"),
        "subpackage variant should mention `Subpackage`, got: {help}",
    );
    assert!(
        help.contains("k8s.io/api/core/v1"),
        "subpackage variant should reference the imported package path, got: {help}",
    );
    assert!(
        help.contains("lis add k8s.io/api@v0.30.0"),
        "subpackage variant should suggest `lis add <module>@<version>` (which runs reconcile and writes missing subpackage typedefs), got: {help}",
    );
    assert!(
        !help.contains("lis sync") && !help.contains("lis check"),
        "subpackage variant must not suggest `lis sync` or `lis check`: neither regenerates a missing subpackage typedef when the module dir already contains the root .d.lis, got: {help}",
    );
}

#[test]
fn store_get_definition_domain_style_go_package() {
    let mut store = Store::new();
    store.add_package("go:github.com/gorilla/mux");

    let package = store.get_package_mut("go:github.com/gorilla/mux").unwrap();
    package.definitions.insert(
        "go:github.com/gorilla/mux.Router".into(),
        syntax::program::Definition {
            visibility: syntax::program::Visibility::Public,
            ty: syntax::types::Type::Nominal {
                id: "go:github.com/gorilla/mux.Router".into(),
                params: vec![],
            },
            name_span: Some(syntax::ast::Span::dummy()),
            doc: None,
            body: syntax::program::DefinitionBody::Struct {
                generics: vec![],
                fields: syntax::ast::StructFields::Record(vec![]),
                methods: Default::default(),
                attributes: Default::default(),
            },
        },
    );

    let def = store.get_definition("go:github.com/gorilla/mux.Router");
    assert!(
        def.is_some(),
        "get_definition must resolve domain-style Go package qualified names"
    );
}

#[test]
fn store_package_for_qualified_name_domain_style() {
    let mut store = Store::new();
    store.add_package("go:github.com/gorilla/mux");
    store.add_package("go:net/http");
    store.add_package("mymod");

    assert_eq!(
        store.package_for_qualified_name("go:github.com/gorilla/mux.Router"),
        Some("go:github.com/gorilla/mux"),
    );
    assert_eq!(
        store.package_for_qualified_name("go:net/http.Request"),
        Some("go:net/http"),
    );
    assert_eq!(
        store.package_for_qualified_name("mymod.MyType"),
        Some("mymod"),
    );
    assert_eq!(
        store.package_for_qualified_name("go:github.com/gorilla/mux.Method.Get"),
        Some("go:github.com/gorilla/mux"),
    );
}

#[test]
fn stdlib_cache_excludes_third_party_packages() {
    // The stdlib cache save filters packages by id.starts_with("go:") and
    // !id.contains('/') after stripping "go:". Third-party packages like
    // "go:github.com/gorilla/mux" contain '/' and must be excluded.
    let third_party = "go:github.com/gorilla/mux";
    let stdlib = "go:net/http";

    let is_stdlib_go = |id: &str| id.strip_prefix("go:").is_some_and(deps::is_stdlib);

    assert!(!is_stdlib_go(third_party));
    assert!(is_stdlib_go(stdlib));
    assert!(is_stdlib_go("go:fmt"));
    assert!(is_stdlib_go("go:crypto/tls"));
}

#[test]
fn store_package_for_qualified_name_major_version_suffix() {
    let mut store = Store::new();
    store.add_package("go:github.com/jackc/pgx/v5");

    assert_eq!(
        store.package_for_qualified_name("go:github.com/jackc/pgx/v5.Conn"),
        Some("go:github.com/jackc/pgx/v5"),
    );
    // Must not match a shorter prefix that is not registered
    assert_eq!(
        store.package_for_qualified_name("go:github.com/jackc/pgx.Row"),
        None,
    );
}

#[test]
fn store_package_for_qualified_name_nested_subpackage() {
    let mut store = Store::new();
    store.add_package("go:github.com/gorilla/mux");

    assert_eq!(
        store.package_for_qualified_name("go:github.com/gorilla/mux.Router"),
        Some("go:github.com/gorilla/mux"),
    );
    assert_eq!(
        store.package_for_qualified_name("go:github.com/gorilla/mux.Router.ServeHTTP"),
        Some("go:github.com/gorilla/mux"),
    );
}

#[test]
fn resolver_root_vs_subpackage_typedef_lookup() {
    use std::collections::BTreeMap;

    let tmp = tempfile::tempdir().unwrap();
    let project_root = tmp.path();

    let root_dir = host_module_cache_dir(project_root, "github.com/gorilla/mux@v1.8.0");
    let sub_dir = root_dir.join("middleware");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(root_dir.join("mux.d.lis"), "// root\n").unwrap();
    std::fs::write(sub_dir.join("middleware.d.lis"), "// sub\n").unwrap();

    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency::Remote {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(
        go_deps,
        Some(project_root.to_path_buf()),
        stdlib::Target::host(),
    );

    match resolver.find_typedef_content("github.com/gorilla/mux") {
        deps::TypedefLocatorResult::Found {
            content: source, ..
        } => {
            assert!(source.contains("root"));
        }
        other => panic!("Root package: expected Found, got {:?}", other),
    }

    match resolver.find_typedef_content("github.com/gorilla/mux/middleware") {
        deps::TypedefLocatorResult::Found {
            content: source, ..
        } => {
            assert!(source.contains("sub"));
        }
        other => panic!("Subpackage: expected Found, got {:?}", other),
    }
}

/// Impl block on a third-party Go struct must not be rejected as foreign.
/// Regression: methods.rs used `find('.')` to extract the package from a
/// qualified name, which broke on `go:github.com/gorilla/mux.Router`.
#[test]
fn third_party_go_struct_impl_methods_registered() {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();
    let project_root = tmp.path();
    let cache_dir = host_module_cache_dir(project_root, "github.com/gorilla/mux@v1.8.0");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("mux.d.lis"),
        "pub struct Router {}\nimpl Router {\n    fn route(self, path: string) -> string\n}\npub fn new_router() -> Router\n",
    )
    .unwrap();

    let mut go_deps = std::collections::BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency::Remote {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(
        go_deps,
        Some(project_root.to_path_buf()),
        stdlib::Target::host(),
    );

    let source = r#"
import "go:github.com/gorilla/mux"

fn main() {
    let r = mux.new_router()
    r.route("/api")
}
"#;

    let no_loader = MemoryLoader::new();

    let result = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Directory,
        loader: &no_loader,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &resolver,
        go_module: "",
        disable_cache: false,
    });

    let impl_errors: Vec<_> = result
        .errors()
        .iter()
        .filter(|e| {
            e.code_str()
                .is_some_and(|c| c == "infer.impl_on_foreign_type")
        })
        .collect();

    assert!(
        impl_errors.is_empty(),
        "impl block on third-party Go struct must not be rejected as foreign: {:?}",
        impl_errors,
    );

    let method_errors: Vec<_> = result
        .errors()
        .iter()
        .filter(|e| e.code_str().is_some_and(|c| c == "infer.member_not_found"))
        .collect();

    assert!(
        method_errors.is_empty(),
        "method call on third-party Go struct must resolve: {:?}",
        method_errors,
    );
}

/// Third-party Go packages must not be saved into the stdlib definition
/// cache. Regression: analyze.rs filtered by `starts_with("go:")` which
/// included third-party packages, causing stale cache entries to bypass
/// the resolver on subsequent runs.
#[test]
fn stdlib_cache_save_load_excludes_third_party() {
    use passes::analyze;
    use semantics::loader::MemoryLoader;
    use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile};

    let tmp = tempfile::tempdir().unwrap();
    let project_root = tmp.path();
    let cache_dir = host_module_cache_dir(project_root, "github.com/gorilla/mux@v1.8.0");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("mux.d.lis"), "pub const VERSION: string\n").unwrap();

    let mut go_deps = std::collections::BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency::Remote {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(
        go_deps,
        Some(project_root.to_path_buf()),
        stdlib::Target::host(),
    );

    let source = r#"
import "go:github.com/gorilla/mux"

fn main() {
    let _ = mux.VERSION
}
"#;

    let no_loader = MemoryLoader::new();

    let result1 = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Directory,
        loader: &no_loader,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &resolver,
        go_module: "",
        disable_cache: false,
    });

    assert!(
        result1.errors().is_empty(),
        "first run should succeed: {:?}",
        result1.errors(),
    );

    let result2 = analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Directory,
        loader: &no_loader,
        entry: Some(EntryFile::new(
            source.to_string(),
            "main.lis".to_string(),
            "main.lis".to_string(),
        )),
        compile_phase: CompilePhase::Check,
        project_kind: semantics::ProjectKind::Binary,
        locator: &resolver,
        go_module: "",
        disable_cache: false,
    });

    assert!(
        result2.errors().is_empty(),
        "second run must not fail from stale stdlib cache: {:?}",
        result2.errors(),
    );
}
