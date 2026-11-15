use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// `lisette.test/root` imports `lisette.test/mid/used`. The sibling package
/// `lisette.test/mid/unused` imports `lisette.test/unreached`, which no
/// import of the root reaches.
const MODULES: &[(&str, &[(&str, &str)])] = &[
    (
        "lisette.test/root",
        &[
            (
                "go.mod",
                "module lisette.test/root\n\ngo 1.21\n\nrequire lisette.test/mid v1.0.0\n",
            ),
            (
                "root.go",
                "package root\n\nimport \"lisette.test/mid/used\"\n\nfunc Origin() used.Point { return used.Point{X: 7} }\n",
            ),
        ],
    ),
    (
        "lisette.test/mid",
        &[
            (
                "go.mod",
                "module lisette.test/mid\n\ngo 1.21\n\nrequire lisette.test/unreached v1.0.0\n",
            ),
            (
                "used/used.go",
                "package used\n\ntype Point struct{ X int }\n",
            ),
            (
                "unused/unused.go",
                "package unused\n\nimport \"lisette.test/unreached\"\n\nfunc Make() unreached.Value { return unreached.Value{N: 3} }\n",
            ),
        ],
    ),
    (
        "lisette.test/unreached",
        &[
            ("go.mod", "module lisette.test/unreached\n\ngo 1.21\n"),
            (
                "unreached.go",
                "package unreached\n\ntype Value struct{ N int }\n",
            ),
        ],
    ),
];

const VERSION: &str = "v1.0.0";

fn go_available() -> bool {
    Command::new("go").arg("version").output().is_ok()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn go_env() -> &'static (PathBuf, PathBuf) {
    static ENV: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    ENV.get_or_init(|| {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("e2e_go_dependencies");
        let proxy = base.join("proxy");
        let modcache = base.join("modcache");

        // The module cache keeps bindgen between runs, but not old fixture content.
        for stale in [
            modcache.join("lisette.test"),
            modcache.join("cache/download/lisette.test"),
        ] {
            make_writable(&stale);
            let _ = fs::remove_dir_all(&stale);
        }
        let _ = fs::remove_dir_all(&proxy);

        for (module, files) in MODULES {
            write_proxy_module(&proxy, module, files);
        }
        (proxy, modcache)
    })
}

fn make_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let mut permissions = metadata.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    let _ = fs::set_permissions(path, permissions);
    if metadata.is_dir()
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            make_writable(&entry.path());
        }
    }
}

fn write_proxy_module(proxy: &Path, module: &str, files: &[(&str, &str)]) {
    let dir = proxy.join(module).join("@v");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("list"), format!("{VERSION}\n")).unwrap();
    fs::write(
        dir.join(format!("{VERSION}.info")),
        format!("{{\"Version\":\"{VERSION}\",\"Time\":\"2026-01-01T00:00:00Z\"}}"),
    )
    .unwrap();
    let go_mod = files.iter().find(|(name, _)| *name == "go.mod").unwrap().1;
    fs::write(dir.join(format!("{VERSION}.mod")), go_mod).unwrap();

    let entries: Vec<(String, &[u8])> = files
        .iter()
        .map(|(name, content)| (format!("{module}@{VERSION}/{name}"), content.as_bytes()))
        .collect();
    fs::write(dir.join(format!("{VERSION}.zip")), stored_zip(&entries)).unwrap();
}

fn stored_zip(entries: &[(String, &[u8])]) -> Vec<u8> {
    const DOS_DATE_1980_01_01: u16 = 0x21;
    let mut out = Vec::new();
    let mut central = Vec::new();

    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_len = name.len() as u16;

        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        for field in [20u16, 0, 0, 0, DOS_DATE_1980_01_01] {
            out.extend_from_slice(&field.to_le_bytes());
        }
        for field in [crc, size, size] {
            out.extend_from_slice(&field.to_le_bytes());
        }
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        for field in [20u16, 20, 0, 0, 0, DOS_DATE_1980_01_01] {
            central.extend_from_slice(&field.to_le_bytes());
        }
        for field in [crc, size, size] {
            central.extend_from_slice(&field.to_le_bytes());
        }
        for field in [name_len, 0, 0, 0, 0] {
            central.extend_from_slice(&field.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }

    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    let count = entries.len() as u16;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    for field in [0u16, 0, count, count] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn lis(project: &Path, args: &[&str]) -> Output {
    let (proxy, modcache) = go_env();
    let manifest = repo().join("Cargo.toml");
    Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", "lisette", "--"])
        .args(args)
        .current_dir(project)
        .env("NO_COLOR", "1")
        // Bindgen comes from the default proxy.
        .env(
            "GOPROXY",
            format!("file://{},https://proxy.golang.org", proxy.display()),
        )
        .env("GONOSUMDB", "lisette.test")
        .env("GOPRIVATE", "")
        .env("GOMODCACHE", modcache)
        .output()
        .expect("failed to invoke lisette")
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const MAIN: &str = "import \"go:fmt\"\nimport \"go:lisette.test/root\"\n\nfn main() {\n  fmt.Println(root.Origin().X)\n}\n";

fn scaffold(go_dependencies: &str, main: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("app");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("lisette.toml"),
        format!("[project]\nname = \"app\"\nversion = \"0.1.0\"\n{go_dependencies}"),
    )
    .unwrap();
    fs::write(project.join("src/main.lis"), main).unwrap();
    (dir, project)
}

const ROOT_ONLY: &str = "\n[dependencies.go]\n\"lisette.test/root\" = \"v1.0.0\"\n";

const RECONCILED_DEPENDENCIES: &str = "[dependencies.go]
\"lisette.test/root\" = \"v1.0.0\"
\"lisette.test/mid\" = { version = \"v1.0.0\", via = [\"lisette.test/root\"] }
";

fn go_dependencies(project: &Path) -> String {
    let manifest = fs::read_to_string(project.join("lisette.toml")).unwrap();
    let start = manifest
        .find("[dependencies.go]")
        .expect("no Go dependencies");
    manifest[start..].to_string()
}

#[test]
fn add_writes_only_modules_that_imports_reach() {
    if !go_available() {
        return;
    }
    let (_dir, project) = scaffold("", MAIN);

    let added = lis(&project, &["add", "lisette.test/root"]);
    assert!(added.status.success(), "{}", combined(&added));
    assert_eq!(go_dependencies(&project), RECONCILED_DEPENDENCIES);

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn sync_adds_missing_transitive_entries() {
    if !go_available() {
        return;
    }
    let (_dir, project) = scaffold(ROOT_ONLY, MAIN);

    let synced = lis(&project, &["sync"]);
    let output = combined(&synced);
    assert!(synced.status.success(), "{output}");
    assert!(output.contains("+ Added lisette.test/mid"), "{output}");
    assert_eq!(go_dependencies(&project), RECONCILED_DEPENDENCIES);

    let again = lis(&project, &["sync"]);
    assert!(
        combined(&again).contains("Manifest already in sync"),
        "{}",
        combined(&again)
    );

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn sync_adds_entries_when_source_also_imports_a_typedef_import() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/root\"\nimport \"go:lisette.test/mid/used\"\n\nfn main() {\n  let p: used.Point = root.Origin()\n  fmt.Println(p.X)\n}\n";
    let (_dir, project) = scaffold(ROOT_ONLY, main);

    let synced = lis(&project, &["sync"]);
    assert!(synced.status.success(), "{}", combined(&synced));
    assert_eq!(go_dependencies(&project), RECONCILED_DEPENDENCIES);

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n");
}

#[test]
fn sync_adds_modules_that_a_transitive_subpackage_import_reaches() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/root\"\nimport \"go:lisette.test/mid/unused\"\n\nfn main() {\n  fmt.Println(root.Origin().X + unused.Make().N)\n}\n";
    let (_dir, project) = scaffold("", main);

    let added = lis(&project, &["add", "lisette.test/root"]);
    assert!(added.status.success(), "{}", combined(&added));
    assert_eq!(go_dependencies(&project), RECONCILED_DEPENDENCIES);

    let synced = lis(&project, &["sync"]);
    let output = combined(&synced);
    assert!(synced.status.success(), "{output}");
    assert!(
        output.contains("+ Added lisette.test/unreached"),
        "{output}"
    );
    assert_eq!(
        go_dependencies(&project),
        format!(
            "{RECONCILED_DEPENDENCIES}\"lisette.test/unreached\" = {{ version = \"v1.0.0\", via = [\"lisette.test/mid\"] }}\n"
        )
    );

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "10\n");
}

#[test]
fn sync_repeats_until_newly_declared_imports_resolve() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/root\"\nimport \"go:lisette.test/mid/unused\"\n\nfn main() {\n  fmt.Println(root.Origin().X + unused.Make().N)\n}\n";
    let (_dir, project) = scaffold(ROOT_ONLY, main);

    let synced = lis(&project, &["sync"]);
    assert!(synced.status.success(), "{}", combined(&synced));
    assert_eq!(
        go_dependencies(&project),
        format!(
            "{RECONCILED_DEPENDENCIES}\"lisette.test/unreached\" = {{ version = \"v1.0.0\", via = [\"lisette.test/mid\"] }}\n"
        )
    );

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "10\n");
}
