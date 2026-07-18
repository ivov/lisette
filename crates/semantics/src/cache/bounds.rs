use diagnostics::LocalSink;
use rustc_hash::FxHashSet;
use syntax::ast::{Expression, Generic};
use syntax::program::{FileImport, Visibility};
use syntax::types::Symbol;

use crate::checker::{FileContextKind, TaskState};
use crate::prelude::PRELUDE_MODULE_ID;
use crate::store::Store;

struct CachedFileBounds {
    file_id: u32,
    items: Vec<Expression>,
    missing_types: Vec<Expression>,
    imports: Vec<FileImport>,
}

pub(crate) fn restore_cached_generic_bounds(
    store: &mut Store,
    sink: &LocalSink,
    cached_modules: &FxHashSet<String>,
) {
    let mut checker = TaskState::with_fresh_allocator(sink);
    let module_ids = store.module_ids.clone();
    for module_id in module_ids {
        restore_module_bounds(
            &mut checker,
            store,
            &module_id,
            cached_modules.contains(&module_id),
        );
    }
}

fn restore_module_bounds(
    checker: &mut TaskState,
    store: &mut Store,
    module_id: &str,
    scan_missing_types: bool,
) {
    let Some(module) = store.get_module(module_id) else {
        return;
    };
    let pending: FxHashSet<Symbol> = module
        .definitions
        .iter()
        .filter_map(|(name, definition)| {
            let generics = definition.body.generics()?;
            if generics.iter().any(needs_restoration) {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();
    if pending.is_empty() && !scan_missing_types {
        return;
    }
    let cached_names: FxHashSet<Symbol> = module.definitions.keys().cloned().collect();
    let sources: Vec<(u32, String)> = module
        .files
        .values()
        .chain(module.typedefs.values())
        .filter(|file| file.items.is_empty() && !file.is_test())
        .map(|file| (file.id, file.source.clone()))
        .collect();
    let files: Vec<CachedFileBounds> = sources
        .into_iter()
        .map(|(file_id, source)| {
            let items = syntax::build_ast(&source, file_id).ast;
            let imports = cached_imports(&items);
            let missing_types = items
                .iter()
                .filter(|item| {
                    type_name(item).is_some_and(|name| {
                        !cached_names.contains(&Symbol::from_parts(module_id, name))
                    })
                })
                .cloned()
                .collect();
            CachedFileBounds {
                file_id,
                items,
                missing_types,
                imports,
            }
        })
        .collect();
    if pending.is_empty() && files.iter().all(|file| file.missing_types.is_empty()) {
        return;
    }

    for file in &files {
        if file.missing_types.is_empty() {
            continue;
        }
        checker.with_file_context_mut(
            store,
            module_id,
            file.file_id,
            &file.imports,
            file_context_kind(module_id),
            |checker, store| {
                checker.register_type_names(store, &file.missing_types, &Visibility::Private);
            },
        );
    }

    for file in &files {
        if file.missing_types.is_empty() {
            continue;
        }
        checker.with_file_context_mut(
            store,
            module_id,
            file.file_id,
            &file.imports,
            file_context_kind(module_id),
            |checker, store| checker.register_type_definitions(store, &file.missing_types),
        );
    }

    for file in files {
        let definitions: Vec<(Symbol, Vec<Generic>)> = file
            .items
            .iter()
            .filter_map(|item| {
                let (name, generics) = type_generics(item)?;
                let name = Symbol::from_parts(module_id, name);
                pending.contains(&name).then(|| (name, generics.to_vec()))
            })
            .collect();
        if definitions.is_empty() {
            continue;
        }
        checker.with_file_context_mut(
            store,
            module_id,
            file.file_id,
            &file.imports,
            file_context_kind(module_id),
            |checker, store| {
                for (name, generics) in definitions {
                    let Some(span) = generics.first().map(|generic| generic.span) else {
                        continue;
                    };
                    checker.scopes.push();
                    checker.put_in_scope(&generics);
                    let resolved = checker.resolve_generic_bounds(&*store, &generics, &span);
                    checker.scopes.pop();

                    let Some(definition) = store
                        .get_module_mut(module_id)
                        .and_then(|module| module.definitions.get_mut(&name))
                    else {
                        continue;
                    };
                    if let Some(target) = definition.body.generics_mut() {
                        target.clone_from_slice(&resolved);
                    }
                }
            },
        );
    }
}

fn cached_imports(items: &[Expression]) -> Vec<FileImport> {
    items
        .iter()
        .filter_map(|item| {
            let Expression::ModuleImport {
                name,
                name_span,
                alias,
                span,
            } = item
            else {
                return None;
            };
            Some(FileImport {
                name: name.clone(),
                name_span: *name_span,
                alias: alias.clone(),
                span: *span,
            })
        })
        .collect()
}

fn type_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Enum { name, .. }
        | Expression::Struct { name, .. }
        | Expression::Interface { name, .. }
        | Expression::TypeAlias { name, .. } => Some(name),
        _ => None,
    }
}

fn type_generics(expression: &Expression) -> Option<(&str, &[Generic])> {
    match expression {
        Expression::Enum { name, generics, .. }
        | Expression::Struct { name, generics, .. }
        | Expression::Interface { name, generics, .. }
        | Expression::TypeAlias { name, generics, .. } => Some((name, generics)),
        _ => None,
    }
}

fn file_context_kind(module_id: &str) -> FileContextKind {
    if module_id == PRELUDE_MODULE_ID {
        FileContextKind::Prelude
    } else if module_id.starts_with("go:") {
        FileContextKind::ImportedTypedef
    } else {
        FileContextKind::Standard
    }
}

fn needs_restoration(generic: &Generic) -> bool {
    generic.bounds.len() != generic.resolved_bounds.len()
}
