pub(crate) mod addressability;
pub(crate) mod constant_values;
mod context;
mod equality;
pub(crate) mod expressions;
mod generic_obligations;
pub(crate) mod interface;
mod unify;
mod validation;
mod value_uses;

pub use context::InferCtx;
pub(crate) use unify::BuiltinBound;

use crate::facts::BindingOrigin;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::BindingKind;

use super::freeze::FreezeFolder;
use super::registration::{RegisteredPackage, RegistrationFile};
use super::state::InferredFile;
use super::{EnvResolve, FileContext, TaskState};
use crate::loader;
use crate::prelude;
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::FileImport;
use syntax::types::Type;

impl TaskState {
    pub(crate) fn install_inferred_files(store: &mut Store, inferred_files: Vec<InferredFile>) {
        for inferred_file in inferred_files {
            *store
                .file_items_mut(inferred_file.id)
                .expect("inferred file must remain in the store") = inferred_file.items;
        }
    }

    /// Infer one registered package and install its typed ASTs.
    pub fn infer_package(&mut self, store: &mut Store, package: RegisteredPackage) {
        let package_id = package.id.clone();
        let inferred_files = self.infer_registered_package(store, package);
        Self::install_inferred_files(store, inferred_files);
        constant_values::evaluate_package_constants(store, &package_id);
    }

    pub(crate) fn infer_registered_package(
        &mut self,
        store: &Store,
        package: RegisteredPackage,
    ) -> Vec<InferredFile> {
        let package_id = package.id;
        let (files, typedefs): (Vec<_>, Vec<_>) = package.files.into_iter().partition(|file| {
            !store
                .get_file(file.id)
                .expect("registered file must remain in the store")
                .is_d_lis()
        });
        let items_per_file: Vec<&[Expression]> = files.iter().map(|f| f.items.as_slice()).collect();
        self.check_const_cycles(&items_per_file);

        let mut inferred: Vec<_> = files
            .into_iter()
            .map(|file| InferCtx::new(self, store).infer_file(&package_id, file))
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
}

impl InferCtx<'_> {
    /// Resolve inference variables before following the outer alias chain.
    fn peel_resolved(&self, ty: &Type) -> Type {
        self.store.peel_alias(&ty.resolve_in(&self.env))
    }

    fn infer_file(mut self, package_id: &str, file: RegistrationFile) -> InferredFile {
        let file_id = file.id;
        let imports = file.imports;

        self.with_file_context(
            FileContext::Standard {
                package_id,
                file_id,
                imports: &imports,
            },
            |context| {
                context.check_definition_package_collisions(&file.items, &imports);

                let inferred_items: Vec<_> = file
                    .items
                    .into_iter()
                    .map(|item| {
                        let type_var = context.new_type_var();
                        context.infer_root_expression(item, &type_var)
                    })
                    .collect();

                context.check_reference_sibling_aliasing(&inferred_items);
                context.check_map_bracket_reads(&inferred_items);
                let context::FileObligations {
                    branch_subsumptions,
                    select_exhaustiveness,
                    statement_tails,
                } = context.file_checks.discharge();
                context.resolve_branch_subsumptions(branch_subsumptions);
                context.resolve_select_exhaustiveness(select_exhaustiveness);
                context.resolve_statement_tails(statement_tails);

                let frozen_items = {
                    let store = context.store;
                    let state = &mut *context.state;
                    let folder = FreezeFolder::new(&state.env, store);
                    folder.freeze_facts(&mut state.facts);
                    FreezeFolder::new(&state.env, store).freeze_items(inferred_items)
                };
                context.check_instantiation_bounds(&frozen_items);
                context.finalize_value_uses(&frozen_items, file_id);

                InferredFile {
                    id: file_id,
                    items: frozen_items,
                }
            },
        )
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
                    loader::import_display_name(import_path),
                    name_span,
                ));
            }
        }
    }

    fn check_binding_shadows_import(&mut self, name: &str, span: Span, is_typedef: bool) {
        if !is_typedef
            && name != prelude::PRELUDE_PACKAGE_ID
            && let Some(import_path) = self.imports.package_id(name)
        {
            self.sink.push(diagnostics::infer::name_shadows_import(
                name,
                loader::import_display_name(import_path),
                span,
            ));
        }
    }

    fn register_block_local_items(&mut self, items: &mut [Expression]) {
        for item in items {
            match item {
                Expression::Const { .. } => self.register_block_local_const(item),
                Expression::Function { .. } => self.register_block_local_fn(item),
                _ => {}
            }
        }
    }

    fn register_block_local_const(&mut self, item: &mut Expression) {
        let store = self.store;
        let Expression::Const {
            binding,
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

        let id = self.facts.add_binding(
            identifier.to_string(),
            *identifier_span,
            BindingKind::Const,
            BindingOrigin::Name {
                in_typedef: false,
                shorthand_field: false,
            },
            None,
        );
        *binding = Some(id);
        self.scopes
            .current_mut()
            .insert_const(identifier.to_string(), const_ty, id);
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
