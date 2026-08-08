use std::collections::HashMap;
use std::path::Path;

use deps::GoModule;
use stdlib::Target;

use super::add::ParsedDependency;
use super::reconciliation::{
    ResolvedDependency, RootWrite, apply_graph_to_document, reconcile_root,
};
use super::script_edit::{
    read, refuse_project_file, save_unchanged, script_dir, third_party_imports, write_go_mod,
};
use super::script_table::ScriptTable;
use crate::cli_error;
use crate::output::{print_add_success, print_preview_notice, print_progress};
use crate::workspace::GoWorkspace;

pub(super) fn run(file: &Path, dep_string: &str) -> i32 {
    let heading = "Failed to add dependency";
    if let Err(code) = refuse_project_file(file, heading) {
        return code;
    }
    let Some(source) = read(file, heading) else {
        return 1;
    };
    let ParsedDependency {
        requested_package,
        version,
    } = match super::add::parse_dep_string(dep_string) {
        Ok(parsed) => parsed,
        Err(message) => {
            cli_error!(
                heading,
                message,
                "Example: `lis add --script tool.lis google/uuid`"
            );
            return 1;
        }
    };

    let mut table = match ScriptTable::read(&source) {
        Ok(table) => table,
        Err(message) => {
            cli_error!(heading, message, "Fix the table and retry");
            return 1;
        }
    };

    print_preview_notice("Third-party Go dependencies", true);

    let dir = match script_dir(file, heading) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let _mutation = match crate::lock::acquire_mutation_lock(&dir, "script") {
        Ok(lock) => lock,
        Err(code) => return code,
    };
    let _target = match crate::lock::acquire_target_lock(&dir) {
        Ok(lock) => lock,
        Err(code) => return code,
    };
    if let Err(code) = write_go_mod(&dir, &table, heading) {
        return code;
    }
    let typedefs = deps::typedef_cache_dir(&dir);
    let workspace = GoWorkspace::new(&dir, &typedefs, Target::host());

    print_progress(&format!("Fetching {}@{}", requested_package, version));
    if let Err(message) = workspace.go_get(GoModule {
        path: &requested_package,
        version: &version,
        replacement: None,
    }) {
        cli_error!(heading, message, "Check the module path and the version");
        return 1;
    }

    let info = match workspace.find_containing_module(&requested_package) {
        Ok(info) if !info.path.is_empty() && !info.version.is_empty() => info,
        Ok(_) | Err(_) => {
            cli_error!(
                heading,
                format!(
                    "Could not resolve the module containing `{}`",
                    requested_package
                ),
                "Check the module path and the version"
            );
            return 1;
        }
    };

    let resolved = ResolvedDependency {
        requested_package,
        canonical_module: info.path,
    };
    let graph = match reconcile_root(&resolved, &workspace, &HashMap::new(), &[]) {
        Ok(graph) => graph,
        Err(code) => return code,
    };

    let upgraded = match apply_graph_to_document(
        &resolved.canonical_module,
        &mut table.document,
        &workspace,
        &graph,
        RootWrite::Remote {
            fallback_version: &info.version,
        },
    ) {
        Ok(upgraded) => upgraded,
        Err(code) => return code,
    };

    if let Err(message) = deps::finalize_via(&mut table.document, &third_party_imports(&source)) {
        cli_error!(heading, message, "Fix the table and retry");
        return 1;
    }

    if let Err(message) = save_unchanged(&table, file, &source) {
        cli_error!(heading, message, "Retry once the file has settled");
        return 1;
    }

    let version = graph
        .version(&resolved.canonical_module)
        .unwrap_or(&info.version)
        .to_string();
    let upgraded_tuples: Vec<(&str, &str, &str)> = upgraded
        .iter()
        .map(|u| {
            (
                u.path.as_str(),
                u.old_version.as_str(),
                u.new_version.as_str(),
            )
        })
        .collect();
    print_add_success(
        &resolved.canonical_module,
        &version,
        &graph.edges(),
        &graph.versions(),
        &upgraded_tuples,
        None,
    );
    0
}
