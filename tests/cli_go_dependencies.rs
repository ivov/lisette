#[allow(dead_code, unused_imports)]
mod _harness;

use _harness::go::go_available;
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
    (
        "lisette.test/platform",
        &[
            (
                "go.mod",
                "module lisette.test/platform\n\ngo 1.21\n\nrequire (\n\tlisette.test/windowsonly v1.0.0\n\tlisette.test/tagonly v1.0.0\n\tlisette.test/subpackageonly v1.0.0\n)\n",
            ),
            (
                "platform.go",
                "package platform\n\nfunc Hello() string { return \"hi\" }\n",
            ),
            (
                "platform_windows.go",
                "package platform\n\nimport \"lisette.test/windowsonly\"\n\nfunc Handle() windowsonly.Value { return windowsonly.Value{} }\n",
            ),
            (
                "platform_purego.go",
                "//go:build purego\n\npackage platform\n\nimport \"lisette.test/tagonly\"\n\nfunc Pure() tagonly.Value { return tagonly.Value{} }\n",
            ),
            (
                "generate.go",
                "//go:build ignore\n\npackage main\n\nimport _ \"lisette.test/ignored\"\n\nfunc main() {}\n",
            ),
            (
                "win/win_windows.go",
                "package win\n\nimport \"lisette.test/subpackageonly\"\n\nfunc Make() subpackageonly.Value { return subpackageonly.Value{} }\n",
            ),
        ],
    ),
    (
        "lisette.test/windowsonly",
        &[
            ("go.mod", "module lisette.test/windowsonly\n\ngo 1.21\n"),
            (
                "windowsonly.go",
                "package windowsonly\n\ntype Value struct{ N int }\n",
            ),
        ],
    ),
    (
        "lisette.test/tagonly",
        &[
            ("go.mod", "module lisette.test/tagonly\n\ngo 1.21\n"),
            (
                "tagonly.go",
                "package tagonly\n\ntype Value struct{ N int }\n",
            ),
        ],
    ),
    (
        "lisette.test/subpackageonly",
        &[
            ("go.mod", "module lisette.test/subpackageonly\n\ngo 1.21\n"),
            (
                "subpackageonly.go",
                "package subpackageonly\n\ntype Value struct{ N int }\n",
            ),
        ],
    ),
    (
        "lisette.test/diamond",
        &[
            (
                "go.mod",
                "module lisette.test/diamond\n\ngo 1.21\n\nrequire (\n\tlisette.test/left v1.0.0\n\tlisette.test/right v1.0.0\n)\n",
            ),
            (
                "diamond.go",
                "package diamond\n\nimport \"lisette.test/left\"\n\nfunc Hello() string { return left.Name() }\n",
            ),
            (
                "diamond_windows.go",
                "package diamond\n\nimport \"lisette.test/right\"\n\nfunc Other() string { return right.Name() }\n",
            ),
        ],
    ),
    (
        "lisette.test/left",
        &[
            (
                "go.mod",
                "module lisette.test/left\n\ngo 1.21\n\nrequire lisette.test/leaf v1.0.0\n",
            ),
            (
                "left.go",
                "package left\n\nimport \"lisette.test/leaf\"\n\nfunc Name() string { return leaf.Name() }\n",
            ),
        ],
    ),
    (
        "lisette.test/right",
        &[
            (
                "go.mod",
                "module lisette.test/right\n\ngo 1.21\n\nrequire lisette.test/leaf v1.0.0\n",
            ),
            (
                "right.go",
                "package right\n\nimport \"lisette.test/leaf\"\n\nfunc Name() string { return leaf.Name() }\n",
            ),
        ],
    ),
    (
        "lisette.test/leaf",
        &[
            ("go.mod", "module lisette.test/leaf\n\ngo 1.21\n"),
            (
                "leaf.go",
                "package leaf\n\nfunc Name() string { return \"leaf\" }\n",
            ),
        ],
    ),
    (
        "lisette.test/drift",
        &[
            (
                "go.mod",
                "module lisette.test/drift\n\ngo 1.21\n\nrequire (\n\tlisette.test/shared v1.0.0\n\tlisette.test/tagged v1.0.0\n)\n",
            ),
            (
                "drift.go",
                "package drift\n\nimport \"lisette.test/shared\"\n\nfunc Hello() string { return shared.Name() }\n",
            ),
            (
                "drift_purego.go",
                "//go:build purego\n\npackage drift\n\nimport \"lisette.test/tagged\"\n\nfunc Tagged() string { return tagged.Name() }\n",
            ),
        ],
    ),
    (
        "lisette.test/tagged",
        &[
            (
                "go.mod",
                "module lisette.test/tagged\n\ngo 1.21\n\nrequire lisette.test/shared v1.1.0\n",
            ),
            (
                "tagged.go",
                "package tagged\n\nimport \"lisette.test/shared\"\n\nfunc Name() string { return shared.Name() }\n",
            ),
        ],
    ),
    (
        "lisette.test/shared",
        &[
            ("go.mod", "module lisette.test/shared\n\ngo 1.21\n"),
            (
                "shared.go",
                "package shared\n\nfunc Name() string { return \"shared\" }\n",
            ),
        ],
    ),
    (
        "lisette.test/child",
        &[
            ("go.mod", "module lisette.test/child\n\ngo 1.21\n"),
            (
                "child.go",
                "package child\n\nfunc Name() string { return \"child\" }\n",
            ),
        ],
    ),
    (
        "lisette.test/taggedbroken",
        &[
            (
                "go.mod",
                "module lisette.test/taggedbroken\n\ngo 1.21\n\nrequire lisette.test/broken v1.0.0\n",
            ),
            (
                "taggedbroken.go",
                "package taggedbroken\n\nfunc Hello() string { return \"hi\" }\n",
            ),
            (
                "taggedbroken_purego.go",
                "//go:build purego\n\npackage taggedbroken\n\nimport _ \"lisette.test/broken\"\n",
            ),
        ],
    ),
    (
        "lisette.test/broken",
        &[
            ("go.mod", "module lisette.test/broken\n\ngo 1.21\n"),
            ("broken.go", "package broken\n\nimport \"unterminated\n"),
        ],
    ),
    (
        "lisette.test/taggedstdtypo",
        &[
            (
                "go.mod",
                "module lisette.test/taggedstdtypo\n\ngo 1.21\n\nrequire lisette.test/stdtypo v1.0.0\n",
            ),
            (
                "taggedstdtypo.go",
                "package taggedstdtypo\n\nfunc Hello() string { return \"hi\" }\n",
            ),
            (
                "taggedstdtypo_purego.go",
                "//go:build purego\n\npackage taggedstdtypo\n\nimport _ \"lisette.test/stdtypo\"\n",
            ),
        ],
    ),
    (
        "lisette.test/stdtypo",
        &[
            ("go.mod", "module lisette.test/stdtypo\n\ngo 1.21\n"),
            (
                "stdtypo.go",
                "package stdtypo\n\nimport _ \"encoding/doesnotexist\"\n",
            ),
        ],
    ),
    (
        "lisette.test/windowsstdtypo",
        &[
            ("go.mod", "module lisette.test/windowsstdtypo\n\ngo 1.21\n"),
            (
                "windowsstdtypo.go",
                "package windowsstdtypo\n\nfunc Hello() string { return \"hi\" }\n",
            ),
            (
                "windowsstdtypo_windows.go",
                "package windowsstdtypo\n\nimport _ \"encoding/doesnotexist\"\n",
            ),
        ],
    ),
];

/// `lisette.test/shared` v1.1.0 imports `lisette.test/child`, which v1.0.0 does not.
const SHARED_UPGRADE: (&str, &str, &[(&str, &str)]) = (
    "lisette.test/shared",
    "v1.1.0",
    &[
        (
            "go.mod",
            "module lisette.test/shared\n\ngo 1.21\n\nrequire lisette.test/child v1.0.0\n",
        ),
        (
            "shared.go",
            "package shared\n\nimport \"lisette.test/child\"\n\nfunc Name() string { return child.Name() }\n",
        ),
    ],
);

const VERSION: &str = "v1.0.0";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn go_env() -> &'static (PathBuf, PathBuf, PathBuf) {
    static ENV: OnceLock<(PathBuf, PathBuf, PathBuf)> = OnceLock::new();
    ENV.get_or_init(|| {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli_go_dependencies");
        let proxy = base.join("proxy");
        let modcache = base.join("modcache");
        let go_env_file = base.join("go.env");

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
            write_proxy_module(&proxy, module, VERSION, files);
        }
        let (module, version, files) = SHARED_UPGRADE;
        write_proxy_module(&proxy, module, version, files);
        fs::write(&go_env_file, "GOFLAGS=-modcacherw\n").expect("failed to write go.env");
        (proxy, modcache, go_env_file)
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

fn write_proxy_module(proxy: &Path, module: &str, version: &str, files: &[(&str, &str)]) {
    let dir = proxy.join(module).join("@v");
    fs::create_dir_all(&dir).unwrap();
    let list = fs::read_to_string(dir.join("list")).unwrap_or_default();
    fs::write(dir.join("list"), format!("{list}{version}\n")).unwrap();
    fs::write(
        dir.join(format!("{version}.info")),
        format!("{{\"Version\":\"{version}\",\"Time\":\"2026-01-01T00:00:00Z\"}}"),
    )
    .unwrap();
    let go_mod = files.iter().find(|(name, _)| *name == "go.mod").unwrap().1;
    fs::write(dir.join(format!("{version}.mod")), go_mod).unwrap();

    let entries: Vec<(String, &[u8])> = files
        .iter()
        .map(|(name, content)| (format!("{module}@{version}/{name}"), content.as_bytes()))
        .collect();
    fs::write(dir.join(format!("{version}.zip")), stored_zip(&entries)).unwrap();
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
    let (proxy, modcache, go_env_file) = go_env();
    let manifest = repo().join("Cargo.toml");
    let output = Command::new("cargo")
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
        .env("GOENV", go_env_file)
        .output()
        .expect("failed to invoke lisette");
    make_writable(&modcache.join("lisette.test"));
    output
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

const PLATFORM_MAIN: &str = "import \"go:fmt\"\nimport \"go:lisette.test/platform\"\n\nfn main() {\n  fmt.Println(platform.Hello())\n}\n";

#[test]
fn add_declares_modules_that_other_targets_and_tags_import() {
    if !go_available() {
        return;
    }
    let (_dir, project) = scaffold("", PLATFORM_MAIN);

    let added = lis(&project, &["add", "lisette.test/platform"]);
    let output = combined(&added);
    assert!(added.status.success(), "{output}");
    assert!(!output.contains("lisette.test/ignored"), "{output}");
    assert_eq!(
        go_dependencies(&project),
        "[dependencies.go]
\"lisette.test/platform\" = \"v1.0.0\"
\"lisette.test/subpackageonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
\"lisette.test/tagonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
\"lisette.test/windowsonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
"
    );

    let windows = lis(&project, &["check", "--target", "windows/amd64"]);
    assert!(windows.status.success(), "{}", combined(&windows));

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hi\n");
}

#[test]
fn sync_declares_modules_that_other_targets_and_tags_import() {
    if !go_available() {
        return;
    }
    let (_dir, project) = scaffold(
        "\n[dependencies.go]\n\"lisette.test/platform\" = \"v1.0.0\"\n",
        PLATFORM_MAIN,
    );

    let synced = lis(&project, &["sync"]);
    let output = combined(&synced);
    assert!(synced.status.success(), "{output}");
    assert!(!output.contains("lisette.test/ignored"), "{output}");
    assert_eq!(
        go_dependencies(&project),
        "[dependencies.go]
\"lisette.test/platform\" = \"v1.0.0\"
\"lisette.test/tagonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
\"lisette.test/windowsonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
"
    );

    let windows = lis(&project, &["check", "--target", "windows/amd64"]);
    assert!(windows.status.success(), "{}", combined(&windows));
}

#[test]
fn add_names_a_local_module_that_only_a_windows_file_imports() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/locala\"\n\nfn main() {\n  fmt.Println(locala.Hello())\n}\n";
    let (dir, project) = scaffold("", main);
    let files: &[(&str, &str)] = &[
        ("b/go.mod", "module lisette.test/localb\n\ngo 1.21\n"),
        ("b/b.go", "package localb\n\ntype Value struct{ N int }\n"),
        (
            "a/go.mod",
            "module lisette.test/locala\n\ngo 1.21\n\nrequire lisette.test/localb v0.0.0\n\nreplace lisette.test/localb => ../b\n",
        ),
        (
            "a/a.go",
            "package locala\n\nfunc Hello() string { return \"hi\" }\n",
        ),
        (
            "a/a_windows.go",
            "package locala\n\nimport \"lisette.test/localb\"\n\nfunc Handle() localb.Value { return localb.Value{} }\n",
        ),
    ];
    for (name, content) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    let rejected = lis(&project, &["add", "--path", "../a"]);
    let output = combined(&rejected);
    assert!(!rejected.status.success(), "{output}");
    assert!(
        output.contains("`lisette.test/locala` needs `lisette.test/localb`"),
        "{output}"
    );

    for path in ["../b", "../a"] {
        let added = lis(&project, &["add", "--path", path]);
        assert!(added.status.success(), "{}", combined(&added));
    }

    let windows = lis(&project, &["check", "--target", "windows/amd64"]);
    assert!(windows.status.success(), "{}", combined(&windows));

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hi\n");
}

#[test]
fn sync_adds_a_parent_that_only_another_target_reaches() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/diamond\"\n\nfn main() {\n  fmt.Println(diamond.Hello())\n}\n";
    let (_dir, project) = scaffold(
        "\n[dependencies.go]
\"lisette.test/diamond\" = \"v1.0.0\"
\"lisette.test/leaf\" = { version = \"v1.0.0\", via = [\"lisette.test/left\"] }
\"lisette.test/left\" = { version = \"v1.0.0\", via = [\"lisette.test/diamond\"] }
",
        main,
    );

    let synced = lis(&project, &["sync"]);
    assert!(synced.status.success(), "{}", combined(&synced));
    assert_eq!(
        go_dependencies(&project),
        "[dependencies.go]
\"lisette.test/diamond\" = \"v1.0.0\"
\"lisette.test/leaf\" = { version = \"v1.0.0\", via = [\"lisette.test/left\", \"lisette.test/right\"] }
\"lisette.test/left\" = { version = \"v1.0.0\", via = [\"lisette.test/diamond\"] }
\"lisette.test/right\" = { version = \"v1.0.0\", via = [\"lisette.test/diamond\"] }
"
    );
}

#[test]
fn sync_records_a_version_that_a_tagged_import_raises() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/drift\"\n\nfn main() {\n  fmt.Println(drift.Hello())\n}\n";
    let (_dir, project) = scaffold(
        "\n[dependencies.go]\n\"lisette.test/drift\" = \"v1.0.0\"\n",
        main,
    );

    let synced = lis(&project, &["sync"]);
    assert!(synced.status.success(), "{}", combined(&synced));
    assert_eq!(
        go_dependencies(&project),
        "[dependencies.go]
\"lisette.test/drift\" = \"v1.0.0\"
\"lisette.test/child\" = { version = \"v1.0.0\", via = [\"lisette.test/shared\"] }
\"lisette.test/shared\" = { version = \"v1.1.0\", via = [\"lisette.test/drift\", \"lisette.test/tagged\"] }
\"lisette.test/tagged\" = { version = \"v1.0.0\", via = [\"lisette.test/drift\"] }
"
    );

    let run = lis(&project, &["run"]);
    assert!(run.status.success(), "{}", combined(&run));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "child\n");
}

#[test]
fn sync_follows_blank_imports() {
    if !go_available() {
        return;
    }
    let main = "import _ \"go:lisette.test/platform\"\n\nfn main() {}\n";
    let (_dir, project) = scaffold(
        "\n[dependencies.go]\n\"lisette.test/platform\" = \"v1.0.0\"\n",
        main,
    );

    let synced = lis(&project, &["sync"]);
    assert!(synced.status.success(), "{}", combined(&synced));
    assert_eq!(
        go_dependencies(&project),
        "[dependencies.go]
\"lisette.test/platform\" = \"v1.0.0\"
\"lisette.test/tagonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
\"lisette.test/windowsonly\" = { version = \"v1.0.0\", via = [\"lisette.test/platform\"] }
"
    );
}

#[test]
fn add_reports_an_error_in_a_package_that_only_a_tag_reaches() {
    if !go_available() {
        return;
    }
    let main = "import \"go:fmt\"\nimport \"go:lisette.test/taggedbroken\"\n\nfn main() {\n  fmt.Println(taggedbroken.Hello())\n}\n";
    let (_dir, project) = scaffold("", main);

    let added = lis(&project, &["add", "lisette.test/taggedbroken"]);
    let output = combined(&added);
    assert!(!added.status.success(), "{output}");
    assert!(output.contains("lisette.test/broken"), "{output}");
}

#[test]
fn add_reports_a_missing_standard_package_that_only_another_target_imports() {
    if !go_available() {
        return;
    }
    for module in ["lisette.test/taggedstdtypo", "lisette.test/windowsstdtypo"] {
        let (_dir, project) = scaffold("", MAIN);

        let added = lis(&project, &["add", module]);
        let output = combined(&added);
        assert!(!added.status.success(), "{module}: {output}");
        assert!(
            output.contains("encoding/doesnotexist"),
            "{module}: {output}"
        );
    }
}
