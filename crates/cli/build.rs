use std::env;
use std::fs;
use std::path::Path;
fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let go_toolchain_path = Path::new(&manifest_dir).join("go-toolchain");

    let go_toolchain = fs::read_to_string(&go_toolchain_path).expect("failed to read go-toolchain");
    let go_toolchain = go_toolchain.trim();
    assert!(
        go_toolchain.chars().all(|c| c.is_ascii_digit() || c == '.'),
        "go-toolchain contains invalid characters: {go_toolchain}"
    );

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(
        format!("{out_dir}/go_version.rs"),
        format!("pub const GO_TOOLCHAIN_VERSION: &str = \"{go_toolchain}\";"),
    )
    .unwrap();

    println!("cargo:rerun-if-changed=go-toolchain");
}
