mod attributes;
mod builtins;
mod convert;
mod declarations;
pub(crate) mod derived_attributes;
mod display;
mod equality;
mod generic_bounds;
mod go;
mod impl_bounds;
mod iterate;
mod metadata;
mod methods;
pub(crate) mod test_functions;
mod types;
mod values;

use attributes::*;
use metadata::{
    declaration_value_position_types, enum_variant_constructor_type, function_signature_pairs,
    has_recursive_instantiation, wrap_with_impl_generics,
};
use std::mem;

use std::path::PathBuf;

use deps::TypedefLocator;

use crate::diagnostics::{GoImportSite, emit_for_locator_result};
use syntax::ast::{
    Annotation, Attribute, AttributeArg, Binding, EnumVariant, Expression, Generic, Span,
    StructFields, VariantFields, Visibility as SyntacticVisibility,
};
use syntax::attributes::struct_attribute_forces_field_export;
use syntax::program::{
    AliasKind, Attributes, Definition, DefinitionBody, File, FileImport, TypeAttribute, Visibility,
};
use syntax::types::{Bound, FunctionParameter, Symbol, Type};

use super::{FileContext, TaskState, resolved_generic_bounds};
use crate::store::Store;

pub(crate) struct RegistrationFile {
    pub(crate) id: u32,
    pub(crate) imports: Vec<FileImport>,
    pub(crate) items: Vec<Expression>,
}

pub(crate) struct PredeclaredPackage {
    pub(crate) id: String,
    files: Vec<RegistrationFile>,
}

#[must_use = "a registered package owns the ASTs that must be passed to inference"]
pub struct RegisteredPackage {
    pub(crate) id: String,
    pub(crate) files: Vec<RegistrationFile>,
}

impl TaskState {
    /// Completes work that must wait until every package has been registered.
    pub fn finalize_registration(&mut self, store: &mut Store) {
        self.finalize_equality(store);
        self.check_pending_generic_bounds(store);
        self.check_pending_array_size_checks(store);
        self.finalize_tests(store);
    }

    fn definition_exists(&self, store: &Store, qualified_name: &str) -> bool {
        self.current_package(store)
            .definitions
            .contains_key(qualified_name)
    }

    fn type_definition_exists(&self, store: &Store, qualified_name: &str) -> bool {
        self.current_package(store)
            .definitions
            .get(qualified_name)
            .is_some_and(|d| {
                matches!(
                    d.body,
                    DefinitionBody::Struct { .. }
                        | DefinitionBody::Enum { .. }
                        | DefinitionBody::Interface { .. }
                        | DefinitionBody::TypeAlias { .. }
                )
            })
    }

    pub fn register_package(&mut self, store: &mut Store, id: &str) -> RegisteredPackage {
        let package = self.predeclare_package(store, id);
        self.register_predeclared_package(store, package)
    }

    pub(crate) fn predeclare_package(&mut self, store: &mut Store, id: &str) -> PredeclaredPackage {
        let type_name_entries = self.collect_package_type_name_entries(store, id);
        self.insert_type_name_entries(store, id, type_name_entries);

        let files = {
            let package = store
                .get_package_mut(id)
                .expect("package must exist for registration");
            package
                .files
                .values_mut()
                .map(|file| RegistrationFile {
                    id: file.id,
                    imports: file.imports(),
                    items: mem::take(&mut file.items),
                })
                .collect::<Vec<_>>()
        };
        PredeclaredPackage {
            id: id.to_string(),
            files,
        }
    }

    pub(crate) fn register_predeclared_package(
        &mut self,
        store: &mut Store,
        package: PredeclaredPackage,
    ) -> RegisteredPackage {
        let PredeclaredPackage { id, mut files } = package;

        for file in &mut files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    package_id: &id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| this.register_constants(store, &file.items, &Visibility::Private),
            );
        }

        for file in &mut files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    package_id: &id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| this.register_type_aliases(store, &mut file.items),
            );
        }

        for file in &mut files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    package_id: &id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| this.register_type_bodies(store, &mut file.items),
            );
        }

        for file in &mut files {
            self.with_file_context_mut(
                store,
                FileContext::Standard {
                    package_id: &id,
                    file_id: file.id,
                    imports: &file.imports,
                },
                |this, store| {
                    this.check_type_generic_bounds(store, &file.items);
                    this.register_impl_blocks(store, &mut file.items);
                    this.register_non_const_values(store, &mut file.items, &Visibility::Private);
                },
            );
        }

        self.register_package_derived_attributes(store, &id, &files);
        self.validate_package_embeds(store, &id);
        self.check_package_recursive_types(store, &id);

        self.register_package_tests(store, &id, &files);
        RegisteredPackage { id, files }
    }
}
