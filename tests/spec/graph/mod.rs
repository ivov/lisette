use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::LocalSink;
use semantics::module_graph::build_module_graph;
use semantics::module_graph::kahn::topological_sort;
use semantics::store::Store;

use crate::_harness::filesystem::MockFileSystem;

fn default_resolver() -> deps::TypedefLocator {
    deps::TypedefLocator::default()
}

fn has_diagnostic_code(sink: &LocalSink, code: &str) -> bool {
    sink.take().iter().any(|d| d.code_str() == Some(code))
}

#[test]
fn kahn_simple_dependency_chain() {
    let mut edges = HashMap::default();
    edges.insert("a".to_string(), HashSet::from_iter(["b".to_string()]));
    edges.insert("b".to_string(), HashSet::from_iter(["c".to_string()]));
    edges.insert("c".to_string(), HashSet::default());

    let (order, cycles) = topological_sort(&edges);

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

    let (order, cycles) = topological_sort(&edges);

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

    let (_, cycles) = topological_sort(&edges);

    assert!(!cycles.is_empty());
}

#[test]
fn kahn_no_dependencies() {
    let mut edges = HashMap::default();
    edges.insert("a".to_string(), HashSet::default());
    edges.insert("b".to_string(), HashSet::default());
    edges.insert("c".to_string(), HashSet::default());

    let (order, cycles) = topological_sort(&edges);

    assert!(cycles.is_empty());
    assert_eq!(order.len(), 3);
}

#[test]
fn graph_simple_dependency() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "lib""#);
    fs.add_file("lib", "lib.lis", "fn foo() { 1 }");

    let mut store = Store::new();
    store.module_ids.push("main".to_string());
    store.module_ids.push("lib".to_string());

    let sink = LocalSink::new();
    let result = build_module_graph(
        &mut store,
        Some(&fs),
        "main",
        &sink,
        false,
        &default_resolver(),
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
fn graph_missing_module() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "missing""#);

    let mut store = Store::new();
    store.module_ids.push("main".to_string());

    let sink = LocalSink::new();
    let _result = build_module_graph(
        &mut store,
        Some(&fs),
        "main",
        &sink,
        false,
        &default_resolver(),
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
    store.module_ids.push("a".to_string());
    store.module_ids.push("b".to_string());
    store.module_ids.push("c".to_string());

    let sink = LocalSink::new();
    let result = build_module_graph(
        &mut store,
        Some(&fs),
        "a",
        &sink,
        false,
        &default_resolver(),
    );

    assert!(!result.cycles.is_empty());
}

#[test]
fn graph_standalone_third_party_go_import_uses_module_not_found() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "go:github.com/gorilla/mux""#);

    let mut store = Store::new();
    store.module_ids.push("main".to_string());

    let sink = LocalSink::new();
    let _result = build_module_graph(
        &mut store,
        Some(&fs),
        "main",
        &sink,
        true, // standalone mode
        &default_resolver(),
    );

    assert!(sink.has_errors());
    assert!(has_diagnostic_code(&sink, "resolve.module_not_found"));
}

#[test]
fn graph_project_third_party_go_import_undeclared() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.lis", r#"import "go:github.com/gorilla/mux""#);

    let mut store = Store::new();
    store.module_ids.push("main".to_string());

    let sink = LocalSink::new();
    let _result = build_module_graph(
        &mut store,
        Some(&fs),
        "main",
        &sink,
        false, // project mode
        &default_resolver(),
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
    store.module_ids.push("main".to_string());

    // Declare the dep in the resolver but do not place any .d.lis file on disk
    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(go_deps, None, None, stdlib::Target::host());

    let sink = LocalSink::new();
    let _result = build_module_graph(&mut store, Some(&fs), "main", &sink, false, &resolver);

    assert!(sink.has_errors());

    let diags = sink.take();
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
    store.module_ids.push("main".to_string());

    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "k8s.io/api".to_string(),
        deps::GoDependency {
            version: "v0.30.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(go_deps, None, None, stdlib::Target::host());

    let sink = LocalSink::new();
    let _result = build_module_graph(&mut store, Some(&fs), "main", &sink, false, &resolver);

    assert!(sink.has_errors());

    let diags = sink.take();
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
        "subpackage variant must not suggest `lis sync` or `lis check` — neither regenerates a missing subpackage typedef when the module dir already contains the root .d.lis, got: {help}",
    );
}

#[test]
fn store_get_definition_domain_style_go_module() {
    let mut store = Store::new();
    store.add_module("go:github.com/gorilla/mux");

    let module = store.get_module_mut("go:github.com/gorilla/mux").unwrap();
    module.definitions.insert(
        "go:github.com/gorilla/mux.Router".into(),
        syntax::program::Definition {
            visibility: syntax::program::Visibility::Public,
            ty: syntax::types::Type::Nominal {
                id: "go:github.com/gorilla/mux.Router".into(),
                params: vec![],
                underlying_ty: None,
            },
            name: Some("Router".into()),
            name_span: Some(syntax::ast::Span::dummy()),
            doc: None,
            body: syntax::program::DefinitionBody::Struct {
                generics: vec![],
                fields: vec![],
                kind: syntax::ast::StructKind::Record,
                methods: Default::default(),
                constructor: None,
            },
        },
    );

    // Must find the definition despite dots in the module path
    let def = store.get_definition("go:github.com/gorilla/mux.Router");
    assert!(
        def.is_some(),
        "get_definition must resolve domain-style Go module qualified names"
    );
}

#[test]
fn store_module_for_qualified_name_domain_style() {
    let mut store = Store::new();
    store.add_module("go:github.com/gorilla/mux");
    store.add_module("go:net/http");
    store.add_module("mymod");

    assert_eq!(
        store.module_for_qualified_name("go:github.com/gorilla/mux.Router"),
        Some("go:github.com/gorilla/mux"),
    );
    assert_eq!(
        store.module_for_qualified_name("go:net/http.Request"),
        Some("go:net/http"),
    );
    assert_eq!(
        store.module_for_qualified_name("mymod.MyType"),
        Some("mymod"),
    );
    // Value enum variant: three dot-separated segments
    assert_eq!(
        store.module_for_qualified_name("go:github.com/gorilla/mux.Method.Get"),
        Some("go:github.com/gorilla/mux"),
    );
}

#[test]
fn stdlib_cache_excludes_third_party_modules() {
    // The stdlib cache save filters modules by id.starts_with("go:") and
    // !id.contains('/') after stripping "go:". Third-party modules like
    // "go:github.com/gorilla/mux" contain '/' and must be excluded.
    let third_party = "go:github.com/gorilla/mux";
    let stdlib = "go:net/http";

    // The canonical check: deps::is_third_party returns true for third-party
    // paths (dot in first segment), false for stdlib paths.
    let is_stdlib_go = |id: &str| id.strip_prefix("go:").is_some_and(deps::is_stdlib);

    assert!(!is_stdlib_go(third_party));
    assert!(is_stdlib_go(stdlib));
    assert!(is_stdlib_go("go:fmt"));
    assert!(is_stdlib_go("go:crypto/tls"));
}

#[test]
fn store_module_for_qualified_name_major_version_suffix() {
    let mut store = Store::new();
    store.add_module("go:github.com/jackc/pgx/v5");

    assert_eq!(
        store.module_for_qualified_name("go:github.com/jackc/pgx/v5.Conn"),
        Some("go:github.com/jackc/pgx/v5"),
    );
    // Must not match a shorter prefix that is not registered
    assert_eq!(
        store.module_for_qualified_name("go:github.com/jackc/pgx.Row"),
        None,
    );
}

#[test]
fn store_module_for_qualified_name_nested_subpackage() {
    let mut store = Store::new();
    store.add_module("go:github.com/gorilla/mux");

    // Subpackage types are qualified under the same module
    assert_eq!(
        store.module_for_qualified_name("go:github.com/gorilla/mux.Router"),
        Some("go:github.com/gorilla/mux"),
    );
    // Method on a type: three segments after module prefix
    assert_eq!(
        store.module_for_qualified_name("go:github.com/gorilla/mux.Router.ServeHTTP"),
        Some("go:github.com/gorilla/mux"),
    );
}

#[test]
fn resolver_root_vs_subpackage_typedef_lookup() {
    use std::collections::BTreeMap;

    let tmp = tempfile::tempdir().unwrap();

    // Set up cache with root package and subpackage
    let home = tmp.path().join("home").to_string_lossy().to_string();
    let root_dir = deps::typedef_cache_dir(&home).join("github.com/gorilla/mux@v1.8.0");
    let sub_dir = root_dir.join("middleware");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(root_dir.join("mux.d.lis"), "// root\n").unwrap();
    std::fs::write(sub_dir.join("middleware.d.lis"), "// sub\n").unwrap();

    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(
        go_deps,
        None,
        Some(tmp.path().join("home").to_string_lossy().to_string()),
        stdlib::Target::host(),
    );

    // Root package resolves to root .d.lis
    match resolver.find_typedef_content("github.com/gorilla/mux") {
        deps::TypedefLocatorResult::Found {
            content: source, ..
        } => {
            assert!(source.contains("root"));
        }
        other => panic!("Root package: expected Found, got {:?}", other),
    }

    // Subpackage resolves to subpackage .d.lis
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
/// Regression: methods.rs used `find('.')` to extract the module from a
/// qualified name, which broke on `go:github.com/gorilla/mux.Router`.
#[test]
fn third_party_go_struct_impl_methods_registered() {
    use semantics::analyze::{AnalyzeInput, CompilePhase, SemanticConfig, analyze};
    use semantics::loader::Loader;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home").to_string_lossy().to_string();
    let cache_dir = deps::typedef_cache_dir(&home).join("github.com/gorilla/mux@v1.8.0");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("mux.d.lis"),
        "pub struct Router {}\nimpl Router {\n    fn route(self, path: string) -> string\n}\npub fn new_router() -> Router\n",
    )
    .unwrap();

    let mut go_deps = std::collections::BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(go_deps, None, Some(home), stdlib::Target::host());

    let source = r#"
import "go:github.com/gorilla/mux"

fn main() {
    let r = mux.new_router()
    r.route("/api")
}
"#;

    struct NoLoader;
    impl Loader for NoLoader {
        fn scan_folder(&self, _: &str) -> rustc_hash::FxHashMap<String, String> {
            rustc_hash::FxHashMap::default()
        }
    }

    let build_result = syntax::build_ast(source, 0);
    let (result, _) = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: false,
            standalone_mode: false,
            load_siblings: false,
        },
        loader: &NoLoader,
        source: source.to_string(),
        filename: "main.lis".to_string(),
        ast: build_result.ast,
        project_root: None,
        compile_phase: CompilePhase::Check,
        locator: resolver,
    });

    let impl_errors: Vec<_> = result
        .errors
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
        .errors
        .iter()
        .filter(|e| e.code_str().is_some_and(|c| c == "infer.member_not_found"))
        .collect();

    assert!(
        method_errors.is_empty(),
        "method call on third-party Go struct must resolve: {:?}",
        method_errors,
    );
}

/// Third-party Go modules must not be saved into the stdlib definition
/// cache. Regression: analyze.rs filtered by `starts_with("go:")` which
/// included third-party modules, causing stale cache entries to bypass
/// the resolver on subsequent runs.
#[test]
fn stdlib_cache_save_load_excludes_third_party() {
    use semantics::analyze::{AnalyzeInput, CompilePhase, SemanticConfig, analyze};
    use semantics::loader::Loader;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home").to_string_lossy().to_string();
    let cache_dir = deps::typedef_cache_dir(&home).join("github.com/gorilla/mux@v1.8.0");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("mux.d.lis"), "pub const VERSION: string\n").unwrap();

    let mut go_deps = std::collections::BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );
    let resolver = deps::TypedefLocator::new(go_deps, None, Some(home), stdlib::Target::host());

    let source = r#"
import "go:github.com/gorilla/mux"

fn main() {
    mux.VERSION
}
"#;

    struct NoLoader;
    impl Loader for NoLoader {
        fn scan_folder(&self, _: &str) -> rustc_hash::FxHashMap<String, String> {
            rustc_hash::FxHashMap::default()
        }
    }

    let build_result = syntax::build_ast(source, 0);

    // First run — registers third-party module
    let (result1, _) = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: false,
            standalone_mode: false,
            load_siblings: false,
        },
        loader: &NoLoader,
        source: source.to_string(),
        filename: "main.lis".to_string(),
        ast: build_result.ast.clone(),
        project_root: None,
        compile_phase: CompilePhase::Check,
        locator: resolver.clone(),
    });

    assert!(
        result1.errors.is_empty(),
        "first run should succeed: {:?}",
        result1.errors,
    );

    // Second run — must still succeed (not load stale cache for third-party)
    let (result2, _) = analyze(AnalyzeInput {
        config: SemanticConfig {
            run_lints: false,
            standalone_mode: false,
            load_siblings: false,
        },
        loader: &NoLoader,
        source: source.to_string(),
        filename: "main.lis".to_string(),
        ast: build_result.ast,
        project_root: None,
        compile_phase: CompilePhase::Check,
        locator: resolver,
    });

    assert!(
        result2.errors.is_empty(),
        "second run must not fail from stale stdlib cache: {:?}",
        result2.errors,
    );
}
