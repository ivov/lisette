use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

include!(concat!(env!("OUT_DIR"), "/go_version.rs"));

use deps::TypedefLocator;
use emit::{OutputFile, PRELUDE_IMPORT_PATH};
use stdlib::Target;

pub fn go_command(target: Target) -> Command {
    let mut c = Command::new("go");
    // Isolate from any user-side env that would change Go's mode against
    // lisette's `target/`: a stray `go.work` (workspace mode) or a stray
    // `GOFLAGS=-mod=vendor` (vendor mode) both turn into multi-line errors
    // for unrelated `lis add` invocations otherwise.
    c.env("GOWORK", "off");
    c.env("GOFLAGS", "");
    c.env("GOOS", target.goos);
    c.env("GOARCH", target.goarch);
    c
}

pub fn require_go() -> Result<(), i32> {
    match go_status() {
        GoStatus::Ready => Ok(()),
        GoStatus::Absent => {
            crate::cli_error!(
                "Go is not installed",
                "`go` is not in PATH",
                "Install Go from https://go.dev/dl/"
            );
            Err(1)
        }
        GoStatus::Outdated { found, required } => {
            crate::cli_error!(
                "Go version is outdated",
                format!("Found Go {}, but {} or later is required", found, required),
                "Upgrade Go at https://go.dev/dl/"
            );
            Err(1)
        }
    }
}

pub fn is_go_present() -> bool {
    !matches!(go_status(), GoStatus::Absent)
}

pub fn go_mod_version() -> String {
    let parts: Vec<&str> = GO_VERSION.split('.').collect();
    format!(
        "{}.{}",
        parts.first().unwrap_or(&"1"),
        parts.get(1).unwrap_or(&"21")
    )
}

enum GoStatus {
    Ready,
    Absent,
    Outdated { found: String, required: String },
}

fn go_status() -> GoStatus {
    let output = match Command::new("go").arg("version").output() {
        Ok(o) => o,
        Err(_) => return GoStatus::Absent,
    };

    let version_string = String::from_utf8_lossy(&output.stdout);

    let version = version_string
        .split_whitespace()
        .find(|s| s.starts_with("go1."))
        .and_then(|s| s.strip_prefix("go"));

    let Some(version) = version else {
        return GoStatus::Absent;
    };

    let parts: Vec<&str> = version.split('.').collect();
    let [major, minor, ..] = parts.as_slice() else {
        return GoStatus::Absent;
    };

    let major: u32 = major.parse().unwrap_or(0);
    let minor: u32 = minor.parse().unwrap_or(0);

    let min_parts: Vec<&str> = GO_VERSION.split('.').collect();
    let min_major: u32 = min_parts.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    let min_minor: u32 = min_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    if major > min_major || (major == min_major && minor >= min_minor) {
        GoStatus::Ready
    } else {
        GoStatus::Outdated {
            found: version.to_string(),
            required: format!("{}.{}", min_major, min_minor),
        }
    }
}

pub fn go_fmt_paths(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cmd = Command::new("gofmt");
    cmd.arg("-w");
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run `gofmt`: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`gofmt` error: {}", stderr));
    }

    Ok(())
}

pub fn write_go_mod(dir: &Path, module_name: &str, locator: &TypedefLocator) -> Result<(), String> {
    let prelude_version = env!("CARGO_PKG_VERSION");

    let mut requires = vec![format!("\t{} v{}", PRELUDE_IMPORT_PATH, prelude_version)];

    for (module_path, dep) in locator.deps() {
        requires.push(format!("\t{} {}", module_path, dep.version));
    }

    let mut content = format!(
        "module {}\n\ngo {}\n\nrequire (\n{}\n)\n",
        module_name,
        go_mod_version(),
        requires.join("\n"),
    );

    if cfg!(debug_assertions) {
        let prelude_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prelude");
        if let Ok(canonical) = prelude_dir.canonicalize() {
            content.push_str(&format!(
                "\nreplace {} => {}\n",
                PRELUDE_IMPORT_PATH,
                canonical.display()
            ));
        }
    }

    let go_mod_path = dir.join("go.mod");
    let lisette_dir = dir.join(".lisette");
    let stamp_path = lisette_dir.join("go.mod.stamp");

    // Stamp tracks pre-tidy content; on-disk go.mod diverges after tidy prunes requires.
    let stamp_matches = go_mod_path.exists()
        && fs::read_to_string(&stamp_path).is_ok_and(|existing| existing == content);

    if !stamp_matches {
        fs::write(&go_mod_path, &content).map_err(|e| format!("Failed to write go.mod: {}", e))?;
        let _ = fs::remove_file(dir.join("go.sum"));
        let _ = fs::remove_file(lisette_dir.join("go.mod.tidy"));
        let _ = fs::create_dir_all(&lisette_dir);
        let _ = fs::write(&stamp_path, &content);
    }

    Ok(())
}

pub struct GoCliError {
    pub message: String,
    pub hint: &'static str,
}

pub struct ManifestEntry {
    pub name: String,
    pub content_hash: u64,
    pub imports: Vec<String>,
}

pub struct EmitWriteResult {
    pub changed: Vec<PathBuf>,
    pub new_manifest: Vec<ManifestEntry>,
}

pub fn write_go_outputs(dir: &Path, files: &[OutputFile]) -> Result<EmitWriteResult, GoCliError> {
    let mut prior_manifest = read_emit_manifest(dir);
    let mut new_manifest: Vec<ManifestEntry> = Vec::with_capacity(files.len());
    let mut changed: Vec<PathBuf> = Vec::with_capacity(files.len());

    for file in files {
        let go_file_path = dir.join(&file.name);
        let go_code = file.to_go_unformatted();
        let hash = hash_go_code(&go_code);
        let prior = prior_manifest.remove(&file.name);

        if let Some(entry) = prior
            && entry.content_hash == hash
            && go_file_path.exists()
        {
            new_manifest.push(entry);
            continue;
        }

        if let Some(parent) = go_file_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            return Err(GoCliError {
                message: format!("Failed to create directory `{}`: {}", parent.display(), e),
                hint: "Check directory permissions",
            });
        }

        if let Err(e) = fs::write(&go_file_path, &go_code) {
            return Err(GoCliError {
                message: format!("Failed to write `{}`: {}", go_file_path.display(), e),
                hint: "Check file permissions",
            });
        }

        let mut imports: Vec<String> = file.imports.keys().cloned().collect();
        imports.sort();
        new_manifest.push(ManifestEntry {
            name: file.name.clone(),
            content_hash: hash,
            imports,
        });
        changed.push(go_file_path);
    }

    // Preserve entries for files emit skipped this build but still on disk.
    for (name, entry) in prior_manifest {
        if dir.join(&name).exists() {
            new_manifest.push(entry);
        }
    }

    Ok(EmitWriteResult {
        changed,
        new_manifest,
    })
}

/// Hash of the sorted union of external (non-stdlib, non-local) Go imports.
pub fn compute_import_set_hash(manifest: &[ManifestEntry], go_module_name: &str) -> u64 {
    let mut paths: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for entry in manifest {
        for path in &entry.imports {
            if is_external_import(path, go_module_name) {
                paths.insert(path.as_str());
            }
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for path in paths {
        for &b in path.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h ^= b'\n' as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn is_external_import(path: &str, go_module_name: &str) -> bool {
    deps::is_third_party(path)
        && path != go_module_name
        && !path
            .strip_prefix(go_module_name)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub fn invalidate_go_mod_stamp(dir: &Path) {
    let _ = fs::remove_file(dir.join(".lisette").join("go.mod.stamp"));
}

fn emit_manifest_path(dir: &Path) -> PathBuf {
    dir.join(".lisette").join("emit-manifest")
}

// FNV-1a: deterministic across Rust versions.
fn hash_go_code(content: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in content.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn read_emit_manifest(dir: &Path) -> std::collections::HashMap<String, ManifestEntry> {
    let Ok(content) = fs::read_to_string(emit_manifest_path(dir)) else {
        return Default::default();
    };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let name = parts.next()?;
            let hash = u64::from_str_radix(parts.next()?, 16).ok()?;
            let imports = parts
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| s.split(',').map(|p| p.to_string()).collect())
                .unwrap_or_default();
            Some((
                name.to_string(),
                ManifestEntry {
                    name: name.to_string(),
                    content_hash: hash,
                    imports,
                },
            ))
        })
        .collect()
}

pub fn write_emit_manifest(dir: &Path, entries: &[ManifestEntry]) {
    let path = emit_manifest_path(dir);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let content: String = entries
        .iter()
        .map(|e| {
            format!(
                "{}\t{:016x}\t{}\n",
                e.name,
                e.content_hash,
                e.imports.join(",")
            )
        })
        .collect();
    let _ = fs::write(&path, content);
}

pub fn finalize_go_dir(
    dir: &Path,
    target: Target,
    changed_paths: &[PathBuf],
    import_set_hash: u64,
) -> Result<(), GoCliError> {
    if let Err(e) = go_fmt_paths(changed_paths) {
        return Err(GoCliError {
            message: format!("Go format failed: {}", e),
            hint: "Check Go installation with `go version`",
        });
    }

    if let Err(e) = ensure_go_sum(dir, target, import_set_hash) {
        return Err(GoCliError {
            message: format!("Failed to resolve Go dependencies: {}", e),
            hint: "Check Go installation and network connectivity",
        });
    }

    Ok(())
}

fn tidy_marker_path(dir: &Path) -> PathBuf {
    dir.join(".lisette").join("go.mod.tidy")
}

pub fn read_tidy_marker(dir: &Path) -> Option<u64> {
    let s = fs::read_to_string(tidy_marker_path(dir)).ok()?;
    u64::from_str_radix(s.trim(), 16).ok()
}

fn write_tidy_marker(dir: &Path, import_set_hash: u64) {
    let path = tidy_marker_path(dir);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, format!("{:016x}\n", import_set_hash));
}

pub fn ensure_go_sum(dir: &Path, target: Target, import_set_hash: u64) -> Result<(), String> {
    if read_tidy_marker(dir) == Some(import_set_hash) {
        return Ok(());
    }
    let result = go_mod_tidy(dir, target);
    if result.is_ok() {
        write_tidy_marker(dir, import_set_hash);
    }
    result
}

pub fn prewarm_module_cache(target: Target) {
    let prelude_version = env!("CARGO_PKG_VERSION");
    let _ = go_command(target)
        .args([
            "mod",
            "download",
            &format!("{}@v{}", PRELUDE_IMPORT_PATH, prelude_version),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn go_mod_tidy(path: &Path, target: Target) -> Result<(), String> {
    let output = go_command(target)
        .args(["mod", "tidy"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run `go mod tidy`: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`go mod tidy` error: {}", stderr));
    }

    Ok(())
}

pub fn binary_name(go_module_name: &str, target: Target) -> String {
    let stem = go_module_name.rsplit('/').next().unwrap_or(go_module_name);
    with_exe_suffix(sanitize_binary_stem(stem), target)
}

pub fn run_binary_name(target: Target) -> String {
    with_exe_suffix("lis-run".to_string(), target)
}

fn with_exe_suffix(stem: String, target: Target) -> String {
    if target.goos == "windows" {
        format!("{stem}.exe")
    } else {
        stem
    }
}

/// Stem that keeps `target/bin/` invisible to Go tooling and is a usable
/// filename on every host.
pub fn sanitize_binary_stem(name: &str) -> String {
    let mut stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();

    stem = stem.trim_start_matches(['.', '_']).to_string();

    while stem.ends_with(".go") {
        stem.truncate(stem.len() - ".go".len());
    }

    if stem.is_empty() || stem == "testdata" || stem == "vendor" || is_windows_reserved_name(&stem)
    {
        return "app".to_string();
    }

    stem
}

/// Reserved on Windows regardless of extension, keyed on the pre-dot segment
/// (`con.txt` too); checked on every host so the name is host-independent.
fn is_windows_reserved_name(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem);
    let lower = base.to_ascii_lowercase();
    if matches!(lower.as_str(), "con" | "prn" | "aux" | "nul") {
        return true;
    }
    let bytes = lower.as_bytes();
    (lower.starts_with("com") || lower.starts_with("lpt"))
        && bytes.len() == 4
        && (b'1'..=b'9').contains(&bytes[3])
}

pub fn is_go_output_flag(token: &str) -> bool {
    matches!(token, "-o" | "--o") || token.starts_with("-o=") || token.starts_with("--o=")
}

/// `go_flags` follow the default `-o` so a caller `-o` wins. `output_path` must
/// be absolute, since `go build` runs with `build_dir` as cwd.
pub fn build_binary(
    build_dir: &Path,
    output_path: &Path,
    target: Target,
    go_flags: &[String],
) -> Result<(), GoCliError> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| GoCliError {
            message: format!("Failed to create `{}`: {}", parent.display(), e),
            hint: "Check directory permissions",
        })?;
    }

    let mut cmd = go_command(target);
    cmd.arg("build").arg("-o").arg(output_path);
    for flag in go_flags {
        cmd.arg(flag);
    }
    cmd.arg(".").current_dir(build_dir);

    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(GoCliError {
            message: "`go build` failed".to_string(),
            hint: "Review the Go compiler output above",
        }),
        Err(e) => Err(GoCliError {
            message: format!("Failed to execute `go build`: {}", e),
            hint: "Check Go installation with `go version`",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux() -> Target {
        Target::new("linux", "amd64")
    }

    fn windows() -> Target {
        Target::new("windows", "amd64")
    }

    #[test]
    fn binary_name_uses_last_module_segment() {
        assert_eq!(binary_name("myproj", linux()), "myproj");
        assert_eq!(binary_name("github.com/u/myproj", linux()), "myproj");
    }

    #[test]
    fn is_go_output_flag_catches_every_go_spelling() {
        assert!(is_go_output_flag("-o"));
        assert!(is_go_output_flag("--o"));
        assert!(is_go_output_flag("-o=dist/app"));
        assert!(is_go_output_flag("--o=dist/app"));
        assert!(!is_go_output_flag("-trimpath"));
        assert!(!is_go_output_flag("-ofoo"));
        assert!(!is_go_output_flag("--output"));
    }

    #[test]
    fn binary_name_appends_exe_on_windows() {
        assert_eq!(binary_name("myproj", windows()), "myproj.exe");
        assert_eq!(run_binary_name(windows()), "lis-run.exe");
        assert_eq!(run_binary_name(linux()), "lis-run");
    }

    #[test]
    fn sanitize_replaces_illegal_chars() {
        assert_eq!(sanitize_binary_stem("weird name!"), "weird_name_");
    }

    #[test]
    fn sanitize_never_ends_in_go() {
        assert_eq!(sanitize_binary_stem("foo.go"), "foo");
        assert_eq!(sanitize_binary_stem("foo.go.go"), "foo");
    }

    #[test]
    fn sanitize_strips_leading_reserved_prefixes() {
        assert_eq!(sanitize_binary_stem(".hidden"), "hidden");
        assert_eq!(sanitize_binary_stem("_x"), "x");
    }

    #[test]
    fn sanitize_falls_back_for_empty_or_reserved() {
        assert_eq!(sanitize_binary_stem("___"), "app");
        assert_eq!(sanitize_binary_stem("testdata"), "app");
        assert_eq!(sanitize_binary_stem("vendor"), "app");
    }

    #[test]
    fn sanitize_avoids_windows_reserved_device_names() {
        assert_eq!(sanitize_binary_stem("con"), "app");
        assert_eq!(sanitize_binary_stem("CON"), "app");
        assert_eq!(sanitize_binary_stem("NuL"), "app");
        assert_eq!(sanitize_binary_stem("com1"), "app");
        assert_eq!(sanitize_binary_stem("LPT9"), "app");
        assert_eq!(sanitize_binary_stem("com0"), "com0");
        assert_eq!(sanitize_binary_stem("com10"), "com10");
        assert_eq!(sanitize_binary_stem("console"), "console");
    }

    #[test]
    fn sanitize_avoids_dotted_windows_reserved_names() {
        assert_eq!(sanitize_binary_stem("con.txt"), "app");
        assert_eq!(sanitize_binary_stem("NUL.anything"), "app");
        assert_eq!(sanitize_binary_stem("com1.foo"), "app");
        assert_eq!(sanitize_binary_stem("foo.con"), "foo.con");
        assert_eq!(sanitize_binary_stem("console.txt"), "console.txt");
    }
}
