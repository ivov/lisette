pub(crate) mod addressability;
mod carry_mut;
mod context;
pub(crate) mod expressions;
mod generic_obligations;
pub(crate) mod interface;
mod unify;
mod validation;

pub use context::InferCtx;
pub(crate) use unify::BuiltinBound;

use rustc_hash::FxHashMap as HashMap;

use super::freeze::FreezeFolder;
use super::{FileContext, InferredFile, TaskState};
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::FileImport;

pub(crate) struct FileInferenceInput {
    pub(crate) id: u32,
    pub(crate) imports: Vec<FileImport>,
    pub(crate) items: Vec<Expression>,
}

impl TaskState {
    pub(crate) fn take_module_inference_input(
        store: &mut Store,
        module_id: &str,
    ) -> Vec<FileInferenceInput> {
        let module = store
            .get_module_mut(module_id)
            .expect("module must exist for inference");
        let file_data = module
            .source_file_entries()
            .map(|(file_id, file)| (*file_id, file.imports()))
            .collect::<Vec<_>>();
        file_data
            .into_iter()
            .map(|(file_id, imports)| FileInferenceInput {
                id: file_id,
                imports,
                items: std::mem::take(
                    &mut module
                        .files
                        .get_mut(&file_id)
                        .expect("source file must remain in its module")
                        .items,
                ),
            })
            .collect()
    }

    pub(crate) fn install_inferred_files(&mut self, store: &mut Store) {
        for inferred_file in std::mem::take(&mut self.inferred_files) {
            store
                .get_file_mut(inferred_file.id)
                .expect("inferred file must remain in the store")
                .items = inferred_file.items;
        }
    }

    /// Infer one registered module and replace its source ASTs with their typed forms.
    pub fn infer_module(&mut self, store: &mut Store, module_id: &str) {
        let files = Self::take_module_inference_input(store, module_id);
        InferCtx::new(self, store).infer_module(module_id, files);
        self.install_inferred_files(store);
    }
}

impl InferCtx<'_> {
    /// Infer types for `files` belonging to `module_id`.
    pub(crate) fn infer_module(&mut self, module_id: &str, files: Vec<FileInferenceInput>) {
        let items_per_file: Vec<&[Expression]> = files.iter().map(|f| f.items.as_slice()).collect();
        self.check_const_cycles(&items_per_file);

        for file in files {
            self.infer_file(module_id, file);
        }
    }

    fn infer_file(&mut self, module_id: &str, file: FileInferenceInput) {
        let store = self.store;
        let file_id = file.id;
        let imports = file.imports;

        self.with_file_context(
            store,
            FileContext::Standard {
                module_id,
                file_id,
                imports: &imports,
            },
            |this, store| {
                let mut ctx = InferCtx::new(this, store);
                ctx.check_definition_module_collisions(&file.items, &imports);

                let inferred_items: Vec<_> = file
                    .items
                    .into_iter()
                    .map(|item| {
                        let type_var = ctx.new_type_var();
                        ctx.infer_root_expression(item, &type_var)
                    })
                    .collect();

                ctx.check_reference_sibling_aliasing(&inferred_items);
                ctx.check_map_bracket_reads(&inferred_items);
                ctx.resolve_branch_subsumptions();
                ctx.resolve_select_exhaustiveness();

                let frozen_items = {
                    let store = ctx.store;
                    let state = &mut *ctx;
                    let folder = FreezeFolder::new(&state.env, store);
                    folder.freeze_facts(&mut state.facts);
                    FreezeFolder::new(&state.env, store).freeze_items(inferred_items)
                };

                ctx.inferred_files.push(InferredFile {
                    id: file_id,
                    items: frozen_items,
                });
            },
        );
    }

    fn check_definition_module_collisions(&mut self, items: &[Expression], imports: &[FileImport]) {
        let store = self.store;
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
                self.sink.push(diagnostics::infer::name_shadows_import(
                    definition_name,
                    crate::loader::import_display_name(import_path),
                    name_span,
                ));
            }
        }
    }

    fn check_binding_shadows_import(&mut self, name: &str, span: Span, is_typedef: bool) {
        if !is_typedef
            && name != crate::prelude::PRELUDE_MODULE_ID
            && let Some(import_path) = self.imports.module_id(name)
        {
            self.sink.push(diagnostics::infer::name_shadows_import(
                name,
                crate::loader::import_display_name(import_path),
                span,
            ));
        }
    }

    fn register_block_local_items(&mut self, items: &[Expression]) {
        for item in items {
            match item {
                Expression::Const { .. } => self.register_block_local_const(item),
                Expression::Function { .. } => self.register_block_local_fn(item),
                _ => {}
            }
        }
    }

    fn register_block_local_const(&mut self, item: &Expression) {
        let store = self.store;
        let Expression::Const {
            identifier,
            identifier_span,
            annotation,
            expression,
            span,
            ..
        } = item
        else {
            return;
        };

        self.check_binding_shadows_import(identifier, *identifier_span, self.is_d_lis(store));

        let qualified_name = self.qualify_name(identifier);
        let is_duplicate = self.scopes.lookup_const(identifier)
            || self.is_const_name(store, qualified_name.as_str());
        if is_duplicate && self.is_lis(store) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "constant",
                identifier,
                *identifier_span,
            ));
            return;
        }

        let const_ty = self.without_diagnostics(|this| {
            if let Some(annotation) = annotation {
                this.convert_to_type(store, annotation, span)
            } else {
                expression
                    .value()
                    .and_then(|value| this.type_from_literal_expression(value))
                    .unwrap_or_else(|| this.new_type_var())
            }
        });

        let scope = self.scopes.current_mut();
        scope.insert_const(identifier.to_string(), const_ty);
    }

    fn register_block_local_fn(&mut self, item: &Expression) {
        let Expression::Function {
            name,
            generics,
            params,
            return_annotation,
            span,
            ..
        } = item
        else {
            return;
        };

        let store = self.store;
        let fn_ty = self.without_diagnostics(|this| {
            this.extract_signature_parts(store, generics, params, return_annotation, span)
        });

        let scope = self.scopes.current_mut();
        scope.insert_value(name.to_string(), fn_ty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::program::File;

    #[test]
    fn inference_detaches_items_without_removing_the_file() {
        let mut store = Store::new();
        store.add_module("m");
        store.store_file(File::new_cached(
            "m",
            "example.test.lis",
            "example.test.lis",
            "",
            42,
        ));

        let inputs = TaskState::take_module_inference_input(&mut store, "m");

        assert_eq!(inputs.len(), 1);
        assert!(store.get_file(42).is_some());
        assert!(store.is_test_file(42));
    }
}
