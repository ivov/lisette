use std::path::Path;
use std::process::Command;

use crate::cli_error;

pub fn bindgen(
    target_pkg: &str,
    output: Option<String>,
    version: Option<String>,
    verbose: bool,
) -> i32 {
    if let Err(code) = crate::go_cli::require_go() {
        return code;
    }

    if target_pkg == "stdlib" {
        let source_dir = Path::new("bindgen");
        if !source_dir.exists() {
            cli_error!(
                "Failed to generate std bindings",
                "Bindgen source not found at `bindgen`",
                "Run this command from the Lisette project root"
            );
            return 1;
        }
        return bindgen_std(source_dir, version, verbose);
    }

    bindgen_pkg(target_pkg, output, verbose)
}

fn bindgen_pkg(target_pkg: &str, output: Option<String>, verbose: bool) -> i32 {
    let output_path = match output {
        Some(path) => path,
        None => {
            let filename = target_pkg.replace('/', "_");
            format!("{}.d.lis", filename)
        }
    };

    if verbose {
        eprintln!("Generating bindings for {} -> {}", target_pkg, output_path);
    }

    // lis bindgen writes to a user-specified path, not the typedef cache
    let workspace = crate::workspace::GoWorkspace::new(Path::new("."), Path::new(""));

    match workspace.run_bindgen(target_pkg) {
        Ok(content) => {
            if let Err(e) = std::fs::write(&output_path, &content) {
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
        Err(msg) => {
            cli_error!(
                "Failed to generate bindings",
                msg,
                "Check Go installation with `go version`"
            );
            1
        }
    }
}

fn bindgen_std(source_dir: &Path, version: Option<String>, verbose: bool) -> i32 {
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
        .join("bindgen/bindgen.stdlib.json")
        .to_string_lossy()
        .to_string();

    let mut args = vec![
        "run".to_string(),
        ".".to_string(),
        "stdlib".to_string(),
        "--config".to_string(),
        config_path,
        "--outdir".to_string(),
        absolute_out_dir,
    ];
    if let Some(ver) = version {
        args.push("--version".to_string());
        args.push(ver);
    }

    let status = Command::new("go")
        .args(&args)
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
                "Check the Go tool builds with `cd bindgen && just build`"
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
