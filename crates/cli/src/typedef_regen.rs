use deps::{BindgenFailure, TypedefLocator, TypedefLocatorResult};
use rustc_hash::FxHashSet as HashSet;

use crate::output::print_warning;
use crate::workspace::extract_go_imports;

/// Generate the typedef for every non-blank `go:` import in source, then
/// recurse into each typedef's own `go:` imports. Returns `Err(1)` if any
/// declared import fails; undeclared and unknown-stdlib imports are left
/// to the type-checker.
pub fn prewarm_typedef_cache(
    source_imports: &[String],
    locator: &TypedefLocator,
) -> Result<(), i32> {
    let mut visited: HashSet<String> = HashSet::default();
    let mut queue: Vec<String> = Vec::with_capacity(source_imports.len());
    for pkg in source_imports {
        if visited.insert(pkg.clone()) {
            queue.push(pkg.clone());
        }
    }

    let mut had_failure = false;

    while let Some(pkg) = queue.pop() {
        match locator.find_typedef_content(&pkg) {
            TypedefLocatorResult::Found { content, .. } => {
                for imp in extract_go_imports(&content) {
                    if visited.insert(imp.clone()) {
                        queue.push(imp);
                    }
                }
            }
            TypedefLocatorResult::UnknownStdlib | TypedefLocatorResult::UndeclaredImport => {
                // Type-checker handles these.
            }
            TypedefLocatorResult::MissingTypedef { module, version } => {
                print_warning(&format!(
                    "missing typedef for {}",
                    pkg_label(&pkg, &module, &version)
                ));
                had_failure = true;
            }
            TypedefLocatorResult::UnreadableTypedef { path, error } => {
                print_warning(&format!(
                    "unreadable typedef at `{}`: {}",
                    path.display(),
                    error
                ));
                had_failure = true;
            }
            TypedefLocatorResult::BindgenFailed {
                kind,
                module,
                version,
                ..
            } => {
                match kind {
                    BindgenFailure::GoToolchainMissing => {
                        print_warning(&format!(
                            "cannot bindgen {}: Go toolchain not installed",
                            pkg_label(&pkg, &module, &version)
                        ));
                    }
                    BindgenFailure::InvocationFailed { stderr } => {
                        print_warning(&format!(
                            "bindgen failed for {}: {}",
                            pkg_label(&pkg, &module, &version),
                            stderr.trim()
                        ));
                    }
                }
                had_failure = true;
            }
        }
    }

    if had_failure { Err(1) } else { Ok(()) }
}

fn pkg_label(pkg: &str, module: &str, version: &str) -> String {
    format!("`{}` ({} {})", pkg, module, version)
}
