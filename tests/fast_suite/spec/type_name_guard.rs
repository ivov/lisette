use std::fs;
use std::path::{Path, PathBuf};

const SPELLING_CHECKS: &[&str] = &["== \"", "!= \"", "Some(\"", ".as_str()", "matches!("];

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read crate dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn no_decision_reads_a_display_name() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
    let mut files = Vec::new();
    rust_files(&crates, &mut files);
    let mut offenders = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("read source file");
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains("display_name()") {
                continue;
            }
            let statement = lines[index..(index + 3).min(lines.len())].join(" ");
            if SPELLING_CHECKS
                .iter()
                .any(|check| statement.contains(check))
            {
                offenders.push(format!("{}:{}", file.display(), index + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "decide a type property with `Store::peeled` or the underlying kind, not a display name:\n{}",
        offenders.join("\n")
    );
}
