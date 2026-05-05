use std::fs;

fn main() {
    let path = "../../Cargo.toml";
    let toml = fs::read_to_string(path).expect("read workspace Cargo.toml");
    let version = toml
        .split("[workspace.package]")
        .nth(1)
        .and_then(|tail| {
            tail.lines().find_map(|line| {
                let line = line.trim();
                line.strip_prefix("version")
                    .and_then(|s| s.trim_start().strip_prefix('='))
                    .and_then(|s| s.trim_start().strip_prefix('"'))
                    .and_then(|s| s.split('"').next())
            })
        })
        .expect("[workspace.package].version not found");
    println!("cargo:rustc-env=LIS_VERSION={version}");
    println!("cargo:rerun-if-changed={path}");
}
