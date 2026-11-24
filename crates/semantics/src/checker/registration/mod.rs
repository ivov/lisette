mod attributes;
mod builtins;
mod convert;
pub(crate) use convert::VarArgsPosition;
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
    wrap_with_impl_generics,
};
use std::mem;

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

/// Declarations in the current context, or a package whose files need their own contexts.
enum RegistrationTarget<'a> {
    Current {
        items: &'a mut [Expression],
        visibility: &'a Visibility,
    },
    Package(&'a mut PredeclaredPackage),
}

impl RegistrationTarget<'_> {
    /// Complete a phase across every file before starting the next phase.
    fn for_each(
        &mut self,
        state: &mut TaskState,
        store: &mut Store,
        mut phase: impl FnMut(&mut TaskState, &mut Store, &mut [Expression], &Visibility),
    ) {
        match self {
            Self::Current { items, visibility } => phase(state, store, items, visibility),
            Self::Package(package) => {
                for file in &mut package.files {
                    state.with_file_context_mut(
                        store,
                        FileContext::Standard {
                            package_id: &package.id,
                            file_id: file.id,
                            imports: &file.imports,
                        },
                        |state, store| phase(state, store, &mut file.items, &Visibility::Private),
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum PackageItemKind {
    Struct,
    Enum,
    Interface,
    TypeAlias,
    Function,
    Constant,
    Variable,
}

impl PackageItemKind {
    fn label(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::TypeAlias => "type alias",
            Self::Function => "function",
            Self::Constant => "constant",
            Self::Variable => "variable",
        }
    }

    /// A type name enters early as a placeholder, so a type conflicts only with a type body.
    fn conflicts_with(self, existing: &Definition) -> bool {
        match self {
            Self::Struct | Self::Enum | Self::Interface | Self::TypeAlias => matches!(
                existing.body,
                DefinitionBody::Struct { .. }
                    | DefinitionBody::Enum { .. }
                    | DefinitionBody::Interface { .. }
                    | DefinitionBody::TypeAlias { .. }
            ),
            Self::Function | Self::Constant | Self::Variable => true,
        }
    }
}

struct PackageItem<'a> {
    kind: PackageItemKind,
    name: &'a str,
    name_span: Span,
}

#[must_use = "a registered package owns the ASTs that must be passed to inference"]
pub struct RegisteredPackage {
    pub(crate) id: String,
    pub(crate) files: Vec<RegistrationFile>,
}

impl TaskState {
    /// Register declarations in dependency order, after all type names are available.
    fn register_declarations(&mut self, store: &mut Store, mut target: RegistrationTarget<'_>) {
        let package_id = match &target {
            RegistrationTarget::Current { .. } => self.cursor.package_id().to_string(),
            RegistrationTarget::Package(package) => package.id.clone(),
        };
        target.for_each(self, store, |this, store, items, visibility| {
            this.register_constants(store, items, visibility);
        });
        target.for_each(self, store, |this, store, items, _| {
            this.register_type_aliases(store, items);
        });
        target.for_each(self, store, |this, store, items, _| {
            this.register_type_bodies(store, items);
        });
        normalize_registered_component_types(store, &package_id);
        target.for_each(self, store, |this, store, items, _| {
            this.derive_enum_constructor_values(store, items);
        });
        target.for_each(self, store, |this, store, items, visibility| {
            this.check_type_generic_bounds(store, items);
            this.register_impl_blocks(store, items);
            this.register_non_const_values(store, items, visibility);
        });
    }

    /// Completes work that must wait until every package has been registered.
    pub fn finalize_registration(&mut self, store: &mut Store) {
        self.finalize_equality(store);
        store.compute_go_method_policy();
        self.check_pending_generic_bounds(store);
        self.check_pending_array_size_checks(store);
        self.check_pending_interface_conflicts(store);
        self.finalize_tests(store);
    }

    fn define_package_item(
        &mut self,
        store: &mut Store,
        item: PackageItem<'_>,
        qualified_name: Symbol,
        definition: Definition,
    ) -> bool {
        let taken = self
            .current_package(&*store)
            .definitions
            .get(qualified_name.as_str())
            .is_some_and(|existing| item.kind.conflicts_with(existing));
        if taken && self.is_lis(&*store) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                item.kind.label(),
                item.name,
                item.name_span,
            ));
            return false;
        }
        self.current_package_mut(store)
            .definitions
            .insert(qualified_name, definition);
        true
    }

    pub fn register_package(&mut self, store: &mut Store, id: &str) -> RegisteredPackage {
        let package = self.predeclare_package(store, id);
        self.register_predeclared_package(store, package)
    }

    pub(crate) fn predeclare_package(&mut self, store: &mut Store, id: &str) -> PredeclaredPackage {
        let type_name_entries = self.collect_package_type_name_entries(store, id);
        self.insert_type_name_entries(store, id, type_name_entries);

        let file_inputs: Vec<_> = store
            .get_package(id)
            .into_iter()
            .flat_map(|package| package.files.values())
            .map(|file| (file.id, file.imports()))
            .collect();
        let files = file_inputs
            .into_iter()
            .map(|(id, imports)| RegistrationFile {
                id,
                imports,
                items: mem::take(store.file_items_mut(id).expect("registered file exists")),
            })
            .collect();
        PredeclaredPackage {
            id: id.to_string(),
            files,
        }
    }

    pub(crate) fn register_predeclared_package(
        &mut self,
        store: &mut Store,
        mut package: PredeclaredPackage,
    ) -> RegisteredPackage {
        self.register_declarations(store, RegistrationTarget::Package(&mut package));
        let PredeclaredPackage { id, files } = package;

        self.register_package_derived_attributes(store, &id, &files);
        self.validate_package_embeds(store, &id);
        self.check_package_recursive_types(store, &id);

        self.register_package_tests(store, &id, &files);
        RegisteredPackage { id, files }
    }
}

/// A forward or self reference converts before its definition exists.
pub(crate) fn normalize_registered_component_types(store: &mut Store, package_id: &str) {
    let Some(package) = store.get_package(package_id) else {
        return;
    };
    let mut changed: Vec<(Symbol, Definition)> = Vec::new();
    for (name, definition) in &package.definitions {
        if !any_component_type(&definition.body, |ty| {
            store.normalized_annotation_type(ty) != *ty
        }) {
            continue;
        }
        let mut updated = definition.clone();
        let normalize = |ty: &mut Type| {
            *ty = store.normalized_annotation_type(ty);
        };
        match &mut updated.body {
            DefinitionBody::Struct { fields, .. } => {
                let (StructFields::Record(fields) | StructFields::Tuple(fields)) = fields;
                for field in fields {
                    normalize(&mut field.ty);
                }
            }
            DefinitionBody::Enum { variants, .. } => {
                for variant in variants {
                    match &mut variant.fields {
                        VariantFields::Unit => {}
                        VariantFields::Tuple(fields) | VariantFields::Struct(fields) => {
                            for field in fields {
                                normalize(&mut field.ty);
                            }
                        }
                    }
                }
            }
            DefinitionBody::TypeAlias {
                alias: AliasKind::Transparent { target, .. },
                ..
            } => {
                normalize(target);
            }
            DefinitionBody::Interface { definition } => {
                for method in definition.methods.values_mut() {
                    normalize(&mut method.ty);
                }
            }
            _ => {}
        }
        changed.push((name.clone(), updated));
    }
    if changed.is_empty() {
        return;
    }
    let Some(package) = store.edit_package(package_id) else {
        return;
    };
    for (name, definition) in changed {
        package.definitions.insert(name, definition);
    }
}

/// Whether any type written in a definition's components satisfies `predicate`.
fn any_component_type(body: &DefinitionBody, mut predicate: impl FnMut(&Type) -> bool) -> bool {
    match body {
        DefinitionBody::Struct { fields, .. } => {
            let (StructFields::Record(fields) | StructFields::Tuple(fields)) = fields;
            fields.iter().any(|field| predicate(&field.ty))
        }
        DefinitionBody::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| match &variant.fields {
                VariantFields::Unit => [].as_slice(),
                VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields,
            })
            .any(|field| predicate(&field.ty)),
        DefinitionBody::TypeAlias {
            alias: AliasKind::Transparent { target, .. },
            ..
        } => predicate(target),
        DefinitionBody::Interface { definition } => definition
            .methods
            .values()
            .any(|method| predicate(&method.ty)),
        _ => false,
    }
}
