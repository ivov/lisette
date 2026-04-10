use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::DiagnosticSink;
use semantics::module_graph::build_module_graph;
use semantics::module_graph::kahn::topological_sort;
use semantics::store::Store;

use crate::_harness::filesystem::MockFileSystem;

fn default_resolver() -> deps::GoDepResolver {
    deps::GoDepResolver::default()
}

fn has_diagnostic_code(sink: &DiagnosticSink, code: &str) -> bool {
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

    let sink = DiagnosticSink::new();
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

    let sink = DiagnosticSink::new();
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

    let sink = DiagnosticSink::new();
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

    let sink = DiagnosticSink::new();
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

    let sink = DiagnosticSink::new();
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
    let resolver = deps::GoDepResolver::new(go_deps, None, None);

    let sink = DiagnosticSink::new();
    let _result = build_module_graph(&mut store, Some(&fs), "main", &sink, false, &resolver);

    assert!(sink.has_errors());
    assert!(has_diagnostic_code(&sink, "resolve.missing_go_typedef"));
}

#[test]
fn resolver_project_override_takes_precedence_over_cache() {
    use std::collections::BTreeMap;

    let tmp = tempfile::tempdir().unwrap();
    let project_root = tmp.path();

    // Set up override and cache with different content
    let override_dir = project_root.join(".lisette/deps/go/github.com/gorilla/mux@v1.8.0");
    std::fs::create_dir_all(&override_dir).unwrap();
    std::fs::write(override_dir.join("mux.d.lis"), "// override version\n").unwrap();

    let cache_dir = tmp
        .path()
        .join("fake_home/.lisette/cache/go/github.com/gorilla/mux@v1.8.0");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("mux.d.lis"), "// cache version\n").unwrap();

    let mut go_deps = BTreeMap::new();
    go_deps.insert(
        "github.com/gorilla/mux".to_string(),
        deps::GoDependency {
            version: "v1.8.0".to_string(),
            via: None,
        },
    );

    let resolver = deps::GoDepResolver::new(
        go_deps,
        Some(project_root.to_path_buf()),
        Some(tmp.path().join("fake_home").to_string_lossy().to_string()),
    );

    match resolver.resolve("github.com/gorilla/mux") {
        deps::GoTypedefResult::Found { source, origin } => {
            assert_eq!(origin, deps::TypedefOrigin::ProjectOverride);
            assert!(source.contains("override version"));
        }
        other => panic!("Expected Found with ProjectOverride, got {:?}", other),
    }
}
