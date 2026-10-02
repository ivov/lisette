use std::path::Path;

use deps::TypedefLocator;
use passes::{Analysis, analyze};
use semantics::loader::MemoryLoader;
use semantics::{AnalysisScope, AnalyzeInput, CompilePhase, EntryFile, ProjectKind, RecoverTarget};
use serde_json::Value;

fn analyze_project(
    root: &Path,
    loader: &MemoryLoader,
    source: &str,
    phase: CompilePhase,
    recover: RecoverTarget,
) -> Analysis {
    analyze(AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Project(root.to_path_buf()),
        loader,
        entry: Some(EntryFile::recovering(
            source.into(),
            "main.lis".into(),
            "main.lis".into(),
        )),
        compile_phase: phase,
        project_kind: ProjectKind::Binary,
        locator: &TypedefLocator::default(),
        go_module: "example.com/cache",
        disable_cache: false,
        recover_target: recover,
    })
}

fn diagnostics(analysis: &Analysis) -> Vec<Value> {
    fn resolve_files(value: &mut Value, analysis: &Analysis) {
        match value {
            Value::Object(fields) => {
                if let Some(file_id) = fields.get_mut("file_id") {
                    let file = &analysis.emit_input.files[&(file_id.as_u64().unwrap() as u32)];
                    *file_id = Value::String(format!("{}/{}", file.package_id, file.name));
                }
                for value in fields.values_mut() {
                    resolve_files(value, analysis);
                }
            }
            Value::Array(values) => {
                for value in values {
                    resolve_files(value, analysis);
                }
            }
            _ => {}
        }
    }
    let mut values: Vec<_> = analysis
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            let mut value = serde_json::to_value(diagnostic).unwrap();
            resolve_files(&mut value, analysis);
            value
        })
        .collect();
    values.sort_by_key(Value::to_string);
    values
}

const MAIN: &str = "import \"dependency\"\nfn main() { let _ = dependency.value(2) }\n";
const WARNING: &str =
    "pub fn value(input: int) -> int {\n  let mut unused = \"héllo\"\n  input + 0\n}\n";

#[test]
fn cached_diagnostics_preserve_messages_labels_severity_help_and_fixes() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("dependency", "value.lis", WARNING);
    loader.add_file(
        "dependency",
        "other.lis",
        "pub fn other() { let unused = 1 }\n",
    );
    let cold = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(cold.errors().is_empty(), "{:?}", cold.errors());
    assert!(cold.lints().len() >= 4, "{:?}", cold.lints());
    assert!(
        cold.lints()
            .iter()
            .any(|diagnostic| diagnostic.fix().is_some())
    );
    for _ in 0..2 {
        let warm = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(warm.emit_input.cached_packages.contains("dependency"));
        assert_eq!(diagnostics(&cold), diagnostics(&warm));
    }
    let with_go = MAIN
        .replacen("fn main()", "import \"go:strings\"\nfn main()", 1)
        .replace(
            "let _ = dependency",
            "let _ = strings.TrimSpace(\"x\"); let _ = dependency",
        );
    let shifted = analyze_project(
        root.path(),
        &loader,
        &with_go,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(shifted.emit_input.cached_packages.contains("dependency"));
    assert_eq!(diagnostics(&cold), diagnostics(&shifted));
}

#[test]
fn source_changes_and_allow_attributes_invalidate_cached_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("dependency", "value.lis", WARNING);
    analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    for source in [
        "pub fn value(input: int) -> int { input + 0 }\n",
        "#[allow(redundant_operation)]\npub fn value(input: int) -> int { input + 0 }\n",
        "pub fn value(input: int) -> int { input + 1 }\n",
    ] {
        loader.add_file("dependency", "value.lis", source);
        let cold = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(!cold.emit_input.cached_packages.contains("dependency"));
        let warm = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(warm.emit_input.cached_packages.contains("dependency"));
        assert_eq!(diagnostics(&cold), diagnostics(&warm));
        assert_eq!(
            warm.lints().is_empty(),
            source.contains("#[allow") || source.contains("+ 1")
        );
    }
}

#[test]
fn cached_diagnostics_follow_compilation_mode_and_unused_item_policy() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file(
        "dependency",
        "value.lis",
        "pub fn value(input: int) -> int { input + 0 }\nfn unused() {}\n",
    );
    let emit = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Emit,
        RecoverTarget::None,
    );
    assert!(
        emit.lints()
            .iter()
            .all(|diagnostic| diagnostic.code_str() != Some("lint.unused_function"))
    );
    let check = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(!check.emit_input.cached_packages.contains("dependency"));
    assert!(
        check
            .lints()
            .iter()
            .any(|diagnostic| diagnostic.code_str() == Some("lint.unused_function"))
    );
    let repeated = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(!repeated.emit_input.cached_packages.contains("dependency"));
    assert_eq!(diagnostics(&check), diagnostics(&repeated));
}

#[test]
fn cached_diagnostics_obey_parse_and_permission_error_suppression() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("dependency", "value.lis", WARNING);
    analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    for source in [
        "import \"dependency\"\nfn main() { let n = 1; n = dependency.value(2) }\n",
        "import \"dependency\"\nfn main() { let _ = dependency.value(2) }\nfn broken(\n",
        "import \"dependency\"\nimport \"missing\"\nfn main() { let _ = dependency.value(2) }\n",
    ] {
        let warm = analyze_project(
            root.path(),
            &loader,
            source,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        let uncached_root = tempfile::tempdir().unwrap();
        let cold = analyze_project(
            uncached_root.path(),
            &loader,
            source,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(!cold.errors().is_empty());
        assert!(warm.emit_input.cached_packages.contains("dependency"));
        assert_eq!(diagnostics(&cold), diagnostics(&warm));
    }
}

#[test]
fn diagnostic_cache_keeps_focused_package_syntax_trees() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("dependency", "value.lis", WARNING);
    analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    let focused = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::Package("dependency".into()),
    );
    assert!(!focused.emit_input.cached_packages.contains("dependency"));
    assert!(
        focused
            .emit_input
            .files
            .values()
            .any(|file| file.package_id == "dependency" && !file.items.is_empty())
    );
    assert!(
        focused
            .bindings()
            .values()
            .any(|binding| binding.name == "unused")
    );
}

#[test]
fn dependency_changes_invalidate_replayed_diagnostics_transitively() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("base", "base.lis", "pub fn amount() -> int { 1 }\n");
    loader.add_file(
        "dependency",
        "value.lis",
        "import \"base\"\npub fn value(input: int) -> int { input + base.amount() + 0 }\n",
    );
    analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    let warm = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(warm.emit_input.cached_packages.contains("dependency"));
    loader.add_file(
        "base",
        "base.lis",
        "pub fn amount() -> string { \"changed\" }\n",
    );
    let changed = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(!changed.emit_input.cached_packages.contains("dependency"));
    assert!(!changed.errors().is_empty());
}

#[test]
fn unrepresentable_diagnostics_keep_the_package_uncached() {
    for source in [
        "struct Hidden {}\npub fn hidden(_: Hidden) {}\npub fn value(input: int) -> int { input }\n",
        "pub fn value(input: int) -> int { input as int }\n",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut loader = MemoryLoader::new();
        loader.add_file("dependency", "value.lis", source);
        let cold = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(!cold.lints().is_empty(), "{:?}", cold.errors());
        let warm = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(!warm.emit_input.cached_packages.contains("dependency"));
        assert_eq!(diagnostics(&cold), diagnostics(&warm));
    }
}

#[test]
fn diagnostic_cache_preserves_internal_test_warnings_and_test_discovery() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file(
        "dependency",
        "value.lis",
        "pub fn value(input: int) -> int { input }\n",
    );
    for source in [
        "#[test]\nfn check_value() { let unused = value(1) }\n",
        "#[test]\nfn check_value() { let _ = value(1) }\n",
    ] {
        loader.add_file("dependency", "value.test.lis", source);
        let cold = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        let warm = analyze_project(
            root.path(),
            &loader,
            MAIN,
            CompilePhase::Check,
            RecoverTarget::None,
        );
        assert!(warm.errors().is_empty(), "{:?}", warm.errors());
        assert!(!cold.emit_input.cached_packages.contains("dependency"));
        assert!(warm.emit_input.cached_packages.contains("dependency"));
        assert_eq!(diagnostics(&cold), diagnostics(&warm));
        assert_eq!(warm.lints().len(), usize::from(source.contains("unused")));
        assert!(
            warm.emit_input
                .test_index
                .contains_qualified("dependency.check_value")
        );
        assert_eq!(
            warm.emit_input.test_index.tests().len(),
            cold.emit_input.test_index.tests().len()
        );
    }
}

#[test]
fn third_party_typedefs_prevent_replaying_and_saving_dependency_warnings() {
    use deps::GoDependency;
    use std::collections::BTreeMap;
    use std::fs;

    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("dependency", "value.lis", WARNING);
    analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    let typedef = deps::typedef_cache_dir(root.path())
        .join(stdlib::Target::host().cache_segment())
        .join("example.com/native@v1.0.0");
    fs::create_dir_all(&typedef).unwrap();
    fs::write(typedef.join("native.d.lis"), "pub const VERSION: string\n").unwrap();
    let locator = TypedefLocator::new(
        BTreeMap::from([(
            "example.com/native".into(),
            GoDependency::Remote {
                version: "v1.0.0".into(),
                via: None,
            },
        )]),
        Some(root.path().to_path_buf()),
        stdlib::Target::host(),
    );
    let source = MAIN
        .replace("fn main()", "import \"go:example.com/native\"\nfn main()")
        .replace(
            "let _ = dependency",
            "let _ = native.VERSION; let _ = dependency",
        );
    let run = || {
        analyze(AnalyzeInput {
            load_siblings: false,
            scope: AnalysisScope::Project(root.path().to_path_buf()),
            loader: &loader,
            entry: Some(EntryFile::new(
                source.clone(),
                "main.lis".into(),
                "main.lis".into(),
            )),
            compile_phase: CompilePhase::Check,
            project_kind: ProjectKind::Binary,
            locator: &locator,
            go_module: "example.com/cache",
            disable_cache: false,
            recover_target: RecoverTarget::None,
        })
    };
    let warm = run();
    assert!(warm.errors().is_empty(), "{:?}", warm.errors());
    assert!(!warm.emit_input.cached_packages.contains("dependency"));
    let cache = root.path().join("target/.lisette/cache");
    fs::remove_dir_all(&cache).unwrap();
    let cold = run();
    assert_eq!(diagnostics(&cold), diagnostics(&warm));
    assert!(!cache.exists());
}

#[test]
fn importer_dependent_method_warnings_are_not_hidden_by_the_diagnostic_cache() {
    let root = tempfile::tempdir().unwrap();
    let mut loader = MemoryLoader::new();
    loader.add_file("dependency", "item.lis", "pub struct Item { pub value: int }\nimpl Item {\n  fn equals(self, other: Item) -> bool { self.value == other.value }\n}\npub fn value(input: int) -> int { input + 0 }\n");
    let use_method = "import \"dependency\"\ninterface Eq { fn equals(other: dependency.Item) -> bool }\nfn accept(_item: Eq) {}\nfn main() { accept(dependency.Item { value: 1 }); let _ = dependency.value(1) }\n";
    let first = analyze_project(
        root.path(),
        &loader,
        use_method,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(first.errors().is_empty(), "{:?}", first.errors());
    assert!(
        first
            .lints()
            .iter()
            .all(|diagnostic| diagnostic.code_str() != Some("lint.unused_function")),
        "{:?}",
        first.lints()
    );
    let changed = analyze_project(
        root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    let cold_root = tempfile::tempdir().unwrap();
    let cold = analyze_project(
        cold_root.path(),
        &loader,
        MAIN,
        CompilePhase::Check,
        RecoverTarget::None,
    );
    assert!(
        cold.lints()
            .iter()
            .any(|diagnostic| diagnostic.code_str() == Some("lint.unused_function"))
    );
    assert_eq!(diagnostics(&cold), diagnostics(&changed));
}
