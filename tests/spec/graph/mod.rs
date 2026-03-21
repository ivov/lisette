use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::DiagnosticSink;
use semantics::module_graph::build_module_graph;
use semantics::module_graph::kahn::topological_sort;
use semantics::store::Store;

use crate::_harness::filesystem::MockFileSystem;

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
    let result = build_module_graph(&mut store, Some(&fs), "main", &sink, false);

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
    let _result = build_module_graph(&mut store, Some(&fs), "main", &sink, false);

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
    let result = build_module_graph(&mut store, Some(&fs), "a", &sink, false);

    assert!(!result.cycles.is_empty());
}
