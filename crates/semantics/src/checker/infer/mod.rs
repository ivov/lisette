pub(crate) mod addressability;
pub(crate) mod expressions;
mod interface;
mod unify;
mod validation;

pub(crate) use unify::BuiltinBound;

use rustc_hash::FxHashMap as HashMap;

use super::freeze::FreezeFolder;
use super::{FileContextKind, TaskState};
use crate::store::Store;
use syntax::ast::Expression;
use syntax::program::{File, FileImport};

impl TaskState<'_> {
    pub fn infer_module(&mut self, store: &mut Store, module_id: &str) {
        let files = self.take_module_files(store, module_id);
        let items_per_file: Vec<&[Expression]> = files.iter().map(|f| f.items.as_slice()).collect();
        self.check_const_cycles(&*store, &items_per_file);

        for file in files {
            self.infer_file(store, module_id, file);
        }
    }

    fn take_module_files(&mut self, store: &mut Store, module_id: &str) -> Vec<File> {
        self.with_module_cursor(module_id, |_this| {
            let module = store
                .get_module_mut(module_id)
                .expect("module must exist for inference");
            std::mem::take(&mut module.files).into_values().collect()
        })
    }

    fn infer_file(&mut self, store: &mut Store, module_id: &str, file: File) {
        let file_id = file.id;
        let imports = file.imports();

        self.with_file_context(
            store,
            module_id,
            file_id,
            &imports,
            FileContextKind::Standard,
            |this, store| {
                this.check_definition_module_collisions(&*store, &file.items, &imports);

                let inferred_items: Vec<_> = file
                    .items
                    .into_iter()
                    .map(|item| {
                        let type_var = this.new_type_var();
                        this.infer_expression(store, item, &type_var)
                    })
                    .collect();

                this.check_reference_sibling_aliasing(&inferred_items);

                let folder = FreezeFolder::new(&this.env);
                folder.freeze_facts(&mut this.facts);
                let frozen_items = FreezeFolder::new(&this.env).freeze_items(inferred_items);

                let typed_file = File {
                    id: file_id,
                    module_id: file.module_id,
                    name: file.name,
                    source: file.source,
                    items: frozen_items,
                };

                this.typed_files.push((module_id.to_string(), typed_file));
            },
        );
    }

    fn check_definition_module_collisions(
        &mut self,
        store: &Store,
        items: &[Expression],
        imports: &[FileImport],
    ) {
        let alias_to_path: HashMap<String, String> = imports
            .iter()
            .filter_map(|imp| {
                imp.effective_alias(&store.go_package_names)
                    .map(|alias| (alias, imp.name.to_string()))
            })
            .collect();

        for item in items {
            let (definition_name, name_span) = match item {
                Expression::Function {
                    name, name_span, ..
                } => (name.as_str(), *name_span),
                Expression::Struct {
                    name, name_span, ..
                } => (name.as_str(), *name_span),
                Expression::Enum {
                    name, name_span, ..
                } => (name.as_str(), *name_span),
                Expression::ValueEnum {
                    name, name_span, ..
                } => (name.as_str(), *name_span),
                Expression::TypeAlias {
                    name, name_span, ..
                } => (name.as_str(), *name_span),
                Expression::Const {
                    identifier,
                    identifier_span,
                    ..
                } => (identifier.as_str(), *identifier_span),
                Expression::Interface {
                    name, name_span, ..
                } => (name.as_str(), *name_span),
                _ => continue,
            };

            if let Some(import_path) = alias_to_path.get(definition_name) {
                self.sink
                    .push(diagnostics::infer::definition_shadows_import(
                        definition_name,
                        import_path,
                        name_span,
                    ));
            }
        }
    }
}
