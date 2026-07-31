use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{Edit, Fix, LisetteDiagnostic};
use semantics::store::Store;
use syntax::ENTRY_MODULE_ID;
use syntax::ast::{Expression, ImportAlias, Span};
use syntax::program::{File, Module, unaliased_binding_name};

pub(super) fn check_redundant_aliases(
    files: &HashMap<u32, File>,
    store: &Store,
    unused_import_spans: &HashSet<Span>,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    for file in files.values().filter(|file| !file.is_d_lis()) {
        for item in &file.items {
            let Expression::ModuleImport {
                name,
                name_span,
                alias: Some(ImportAlias::Named(alias, alias_span)),
                ..
            } = item
            else {
                continue;
            };

            if unused_import_spans.contains(name_span) {
                continue;
            }

            // Rewritten from `import "root"`, so this path is not what the file says.
            if name == ENTRY_MODULE_ID {
                continue;
            }

            // Without typedefs, the package clause that names the binding is unknown.
            if name.starts_with("go:") && store.get_module(name).is_none_or(Module::is_empty_stub) {
                continue;
            }

            if unaliased_binding_name(name, &store.go_package_names) != alias.as_str() {
                continue;
            }

            let deletion = Span::new(
                alias_span.file_id,
                alias_span.byte_offset,
                name_span.byte_offset - alias_span.byte_offset,
            );
            diagnostics.push(
                diagnostics::lint::redundant_import_alias(alias_span, name).with_fix(Fix::new(
                    "Remove the redundant alias",
                    Edit::deletion(deletion),
                )),
            );
        }
    }
}
