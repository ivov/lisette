use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli_error;

const BINDGEN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the path to the bindgen binary for user-facing `lis bindgen <pkg>`.
///
/// Resolution order:
/// 1. `tools/bindgen/bin/bindgen` — dev builds (present in source tree)
/// 2. `~/.lisette/bin/bindgen` — user installs (built from embedded source)
fn resolve_bindgen_binary() -> Option<PathBuf> {
    let dev_path = Path::new("tools/bindgen/bin/bindgen");
    if dev_path.exists() {
        return Some(dev_path.to_path_buf());
    }

    let home = std::env::var("HOME").ok()?;
    let cache_dir = PathBuf::from(&home).join(".lisette").join("bin");
    let bin_path = cache_dir.join("bindgen");
    let version_path = cache_dir.join("bindgen.version");

    if bin_path.exists()
        && let Ok(cached_version) = std::fs::read_to_string(&version_path)
        && cached_version.trim() == BINDGEN_VERSION
    {
        return Some(bin_path);
    }

    let source_dir = Path::new("tools/bindgen");
    if source_dir.join("go.mod").exists() {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            eprintln!("warning: failed to create cache dir: {}", e);
            return None;
        }

        eprintln!("Building bindgen...");

        let status = Command::new("go")
            .args(["build", "-o", &bin_path.to_string_lossy(), "./cmd/bindgen"])
            .current_dir(source_dir)
            .status();

        match status {
            Ok(s) if s.success() => {
                let _ = std::fs::write(&version_path, BINDGEN_VERSION);
                return Some(bin_path);
            }
            Ok(_) => {
                eprintln!("warning: failed to build bindgen");
                return None;
            }
            Err(e) => {
                eprintln!("warning: failed to run `go build`: {}", e);
                return None;
            }
        }
    }

    None
}

pub fn bindgen(package: &str, output: Option<String>, _force: bool, verbose: bool) -> i32 {
    if let Err(code) = crate::go_cli::require_go() {
        return code;
    }

    if package == "stdlib" {
        let source_dir = Path::new("tools/bindgen");
        if !source_dir.exists() {
            cli_error!(
                "Failed to generate std bindings",
                "Bindgen source not found at `tools/bindgen`",
                "Run this command from the Lisette project root"
            );
            return 1;
        }
        return bindgen_std(source_dir, verbose);
    }

    let bin_path = match resolve_bindgen_binary() {
        Some(path) => path,
        None => {
            cli_error!(
                "Failed to generate bindings",
                "Bindgen binary not found",
                "Check Go installation with `go version`"
            );
            return 1;
        }
    };

    bindgen_pkg(&bin_path, package, output, verbose)
}

fn bindgen_pkg(bin_path: &Path, package: &str, output: Option<String>, verbose: bool) -> i32 {
    let output_path = match output {
        Some(path) => path,
        None => {
            let filename = package.replace('/', "_");
            format!("{}.d.lis", filename)
        }
    };

    if verbose {
        eprintln!("Generating bindings for {} -> {}", package, output_path);
    }

    let result = Command::new(bin_path).args(["pkg", package]).output();

    match result {
        Ok(output) if output.status.success() => {
            if let Err(e) = std::fs::write(&output_path, &output.stdout) {
                cli_error!(
                    "Failed to write bindings",
                    format!("Could not write to {}: {}", output_path, e),
                    "Check file permissions"
                );
                return 1;
            }
            eprintln!();
            eprintln!("  ✓ Generated bindings: {}", output_path);
            0
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            cli_error!(
                "Failed to generate bindings",
                format!("Bindgen exited with code {:?}", output.status.code()),
                stderr.trim().to_string()
            );
            1
        }
        Err(e) => {
            cli_error!(
                "Failed to run bindgen",
                e.to_string(),
                "Check Go installation with `go version`"
            );
            1
        }
    }
}

fn bindgen_std(source_dir: &Path, verbose: bool) -> i32 {
    let out_dir = "crates/stdlib/typedefs";

    if verbose {
        eprintln!("Generating stdlib bindings to {}", out_dir);
    }

    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            cli_error!(
                "Failed to generate bindings",
                format!("Could not determine working directory: {}", e),
                "Check file permissions"
            );
            return 1;
        }
    };

    let absolute_out_dir = cwd.join(out_dir).to_string_lossy().to_string();
    let config_path = cwd
        .join("tools/bindgen/bindgen.stdlib.json")
        .to_string_lossy()
        .to_string();

    let status = Command::new("go")
        .args([
            "run",
            "./cmd/bindgen",
            "stdlib",
            "--config",
            &config_path,
            "--outdir",
            &absolute_out_dir,
        ])
        .current_dir(source_dir)
        .status();

    match status {
        Ok(status) if status.success() => {
            eprintln!();
            eprintln!("  ✓ Generated std bindings: {}", out_dir);
            0
        }
        Ok(status) => {
            cli_error!(
                "Failed to generate std bindings",
                format!("Bindgen exited with code {:?}", status.code()),
                "Check the Go tool builds with `cd tools/bindgen && just build`"
            );
            1
        }
        Err(e) => {
            cli_error!(
                "Failed to generate std bindings",
                format!("Failed to run bindgen: {}", e),
                "Check Go installation with `go version`"
            );
            1
        }
    }
}
