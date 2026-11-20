use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::_harness::build::{compile_check, try_compile_project_files};
use crate::_harness::filesystem::MockFileSystem;
use semantics::store::ENTRY_PACKAGE_ID;

type ProjectFiles<'a> = &'a [(&'a str, &'a str, &'a str)];

fn project(files: ProjectFiles<'_>) -> MockFileSystem {
    let mut fs = MockFileSystem::new();
    for (package, name, source) in files {
        fs.add_file(package, name, source);
    }
    fs
}

fn check_has_errors(files: ProjectFiles<'_>) -> bool {
    compile_check(project(files))
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.is_error())
}

fn leading_identifier(text: &str) -> String {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("")
        .to_string()
}

fn package_block_go_names(files: &[emit::OutputFile]) -> BTreeMap<String, Vec<String>> {
    let mut by_directory: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let directory = file
            .name
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory)
            .to_string();
        let names = by_directory.entry(directory).or_default();
        for import in &file.imports {
            let qualifier = import.qualifier.clone().unwrap_or_else(|| {
                import
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&import.path)
                    .to_string()
            });
            if qualifier != "_" {
                names.push(qualifier);
            }
        }
        let mut in_group = false;
        for line in file.to_go().lines() {
            if in_group {
                if line == ")" {
                    in_group = false;
                } else if let Some(rest) = line.strip_prefix('\t')
                    && !rest.starts_with('\t')
                    && !rest.starts_with("//")
                {
                    names.push(leading_identifier(rest));
                }
                continue;
            }
            if matches!(line, "const (" | "var (" | "type (") {
                in_group = true;
                continue;
            }
            for keyword in ["func ", "type ", "const ", "var "] {
                if let Some(rest) = line.strip_prefix(keyword)
                    && !rest.starts_with('(')
                {
                    names.push(leading_identifier(rest));
                }
            }
        }
        names.retain(|name| !name.is_empty() && name != "_" && name != "init");
    }
    by_directory
}

fn check_clean_implies_distinct_go_names(
    label: &str,
    files: ProjectFiles<'_>,
) -> Result<(), String> {
    if check_has_errors(files) {
        return Ok(());
    }
    let emitted = try_compile_project_files(project(files), "github.com/user/myproject", false)
        .map_err(|diagnostics| {
            let codes: Vec<_> = diagnostics.iter().filter_map(|d| d.code_str()).collect();
            format!("{label}: passes check but emit rejects it: {codes:?}")
        })?;
    for (directory, mut names) in package_block_go_names(&emitted) {
        names.sort();
        let duplicates: Vec<_> = names.windows(2).filter(|pair| pair[0] == pair[1]).collect();
        if !duplicates.is_empty() {
            return Err(format!(
                "{label}: package `{directory}` declares Go names twice: {duplicates:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn check_clean_programs_declare_distinct_package_go_names() {
    let cases: &[(&str, ProjectFiles<'_>)] = &[
        (
            "two interfaces with one name",
            &[(
                ENTRY_PACKAGE_ID,
                "main.lis",
                "interface Shape { fn area() -> int }\ninterface Shape { fn area() -> int }\nfn main() {}\n",
            )],
        ),
        (
            "escape remap and an alias spelled like its number",
            &[(
                ENTRY_PACKAGE_ID,
                "main.lis",
                "import \"go:fmt\"\nimport fooBar2 \"go:strconv\"\nfn foo_bar() -> int { 1 }\nfn fooBar() -> int { 2 }\nfn main() { fmt.Println(foo_bar(), fooBar(), fooBar2.Itoa(3)) }\n",
            )],
        ),
        (
            "escape remap and a struct spelled like its natural name",
            &[(
                ENTRY_PACKAGE_ID,
                "main.lis",
                "import \"go:fmt\"\nstruct fooBar { n: int }\nfn foo_bar() -> int { 1 }\nfn main() { fmt.Println(foo_bar(), fooBar { n: 2 }.n) }\n",
            )],
        ),
        (
            "`_pkg` alias and a struct spelled like it",
            &[
                (
                    ENTRY_PACKAGE_ID,
                    "main.lis",
                    "import \"go:fmt\"\nimport \"other\"\nfn main() { fmt.Println(other.wait(), other.helper()) }\n",
                ),
                (
                    "other",
                    "a.lis",
                    "import \"go:time\"\npub fn wait() -> time.Duration { time.Second }\n",
                ),
                (
                    "other",
                    "b.lis",
                    "pub fn helper() -> string { f\"{time()}{time_pkg { n: 1 }.n}\" }\nfn time() -> string { \"t\" }\nstruct time_pkg { n: int }\n",
                ),
            ],
        ),
    ];
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(label, files)| check_clean_implies_distinct_go_names(label, files).err())
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

fn source_files(directory: &str) -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut pending = vec![root.join(directory)];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path).expect("read source directory") {
            let path = entry.expect("read directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path
                    .strip_prefix(&root)
                    .expect("source path is under the workspace")
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = fs::read_to_string(&path).expect("read source file");
                files.push((relative, text));
            }
        }
    }
    files.sort();
    files
}

fn non_test_code(text: &str) -> &str {
    text.split("#[cfg(test)]").next().unwrap_or(text)
}

#[test]
fn package_definitions_are_inserted_by_one_owner() {
    let allowed = [
        ("crates/semantics/src/checker/registration/mod.rs", 2),
        ("crates/semantics/src/checker/registration/methods.rs", 1),
        ("crates/semantics/src/checker/registration/types.rs", 1),
        ("crates/semantics/src/checker/registration/iterate.rs", 1),
        ("crates/semantics/src/cache/types.rs", 1),
    ];
    let mut found = Vec::new();
    for (path, text) in source_files("crates/semantics/src") {
        let compact: String = non_test_code(&text)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let count = compact.matches("definitions.insert(").count();
        if count > 0 {
            found.push((path, count));
        }
    }
    let expected: Vec<(String, usize)> = allowed
        .iter()
        .map(|(path, count)| ((*path).to_string(), *count))
        .collect();
    let mut found_sorted = found.clone();
    found_sorted.sort();
    let mut expected_sorted = expected;
    expected_sorted.sort();
    assert_eq!(
        found_sorted, expected_sorted,
        "insert package-level declarations with `define_package_item`"
    );
}

#[test]
fn package_block_names_are_numbered_by_one_allocator() {
    let allowed = [
        "crates/emit/src/names/go_name.rs",
        "crates/emit/src/state/package_state.rs",
        "crates/emit/src/state/scope/names.rs",
        "crates/emit/src/names/receiver.rs",
        "crates/emit/src/expressions/top_items.rs",
    ];
    let offenders: Vec<String> = source_files("crates/emit/src")
        .into_iter()
        .filter(|(path, text)| {
            non_test_code(text).contains("fresh_suffixed(") && !allowed.contains(&path.as_str())
        })
        .map(|(path, _)| path)
        .collect();
    assert!(
        offenders.is_empty(),
        "number package-block names with `fresh_package_name`: {offenders:?}"
    );
}

#[test]
fn import_used_beside_a_sibling_function_of_its_name_is_not_unused() {
    let files: ProjectFiles<'_> = &[
        (
            ENTRY_PACKAGE_ID,
            "main.lis",
            "import \"go:fmt\"\nimport \"other\"\nfn main() { fmt.Println(other.wait(), other.helper()) }\n",
        ),
        (
            "other",
            "a.lis",
            "import \"go:time\"\npub fn wait() -> time.Duration { time.Second }\n",
        ),
        (
            "other",
            "b.lis",
            "pub fn helper() -> string { time() }\nfn time() -> string { \"t\" }\n",
        ),
    ];
    let codes: Vec<String> = compile_check(project(files))
        .diagnostics()
        .iter()
        .filter_map(|d| d.code_str().map(str::to_string))
        .collect();
    assert!(
        !codes.iter().any(|code| code == "lint.unused_import"),
        "`time.Second` reads the import; got: {codes:?}"
    );
    assert!(check_clean_implies_distinct_go_names("import beside sibling function", files).is_ok());
}

#[test]
fn duplicate_definition_does_not_depend_on_declaration_order() {
    let declarations = [
        "struct Foo { x: int }",
        "enum Foo { A }",
        "interface Foo { fn get() -> int }",
        "type Foo = int",
        "fn Foo() {}",
        "const Foo = 1",
    ];
    let reports_duplicate = |first: &str, second: &str| {
        let source = format!("{first}\n\n{second}\n\nfn main() {{}}\n");
        compile_check(project(&[(ENTRY_PACKAGE_ID, "main.lis", &source)]))
            .diagnostics()
            .iter()
            .any(|d| d.code_str() == Some("infer.duplicate_definition"))
    };
    let mut missing = Vec::new();
    for first in declarations {
        for second in declarations {
            if !reports_duplicate(first, second) {
                missing.push(format!("`{first}` then `{second}`"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "no duplicate reported for: {missing:#?}"
    );
}
