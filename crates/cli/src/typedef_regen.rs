use deps::{BindgenFailure, TypedefLocator, TypedefLocatorResult};
use rustc_hash::FxHashSet;

use crate::output::print_warning;
use crate::workspace::extract_go_imports;

pub struct PrewarmOutcome {
    /// `Err(1)` if any declared import fails.
    pub result: Result<(), i32>,
    /// Packages that typedefs import but no declared module contains.
    pub missing_transitive: Vec<String>,
}

/// Generate the typedef for every non-blank `go:` import in source, then
/// recurse into each typedef's own `go:` imports. Undeclared and
/// unknown-stdlib imports are left to the type-checker.
pub fn prewarm_typedef_cache(
    source_imports: &[String],
    locator: &TypedefLocator,
) -> PrewarmOutcome {
    let mut visited: FxHashSet<String> = FxHashSet::default();
    let mut queue: Vec<String> = Vec::with_capacity(source_imports.len());
    for pkg in source_imports {
        if visited.insert(pkg.clone()) {
            queue.push(pkg.clone());
        }
    }

    let mut had_failure = false;
    let mut undeclared: FxHashSet<String> = FxHashSet::default();
    let mut typedef_imports: FxHashSet<String> = FxHashSet::default();

    while let Some(pkg) = queue.pop() {
        match locator.find_typedef_content(&pkg) {
            TypedefLocatorResult::Found { content, .. } => {
                for imp in extract_go_imports(&content) {
                    typedef_imports.insert(imp.clone());
                    if visited.insert(imp.clone()) {
                        queue.push(imp);
                    }
                }
            }
            TypedefLocatorResult::UndeclaredImport => {
                undeclared.insert(pkg);
            }
            TypedefLocatorResult::UnknownStdlib | TypedefLocatorResult::InternalPackage { .. } => {
                // Type-checker handles these.
            }
            TypedefLocatorResult::MissingTypedef {
                module, version, ..
            } => {
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
                    BindgenFailure::InvocationFailed { stderr, hint } => {
                        print_warning(&format!(
                            "bindgen failed for {}: {}",
                            pkg_label(&pkg, &module, &version),
                            stderr.trim()
                        ));
                        if let Some(hint) = hint {
                            print_warning(&hint);
                        }
                    }
                }
                had_failure = true;
            }
        }
    }

    let mut missing_transitive: Vec<String> = undeclared
        .into_iter()
        .filter(|pkg| typedef_imports.contains(pkg))
        .collect();
    missing_transitive.sort();

    PrewarmOutcome {
        result: if had_failure { Err(1) } else { Ok(()) },
        missing_transitive,
    }
}

fn pkg_label(pkg: &str, module: &str, version: &str) -> String {
    format!("`{}` ({} {})", pkg, module, version)
}
