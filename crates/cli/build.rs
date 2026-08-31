use std::env;
use std::fs;
use std::path::Path;

fn read_version(manifest_dir: &str, file: &str) -> String {
    let path = Path::new(manifest_dir).join(file);
    let version = fs::read_to_string(&path).unwrap_or_else(|_| panic!("failed to read {file}"));
    let version = version.trim().to_string();
    let parts: Vec<&str> = version.split('.').collect();
    assert!(
        parts.len() >= 2
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
        "{file} must hold a Go version like 1.25 or 1.25.10, got: {version:?}"
    );
    version
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let toolchain = read_version(&manifest_dir, "go-toolchain");
    let language = read_version(&manifest_dir, "go-language");

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(
        format!("{out_dir}/go_versions.rs"),
        format!(
            "pub const GO_TOOLCHAIN_VERSION: &str = \"{toolchain}\";\npub const GO_LANGUAGE_VERSION: &str = \"{language}\";"
        ),
    )
    .unwrap();

    println!("cargo:rerun-if-changed=go-toolchain");
    println!("cargo:rerun-if-changed=go-language");
}
