pub mod checks;
mod expressions;
mod interface;
mod unify;
mod validation;

use rustc_hash::FxHashMap as HashMap;

use super::Checker;
use syntax::ast::Expression;
use syntax::program::{File, FileImport};

impl Checker<'_, '_> {
    pub fn infer_module(&mut self, module_id: &str) {
        self.cursor.module_id = module_id.to_string();

        let files: Vec<File> = {
            let module = self
                .store
                .get_module_mut(module_id)
                .expect("module must exist for inference");
            std::mem::take(&mut module.files).into_values().collect()
        };

        for file in files {
            let imports = file.imports();

            self.reset_scopes();
            self.cursor.file_id = Some(file.id);
            self.put_prelude_in_scope();
            self.put_unprefixed_module_in_scope(module_id);
            self.put_imported_modules_in_scope(&imports);
            self.check_definition_module_collisions(&file.items, &imports);

            let inferred_items: Vec<_> = file
                .items
                .into_iter()
                .map(|item| {
                    let type_var = self.new_type_var();
                    self.infer_expression(item, &type_var)
                })
                .collect();

            self.run_post_inference_checks();
            self.check_reference_sibling_aliasing(&inferred_items);

            let typed_file = File {
                id: file.id,
                module_id: file.module_id,
                name: file.name,
                source: file.source,
                items: inferred_items,
            };

            self.store.store_file(module_id, typed_file);
        }

        self.cursor.file_id = None;
    }

    fn check_definition_module_collisions(&mut self, items: &[Expression], imports: &[FileImport]) {
        let alias_to_path: HashMap<String, String> = imports
            .iter()
            .filter_map(|imp| {
                imp.effective_alias(&self.store.go_package_names)
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
