use std::fs;
use std::fs::File;
use std::path::Path;

use fs2::FileExt;

use crate::go_cli;
use crate::output::{print_progress, print_warning};
use crate::workspace::GoWorkspace;
use crate::{cli_error, error};
use deps::{GoModule, Manifest, TypedefLocator};

/// Generate any Go typedefs declared in the manifest but missing from the cache:
/// `~/.lisette/cache/typedefs/lis@v{version}/{module}@{version}/*.d.lis`
pub fn generate_missing_typedefs(project_root: &Path, manifest: &Manifest) -> Result<(), i32> {
    let go_deps = manifest.go_deps();
    if go_deps.is_empty() {
        return Ok(());
    }

    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => {
            error!(
                "failed to regenerate Go typedefs",
                "HOME environment variable not set".to_string()
            );
            return Err(1);
        }
    };
    let typedef_cache_dir = deps::typedef_cache_dir(&home);

    let missing_modules: Vec<(String, String)> = go_deps
        .iter()
        .filter(|(module_path, dep)| {
            !module_dir_populated(&typedef_cache_dir, module_path, &dep.version)
        })
        .map(|(module_path, dep)| (module_path.clone(), dep.version.clone()))
        .collect();

    if missing_modules.is_empty() {
        return Ok(());
    }

    go_cli::require_go()?;

    let project_target_dir = project_root.join("target");
    if project_target_dir.is_file() {
        cli_error!(
            "Failed to regenerate Go typedefs",
            "`target/` exists but is a file, not a directory",
            "Remove or move `target/` and retry"
        );
        return Err(1);
    }
    if let Err(e) = fs::create_dir_all(&project_target_dir) {
        error!(
            "failed to regenerate Go typedefs",
            format!("Failed to create target directory: {}", e)
        );
        return Err(1);
    }

    if let Err(e) = fs::create_dir_all(&typedef_cache_dir) {
        error!(
            "failed to regenerate Go typedefs",
            format!("Failed to create typedef cache directory: {}", e)
        );
        return Err(1);
    }

    let _lock = acquire_regen_lock(&typedef_cache_dir)?;

    // Another process may have regenerated everything while we were waiting.
    let still_missing: Vec<(String, String)> = missing_modules
        .into_iter()
        .filter(|(module_path, version)| {
            !module_dir_populated(&typedef_cache_dir, module_path, version)
        })
        .collect();

    if still_missing.is_empty() {
        return Ok(());
    }

    let locator = TypedefLocator::new(
        go_deps.clone(),
        Some(project_root.to_path_buf()),
        Some(home),
    );
    if let Err(msg) = go_cli::write_go_mod(&project_target_dir, &manifest.project.name, &locator) {
        error!("failed to write target/go.mod", msg);
        return Err(1);
    }

    let workspace = GoWorkspace::new(&project_target_dir, &typedef_cache_dir);

    let lis_version = env!("CARGO_PKG_VERSION");
    print_progress(&format!(
        "Regenerating Go typedefs for lis v{}",
        lis_version
    ));

    for (module_path, version) in &still_missing {
        print_progress(&format!("Regenerating {}@{}", module_path, version));

        let module = GoModule {
            path: module_path,
            version,
        };
        if let Err(msg) = workspace.reconcile(module) {
            print_warning(&format!("Failed to regenerate {}: {}", module_path, msg));
        }
    }

    Ok(())
}

fn module_dir_populated(cache_dir: &Path, module_path: &str, version: &str) -> bool {
    let module_dir = cache_dir.join(format!("{}@{}", module_path, version));
    match fs::read_dir(&module_dir) {
        Ok(mut entries) => entries.next().is_some(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => false,
    }
}

fn acquire_regen_lock(typedef_cache_dir: &Path) -> Result<File, i32> {
    let lock_path = typedef_cache_dir.join(".lis-regen.lock");
    let file = match File::create(&lock_path) {
        Ok(f) => f,
        Err(e) => {
            error!(
                "failed to create regen lock file",
                format!("Failed to create `{}`: {}", lock_path.display(), e)
            );
            return Err(1);
        }
    };

    if let Err(e) = file.lock_exclusive() {
        error!("failed to acquire regen lock", format!("{}", e));
        return Err(1);
    }

    Ok(file)
}
