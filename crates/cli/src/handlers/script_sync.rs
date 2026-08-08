//! `lis sync --script`.

use std::path::Path;

use super::script_edit::{
    read, refuse_project_file, save_unchanged, script_dir, third_party_imports,
};
use super::script_table::ScriptTable;
use crate::cli_error;
use crate::output::print_sync_summary;

const SUBJECT: &str = "`[dependencies.go]`";

pub(super) fn run(file: &Path) -> i32 {
    let heading = "Failed to sync script";
    if let Err(code) = refuse_project_file(file, heading) {
        return code;
    }
    let Some(source) = read(file, heading) else {
        return 1;
    };
    let mut table = match ScriptTable::read(&source) {
        Ok(table) => table,
        Err(message) => {
            cli_error!(heading, message, "Fix the table and retry");
            return 1;
        }
    };

    let dir = match script_dir(file, heading) {
        Ok(dir) => dir,
        Err(code) => return code,
    };
    let _mutation = match crate::lock::acquire_mutation_lock(&dir, "script") {
        Ok(lock) => lock,
        Err(code) => return code,
    };

    let imported = third_party_imports(&source);
    let changes = match deps::finalize_via(&mut table.document, &imported) {
        Ok(changes) => changes,
        Err(message) => {
            cli_error!(heading, message, "Fix the table and retry");
            return 1;
        }
    };

    if !changes.is_empty()
        && let Err(message) = save_unchanged(&table, file, &source)
    {
        cli_error!(heading, message, "Retry once the file has settled");
        return 1;
    }

    print_sync_summary(
        SUBJECT,
        &changes.trimmed,
        &changes.promoted,
        &changes.removed,
        true,
    );
    0
}
