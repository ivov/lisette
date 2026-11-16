use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// A registry build has no sibling crate folders, so its version alone identifies it.
const COMPILER_CRATES: &[&str] = &[
    "syntax",
    "semantics",
    "diagnostics",
    "deps",
    "stdlib",
    "passes",
    "emit",
];

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let crates_dir = manifest_dir.parent().unwrap();

    let mut hash = FNV_OFFSET;
    fnv(&mut hash, env::var("CARGO_PKG_VERSION").unwrap().as_bytes());

    for name in COMPILER_CRATES {
        let src = crates_dir.join(name).join("src");
        if !src.is_dir() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", src.display());
        let mut files = Vec::new();
        collect_files(&src, &mut files);
        files.sort();
        for file in files {
            let Ok(bytes) = fs::read(&file) else {
                continue;
            };
            let relative = file.strip_prefix(crates_dir).unwrap_or(&file);
            fnv(&mut hash, relative.to_string_lossy().as_bytes());
            fnv(&mut hash, &bytes);
        }
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(
        out_dir.join("compiler_build_hash.rs"),
        format!("{hash}u64\n"),
    )
    .unwrap();
}
