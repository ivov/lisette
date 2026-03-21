use std::fs;
use std::path::Path;
use std::process::Command;

include!(concat!(env!("OUT_DIR"), "/go_version.rs"));

use emit::PRELUDE_IMPORT_PATH;

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

pub fn go_fmt(path: &Path) -> Result<(), String> {
    let output = Command::new("gofmt")
        .arg("-w")
        .arg(path)
        .output()
        .map_err(|e| format!("Failed to run `gofmt`: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`gofmt` error: {}", stderr));
    }

    Ok(())
}

pub fn write_go_mod(dir: &Path, module_name: &str) -> Result<(), String> {
    let prelude_version = env!("CARGO_PKG_VERSION");
    let mut content = format!(
        "module {}\n\ngo {}\n\nrequire {} v{}\n",
        module_name,
        go_mod_version(),
        PRELUDE_IMPORT_PATH,
        prelude_version,
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

    // Skip write if content unchanged; invalidate go.sum if content changed
    let content_changed = fs::read_to_string(&go_mod_path)
        .map(|existing| existing != content)
        .unwrap_or(true);

    if content_changed {
        fs::write(&go_mod_path, &content).map_err(|e| format!("Failed to write go.mod: {}", e))?;
        let _ = fs::remove_file(dir.join("go.sum"));
    }

    let _ = fs::remove_dir_all(dir.join("lisette"));

    Ok(())
}

pub fn ensure_go_sum(dir: &Path) -> Result<(), String> {
    let go_sum_path = dir.join("go.sum");
    if go_sum_path.exists() {
        return Ok(());
    }
    go_mod_download(dir)
}

pub fn prewarm_module_cache() {
    let prelude_version = env!("CARGO_PKG_VERSION");
    let _ = Command::new("go")
        .args([
            "mod",
            "download",
            &format!("{}@v{}", PRELUDE_IMPORT_PATH, prelude_version),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn go_mod_download(path: &Path) -> Result<(), String> {
    let output = Command::new("go")
        .args(["mod", "download"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run `go mod download`: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`go mod download` error: {}", stderr));
    }

    Ok(())
}
