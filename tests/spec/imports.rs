//! The graph reads imports with the scanner, which must agree with the parser.

use std::path::{Path, PathBuf};

use syntax::build_ast;
use syntax::imports::scan_imports;
use syntax::program::{File, FileImport};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests/ has a parent")
        .to_path_buf()
}

fn collect_lisette_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "target" && name != "node_modules" && !name.starts_with('.') {
                collect_lisette_files(&path, found);
            }
        } else if name.ends_with(".lis") {
            found.push(path);
        }
    }
}

fn parsed_imports(source: &str) -> Option<Vec<FileImport>> {
    let result = build_ast(source, 0);
    if !result.errors.is_empty() {
        return None;
    }
    Some(
        File {
            id: 0,
            module_id: "corpus".to_string(),
            name: "corpus.lis".to_string(),
            display_path: "corpus.lis".to_string(),
            source_path: None,
            source: source.to_string(),
            items: result.ast,
            file_comment: result.file_comment,
        }
        .imports(),
    )
}

#[test]
fn the_scanner_matches_the_parser_over_the_repository_corpus() {
    let root = repository_root();
    let mut files = Vec::new();
    collect_lisette_files(&root, &mut files);
    files.sort();

    let mut compared = 0;
    let mut with_imports = 0;
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(parsed) = parsed_imports(&source) else {
            continue;
        };
        compared += 1;
        with_imports += usize::from(!parsed.is_empty());

        assert_eq!(
            scan_imports(&source, 0),
            parsed,
            "scanner and parser disagree on {}",
            path.display()
        );
    }

    assert!(
        compared > 300 && with_imports > 100,
        "the corpus walk found too little to be meaningful: {compared} files, {with_imports} with imports"
    );
}
