fn main() {
    let manifest = std::fs::read_to_string("../../Cargo.toml").expect("failed to read Cargo.toml");

    let version = manifest
        .lines()
        .find(|l| l.contains("go-version"))
        .and_then(|l| l.split('"').nth(1))
        .expect("go-version not found in Cargo.toml");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(
        format!("{out_dir}/go_version.rs"),
        format!("pub const GO_VERSION: &str = \"{}\";", version),
    )
    .unwrap();

    println!("cargo:rerun-if-changed=../../Cargo.toml");
}
