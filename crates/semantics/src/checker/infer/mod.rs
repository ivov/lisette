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
use super::registration::{RegisteredPackage, RegistrationFile};
use super::state::InferredFile;
use super::{FileContext, TaskState};
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::FileImport;

impl TaskState {
    pub(crate) fn install_inferred_files(store: &mut Store, inferred_files: Vec<InferredFile>) {
        for inferred_file in inferred_files {
            store
                .get_file_mut(inferred_file.id)
                .expect("inferred file must remain in the store")
                .items = inferred_file.items;
        }
    }

    /// Infer one registered package and install its typed ASTs.
    pub fn infer_package(&mut self, store: &mut Store, package: RegisteredPackage) {
        let inferred_files = InferCtx::new(self, store).infer_package(package);
        Self::install_inferred_files(store, inferred_files);
    }
}

impl InferCtx<'_> {
    pub(crate) fn infer_package(&mut self, package: RegisteredPackage) -> Vec<InferredFile> {
        let package_id = package.id;
        let (files, typedefs): (Vec<_>, Vec<_>) = package.files.into_iter().partition(|file| {
            !self
                .store
                .get_file(file.id)
                .expect("registered file must remain in the store")
                .is_d_lis()
        });
        let items_per_file: Vec<&[Expression]> = files.iter().map(|f| f.items.as_slice()).collect();
        self.check_const_cycles(&items_per_file);

        let mut inferred: Vec<_> = files
            .into_iter()
            .map(|file| self.infer_file(&package_id, file))
            .collect();
        inferred.extend(
            typedefs
                .into_iter()
                .map(|file: RegistrationFile| InferredFile {
                    id: file.id,
                    items: file.items,
                }),
        );
        inferred
    }

    fn infer_file(&mut self, package_id: &str, file: RegistrationFile) -> InferredFile {
        assert!(
            self.file_checks.is_empty(),
            "file checks from the previous file must be resolved"
        );
        let file_id = file.id;
        let imports = file.imports;

        let inferred = self.with_file_context(
            FileContext::Standard {
                package_id,
                file_id,
                imports: &imports,
            },
            |ctx| {
                ctx.check_definition_package_collisions(&file.items, &imports);

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
                    let state = &mut *ctx.state;
                    let folder = FreezeFolder::new(&state.env, store);
                    folder.freeze_facts(&mut state.facts);
                    FreezeFolder::new(&state.env, store).freeze_items(inferred_items)
                };

                InferredFile {
                    id: file_id,
                    items: frozen_items,
                }
            },
        );
        assert!(
            self.file_checks.is_empty(),
            "file checks must be resolved before inference returns"
        );
        inferred
    }

    fn check_definition_package_collisions(
        &mut self,
        items: &[Expression],
        imports: &[FileImport],
    ) {
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
            && name != crate::prelude::PRELUDE_PACKAGE_ID
            && let Some(import_path) = self.imports.package_id(name)
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
            let mut generics = generics.clone();
            this.extract_signature_parts(store, &mut generics, params, return_annotation, span)
        });

        let scope = self.scopes.current_mut();
        scope.insert_value(name.to_string(), fn_ty);
    }
}
