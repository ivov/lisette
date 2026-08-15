use std::env;
use std::fs;
use std::path::Path;
fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let go_version_path = Path::new(&manifest_dir).join("go-version");

    let go_version = fs::read_to_string(&go_version_path).expect("failed to read go-version");
    let go_version = go_version.trim();
    assert!(
        go_version.chars().all(|c| c.is_ascii_digit() || c == '.'),
        "go-version contains invalid characters: {go_version}"
    );

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(
        format!("{out_dir}/go_version.rs"),
        format!("pub const GO_VERSION: &str = \"{go_version}\";"),
    )
    .unwrap();

    println!("cargo:rerun-if-changed=go-version");
}
