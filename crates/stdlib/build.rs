use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("stdlib_hash.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir);

    let mut prelude_contents: BTreeSet<(String, String)> = BTreeSet::new();
    let prelude_dlis_path = manifest_path.join("prelude.d.lis");
    let prelude_source =
        fs::read_to_string(&prelude_dlis_path).expect("prelude/prelude.d.lis not found");
    prelude_contents.insert(("prelude.d.lis".to_string(), prelude_source));

    let mut go_std_contents: BTreeSet<(String, String)> = BTreeSet::new();
    let go_std_dir = manifest_path.join("typedefs");
    if go_std_dir.exists() {
        collect_dlis_files(&go_std_dir, &go_std_dir, &mut go_std_contents);
    }

    let prelude_hash = compute_hash(&prelude_contents);
    let go_std_hash = compute_hash(&go_std_contents);

    let mut all_contents = prelude_contents;
    all_contents.extend(go_std_contents);
    let combined_hash = compute_hash(&all_contents);

    let mut file = fs::File::create(&dest_path).unwrap();
    writeln!(
        file,
        "/// Hash of all stdlib content (prelude.d.lis + typedefs/*.d.lis)."
    )
    .unwrap();
    writeln!(
        file,
        "pub const STDLIB_CONTENT_HASH: u64 = {combined_hash};"
    )
    .unwrap();
    writeln!(file, "/// Hash of prelude.d.lis content.").unwrap();
    writeln!(
        file,
        "pub const PRELUDE_CONTENT_HASH: u64 = {prelude_hash};"
    )
    .unwrap();
    writeln!(
        file,
        "/// Hash of Go stdlib content only (typedefs/*.d.lis)."
    )
    .unwrap();
    writeln!(file, "pub const GO_STD_CONTENT_HASH: u64 = {go_std_hash};").unwrap();

    println!("cargo:rerun-if-changed={}", prelude_dlis_path.display());
}

fn collect_dlis_files(dir: &Path, base: &Path, contents: &mut BTreeSet<(String, String)>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_dlis_files(&path, base, contents);
            } else if path.extension().map(|e| e == "lis").unwrap_or(false)
                && let Ok(content) = fs::read_to_string(&path)
            {
                println!("cargo:rerun-if-changed={}", path.display());
                let relative = path.strip_prefix(base).unwrap_or(&path);
                contents.insert((relative.to_string_lossy().to_string(), content));
            }
        }
    }
}

fn compute_hash(contents: &BTreeSet<(String, String)>) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;

    // BTreeSet ensures deterministic ordering
    for (name, content) in contents {
        for byte in name.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        for byte in content.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }

    hash
}
