use std::mem::take;
use std::sync::Arc;
use syntax::collections::DenseMap;
use syntax::constant::ConstantSource;

use rustc_hash::FxHashMap as HashMap;

use syntax::EcoString;
use syntax::ast::{EnumVariant, StructFieldDefinition};
use syntax::ast::{Expression, StructKind};
use syntax::program::{
    Definition, DefinitionBody, EqualityIndex, File, Interface, Method, Methods, Package,
    TestIndex, UninferredExports, method_for_type, methods_for_type, type_has_any_method,
};
use syntax::types;
use syntax::types::{AliasPeeled, CompoundKind, SimpleKind, Symbol, Type};

pub use crate::closed_domain::{ClosedDomain, ClosedMember, DomainValue};
pub use syntax::ENTRY_PACKAGE_ID;
pub const ENTRY_FILE_ID: u32 = 0;

pub struct PackageMetadataMut<'a> {
    pub definitions: &'a mut DenseMap<Symbol, Definition>,
    pub unsupported_go_symbols: &'a mut HashMap<EcoString, String>,
    pub uninferred_exports: &'a mut Option<UninferredExports>,
}

#[derive(Clone)]
pub struct Store {
    /// `Arc` so registration workers share a read view; [`Arc::make_mut`]
    /// writes stay zero-copy while a package has a single owner.
    packages: HashMap<String, Arc<Package>>,
    /// Avoids scanning every package on the hot file-lookup path (see #1227).
    file_packages: HashMap<u32, String>,
    /// Go package ID -> package name from the typedef `// Package:` directive.
    pub go_package_names: HashMap<String, String>,
    /// File ID counter. Starts at 2 because 0 is reserved for entry, 1 for prelude.
    next_file_id: u32,
    pub equality_index: EqualityIndex,
    pub test_index: TestIndex,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        let prelude_package = Package::new("prelude");
        let nominal_package = Package::nominal();

        let packages = vec![
            (prelude_package.id.clone(), Arc::new(prelude_package)),
            (nominal_package.id.clone(), Arc::new(nominal_package)),
        ]
        .into_iter()
        .collect();

        Self {
            packages,
            file_packages: HashMap::default(),
            go_package_names: Default::default(),
            next_file_id: 2, // 0 = entrypoint, 1 = prelude
            equality_index: Default::default(),
            test_index: Default::default(),
        }
    }

    pub fn new_file_id(&mut self) -> u32 {
        let id = self.next_file_id;
        self.next_file_id += 1;
        id
    }

    pub(crate) fn reserve_file_ids(&mut self, count: u32) -> u32 {
        let first = self.next_file_id;
        self.next_file_id += count;
        first
    }

    /// Creates the entry package (empty for a library, whose root files load as siblings).
    pub(crate) fn init_entry_package(&mut self) {
        self.add_package(ENTRY_PACKAGE_ID);
    }

    pub fn store_package(&mut self, package_id: &str, files: Vec<File>) {
        self.add_package(package_id);

        for file in files {
            assert_eq!(file.package_id, package_id);
            self.store_file(file);
        }
    }

    pub(crate) fn store_uninferred_package(
        &mut self,
        package_id: &str,
        files: Vec<File>,
        exports: UninferredExports,
    ) {
        self.store_package(package_id, files);
        if let Some(package) = self.edit_package(package_id) {
            *package.uninferred_exports = Some(exports);
        }
    }

    pub fn store_file(&mut self, file: File) {
        let owner = file.package_id.clone();
        let id = file.id;
        if let Some(previous) = self.file_packages.get(&id)
            && previous != &owner
            && let Some(package) = self.packages.get_mut(previous)
        {
            Arc::make_mut(package).files.remove(&id);
        }
        Arc::make_mut(
            self.packages
                .get_mut(&owner)
                .expect("file owner must exist"),
        )
        .files
        .insert(id, file);
        self.file_packages.insert(id, owner);
    }

    pub fn get_file(&self, file_id: u32) -> Option<&File> {
        self.packages
            .get(self.file_packages.get(&file_id)?)?
            .get_file(file_id)
    }

    fn get_file_mut(&mut self, file_id: u32) -> Option<&mut File> {
        let owner = self.file_packages.get(&file_id)?;
        Arc::make_mut(self.packages.get_mut(owner)?)
            .files
            .get_mut(&file_id)
    }

    pub fn packages(&self) -> &HashMap<String, Arc<Package>> {
        &self.packages
    }

    pub fn take_packages(&mut self) -> HashMap<String, Arc<Package>> {
        self.file_packages.clear();
        take(&mut self.packages)
    }

    pub(crate) fn take_package(&mut self, id: &str) -> Option<Arc<Package>> {
        let package = self.packages.remove(id)?;
        for file_id in package.files.keys() {
            self.file_packages.remove(file_id);
        }
        Some(package)
    }

    pub(crate) fn insert_shared_package(&mut self, package: Arc<Package>) {
        self.take_package(&package.id);
        self.file_packages
            .extend(package.files.keys().map(|&id| (id, package.id.clone())));
        self.packages.insert(package.id.clone(), package);
    }

    pub(crate) fn file_items_mut(&mut self, id: u32) -> Option<&mut Vec<Expression>> {
        self.get_file_mut(id).map(|file| &mut file.items)
    }

    pub fn get_package(&self, package_id: &str) -> Option<&Package> {
        self.packages.get(package_id).map(Arc::as_ref)
    }

    pub(crate) fn has(&self, package_id: &str) -> bool {
        self.packages.contains_key(package_id)
    }

    pub(crate) fn uninferred_package_may_export(&self, package_id: &str, member: &str) -> bool {
        self.packages
            .get(package_id)
            .and_then(|package| package.uninferred_exports.as_ref())
            .is_some_and(|exports| exports.may_contain(member))
    }

    pub(crate) fn unsupported_go_symbol(&self, package_id: &str, member: &str) -> Option<&str> {
        self.packages
            .get(package_id)?
            .unsupported_go_symbols
            .get(member)
            .map(String::as_str)
    }

    pub fn add_package(&mut self, package_id: &str) {
        if self.packages.contains_key(package_id) {
            return;
        }

        self.packages
            .insert(package_id.to_string(), Arc::new(Package::new(package_id)));
    }

    pub fn edit_package(&mut self, package_id: &str) -> Option<PackageMetadataMut<'_>> {
        let Package {
            id: _,
            files: _,
            definitions,
            unsupported_go_symbols,
            uninferred_exports,
        } = Arc::make_mut(self.packages.get_mut(package_id)?);
        Some(PackageMetadataMut {
            definitions,
            unsupported_go_symbols,
            uninferred_exports,
        })
    }

    /// Inserts a worker-built package (e.g. cache-decoded).
    pub(crate) fn insert_prebuilt_package(&mut self, package: Package) {
        self.insert_shared_package(Arc::new(package));
    }

    /// `Arc`-bump snapshot for a registration worker, which inserts its own
    /// detached package before use.
    pub(crate) fn registration_view(&self) -> Store {
        Store {
            packages: self.packages.clone(),
            file_packages: self.file_packages.clone(),
            go_package_names: self.go_package_names.clone(),
            next_file_id: self.next_file_id,
            equality_index: EqualityIndex::default(),
            test_index: TestIndex::default(),
        }
    }

    pub fn get_definition(&self, qualified_name: &str) -> Option<&Definition> {
        let package_name = self.package_for_qualified_name(qualified_name)?;

        self.get_package(package_name)?
            .definitions
            .get(qualified_name)
    }

    /// Whether a definition was declared in a `.test.lis` file. Production
    /// contexts must not resolve such definitions.
    pub fn is_test_definition(&self, definition: &Definition) -> bool {
        definition
            .name_span
            .and_then(|span| self.get_file(span.file_id))
            .is_some_and(File::is_test)
    }

    pub(crate) fn is_test_file(&self, file_id: u32) -> bool {
        self.get_file(file_id).is_some_and(File::is_test)
    }

    pub fn package_for_qualified_name<'a>(&'a self, qualified_name: &'a str) -> Option<&'a str> {
        types::package_for_qualified_name(qualified_name, |package| {
            self.packages.contains_key(package)
        })
    }

    pub(crate) fn is_const(&self, qualified_name: &str) -> bool {
        self.get_definition(qualified_name)
            .is_some_and(Definition::is_const)
    }

    pub fn variants_of(&self, qualified_name: &str) -> Option<&[EnumVariant]> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Enum { variants, .. } => Some(variants),
            _ => None,
        }
    }

    pub fn variant_of(&self, enum_qualified: &str, variant_name: &str) -> Option<&EnumVariant> {
        self.variants_of(enum_qualified)?
            .iter()
            .find(|v| v.name == variant_name)
    }

    pub(crate) fn is_nominal_defined_type(&self, qualified_name: &str) -> bool {
        match self.get_definition(qualified_name) {
            Some(def) => def.is_newtype(),
            None => false,
        }
    }

    pub(crate) fn fields_of(&self, qualified_name: &str) -> Option<&[StructFieldDefinition]> {
        self.get_definition(qualified_name)?.fields()
    }

    pub fn struct_kind(&self, qualified_name: &str) -> Option<StructKind> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { fields, .. } => Some(fields.kind()),
            _ => None,
        }
    }

    pub fn deep_struct_kind(&self, ty: &Type) -> Option<StructKind> {
        self.struct_kind(self.peel_alias(ty).get_qualified_id()?)
    }

    pub(crate) fn get_type(&self, qualified_name: &str) -> Option<&Type> {
        self.get_definition(qualified_name)
            .map(|definition| &definition.ty)
    }

    pub fn get_interface(&self, qualified_name: &str) -> Option<&Interface> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Interface { definition, .. } => Some(definition),
            _ => None,
        }
    }

    pub(crate) fn definition_has_equals_method(&self, qualified_id: &str) -> bool {
        matches!(
            self.get_definition(qualified_id).map(|d| &d.body),
            Some(DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. })
                if methods.contains_key("equals")
        )
    }

    pub fn is_interface(&self, ty: &Type) -> bool {
        matches!(ty, Type::Nominal { id, .. } if self.get_interface(id.as_str()).is_some())
    }

    pub fn is_nilable_go_type(&self, ty: &Type) -> bool {
        types::is_nilable_go_type(ty, |id| self.get_definition(id))
    }

    pub fn partial_ok_is_nilable(&self, ty: &Type) -> bool {
        types::partial_ok_is_nilable(ty, |id| self.get_definition(id))
    }

    pub fn peel_alias(&self, ty: &Type) -> Type {
        types::peel_alias(ty, |id| self.get_definition(id))
    }

    pub fn receiver_generic_context(&self, receiver_ty: &Type) -> Vec<(EcoString, Vec<Type>)> {
        let stripped = receiver_ty.strip_refs();
        let Type::Nominal { id, params, .. } = &stripped else {
            return Vec::new();
        };
        let Some(generics) = self
            .get_definition(id)
            .and_then(|definition| definition.body.generics())
        else {
            return Vec::new();
        };
        let substitution = types::build_substitution_map(generics, params);
        generics
            .iter()
            .zip(params)
            .filter_map(|(generic, param)| {
                let Type::Parameter(name) = param else {
                    return None;
                };
                let bounds = generic
                    .resolved_bounds()
                    .expect("generic bounds must be resolved before the impl block")
                    .map(|bound| types::substitute(bound, &substitution))
                    .collect();
                Some((name.clone(), bounds))
            })
            .collect()
    }

    pub fn receiver_shape(&self, ty: &Type) -> Type {
        types::receiver_shape(ty, |id| self.get_definition(id))
    }

    pub fn peeled(&self, ty: &Type) -> AliasPeeled {
        types::peel_alias_view(ty, |id| self.get_definition(id))
    }

    /// The nominal type a definition names once its type aliases are peeled.
    pub fn peel_alias_name(&self, qualified_name: &str) -> Option<Symbol> {
        match self.peel_alias(&self.get_definition(qualified_name)?.ty) {
            Type::Nominal { id, .. } => Some(id),
            _ => None,
        }
    }

    pub fn underlying_type(&self, ty: &Type) -> Option<Type> {
        types::underlying_type(ty, |id| self.get_definition(id))
    }

    pub fn peel_underlying(&self, ty: &Type) -> Type {
        types::peel_underlying(ty, |id| self.get_definition(id))
    }

    pub fn underlying_simple_kind(&self, ty: &Type) -> Option<SimpleKind> {
        types::underlying_simple_kind(ty, |id| self.get_definition(id))
    }

    pub fn underlying_numeric_type(&self, ty: &Type) -> Option<Type> {
        types::underlying_numeric_type(ty, |id| self.get_definition(id))
    }

    pub fn literal_adaptation_target(&self, ty: &Type) -> Option<Type> {
        types::literal_adaptation_target(ty, |id| self.get_definition(id))
    }

    pub fn is_numeric_compatible_with(&self, left: &Type, right: &Type) -> bool {
        types::is_numeric_compatible_with(left, right, |id| self.get_definition(id))
    }

    pub fn is_aliased_numeric_type(&self, ty: &Type) -> bool {
        types::is_aliased_numeric_type(ty, |id| self.get_definition(id))
    }

    pub fn has_underlying_numeric_type(&self, ty: &Type) -> bool {
        self.underlying_numeric_type(ty).is_some()
    }

    pub fn has_underlying_rune(&self, ty: &Type) -> bool {
        self.underlying_numeric_type(ty)
            .is_some_and(|ty| ty.is_rune())
    }

    pub fn has_underlying_byte(&self, ty: &Type) -> bool {
        self.underlying_numeric_type(ty)
            .is_some_and(|ty| ty.is_simple(SimpleKind::Byte) || ty.is_simple(SimpleKind::Uint8))
    }

    pub fn has_byte_or_rune_slice_underlying(&self, ty: &Type) -> bool {
        types::has_byte_or_rune_slice_underlying(ty, |id| self.get_definition(id))
    }

    pub fn is_orderable(&self, ty: &Type) -> bool {
        types::is_orderable(ty, |id| self.get_definition(id))
    }

    pub fn satisfies_ordered_constraint(&self, ty: &Type) -> bool {
        types::satisfies_ordered_constraint(ty, |id| self.get_definition(id))
    }

    pub fn resolves_to_unknown(&self, ty: &Type) -> bool {
        types::resolves_to_unknown(ty, |id| self.get_definition(id))
    }

    pub fn resolves_to_unit(&self, ty: &Type) -> bool {
        types::resolves_to_unit(ty, |id| self.get_definition(id))
    }

    pub fn contains_unknown(&self, ty: &Type) -> bool {
        types::contains_unknown(ty, |id| self.get_definition(id))
    }

    pub fn contains_write_permission(&self, ty: &Type) -> bool {
        types::contains_write_permission(ty, |id| self.get_definition(id))
    }

    pub fn parameter_grants_write(&self, ty: &Type) -> bool {
        types::parameter_grants_write(ty, |id| self.get_definition(id))
    }

    pub fn demoted(&self, ty: &Type) -> Type {
        types::demoted(ty, &|id| self.get_definition(id))
    }

    pub fn demotion_changes(&self, ty: &Type) -> bool {
        types::demotion_changes(ty, &|id| self.get_definition(id))
    }

    /// Binding demotion: the top-level hop and the nominal flag only.
    pub fn demoted_at_binding(&self, ty: &Type) -> Type {
        match ty {
            Type::Compound { kind, args, .. } if kind.carries_write_permission() => {
                let args = if args.iter().any(|arg| self.demotion_changes(arg)) {
                    args.iter().map(|arg| self.demoted(arg)).collect()
                } else {
                    args.clone()
                };
                Type::Compound {
                    kind: *kind,
                    args,
                    writable: false,
                }
            }
            Type::Nominal { id, params, .. } => {
                let cleared = Type::Nominal {
                    id: id.clone(),
                    params: params.clone(),
                    writable: false,
                };
                let peeled = self.peel_alias(&cleared);
                if peeled == cleared {
                    return cleared;
                }
                let demoted = self.demoted_at_binding(&peeled);
                if demoted == peeled { cleared } else { demoted }
            }
            other => other.clone(),
        }
    }

    pub fn nominal_declares_writable_components(&self, id: &str) -> bool {
        types::declares_writable_components(id, &|id| self.get_definition(id))
    }

    /// The conversion path demotes a refused qualifier once the body exists.
    fn nominal_refuses_write_permission(&self, id: &str) -> bool {
        matches!(
            self.get_definition(id).map(|definition| &definition.body),
            Some(DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. })
        ) && !self.nominal_declares_writable_components(id)
    }

    /// Canonical form for a type built from an annotation.
    pub fn normalized_annotation_type(&self, ty: &Type) -> Type {
        let mut mapped = ty.map_children(|child| self.normalized_annotation_type(child));
        match &mut mapped {
            Type::Compound {
                kind,
                args: arguments,
                writable,
            } => {
                if *kind == CompoundKind::Ref && *writable {
                    if let Some(argument) = arguments.first_mut() {
                        *argument =
                            types::writable_ref_pointee(argument, &|id| self.get_definition(id));
                    }
                } else if kind.carries_write_permission()
                    && !*writable
                    && arguments
                        .iter()
                        .any(|argument| self.demotion_changes(argument))
                {
                    *arguments = arguments
                        .iter()
                        .map(|argument| self.demoted(argument))
                        .collect();
                }
            }
            Type::Nominal { .. } if self.peel_alias(ty).is_never() => return Type::Never,
            Type::Nominal { id, writable, .. } => {
                *writable &= !self.nominal_refuses_write_permission(id.as_str());
            }
            Type::Simple(_)
            | Type::ImportNamespace(_)
            | Type::Function(_)
            | Type::Var { .. }
            | Type::Uninferred
            | Type::Ignored
            | Type::Forall { .. }
            | Type::Parameter(_)
            | Type::Never
            | Type::Tuple(_)
            | Type::Array { .. }
            | Type::Error
            | Type::ReceiverPlaceholder => {}
        }
        mapped
    }

    pub fn resolve_to_function_type(&self, ty: &Type) -> Option<Type> {
        types::resolve_to_function_type(ty, |id| self.get_definition(id))
    }

    pub fn peel_refs_and_aliases(&self, ty: &Type) -> (Type, bool) {
        (self.receiver_shape(ty), self.is_reference(ty))
    }

    pub fn is_reference(&self, ty: &Type) -> bool {
        types::is_reference(ty, |id| self.get_definition(id))
    }

    pub fn points_to_ref(&self, ty: &Type) -> bool {
        types::points_to_ref(ty, |id| self.get_definition(id))
    }

    pub fn is_option_or_result(&self, ty: &Type) -> bool {
        types::is_option_or_result(ty, |id| self.get_definition(id))
    }

    /// The definition that a type names, after following aliases.
    pub(crate) fn resolved_definition(&self, ty: &Type) -> Option<&Definition> {
        let ty = match ty {
            Type::Forall { body, .. } => body,
            other => other,
        };
        self.get_definition(self.peel_alias(ty).get_qualified_id()?)
    }

    pub(crate) fn peel_alias_deep(&self, ty: &Type) -> Type {
        match self.peel_alias(ty) {
            Type::Compound {
                kind,
                args,
                writable,
            } => Type::qualified_compound(
                kind,
                args.iter().map(|a| self.peel_alias_deep(a)).collect(),
                writable,
            ),
            Type::Tuple(elements) => {
                Type::Tuple(elements.iter().map(|e| self.peel_alias_deep(e)).collect())
            }
            Type::Array { length, element } => Type::Array {
                length,
                element: Box::new(self.peel_alias_deep(&element)),
            },
            Type::Nominal {
                id,
                params,
                writable,
            } => Type::Nominal {
                id,
                params: params.iter().map(|p| self.peel_alias_deep(p)).collect(),
                writable,
            },
            Type::Function(f) => {
                let new_params = f
                    .params
                    .iter()
                    .map(|p| p.with_type(self.peel_alias_deep(&p.ty)))
                    .collect();
                let new_return = Box::new(self.peel_alias_deep(&f.return_type));
                f.rebuild(new_params, f.bounds.clone(), new_return)
            }
            other => other,
        }
    }

    pub fn get_own_methods(&self, qualified_name: &str) -> Option<&Methods> {
        self.get_definition(qualified_name)?.methods()
    }

    pub fn get_method(&self, qualified_type: &str, method: &str) -> Option<&Method> {
        let methods = self.get_own_methods(qualified_type)?;
        methods.get(method).or_else(|| {
            methods
                .values()
                .find(|candidate| candidate.source_name == method)
        })
    }

    pub fn is_ufcs_method(&self, qualified_type: &str, method: &str) -> bool {
        self.get_definition(qualified_type)
            .is_some_and(|definition| definition.is_ufcs_method(method))
    }

    pub fn is_interpolatable(&self, ty: &Type) -> bool {
        let peeled = self.peel_alias(ty);
        let Type::Nominal { id, .. } = &peeled else {
            return true;
        };
        let Some(definition) = self.get_definition(id.as_str()) else {
            return true;
        };
        if !matches!(
            definition.body,
            DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. }
        ) {
            return true;
        }
        self.is_foreign_definition(definition, id.as_str())
            || self.has_stringer(definition, id.as_str())
    }

    fn is_foreign_definition(&self, definition: &Definition, qualified_name: &str) -> bool {
        if let Some(package) = self.package_for_qualified_name(qualified_name)
            && (package == "prelude" || package.starts_with("go:"))
        {
            return true;
        }
        definition
            .name_span
            .and_then(|span| self.get_file(span.file_id))
            .is_some_and(File::is_d_lis)
    }

    fn has_stringer(&self, definition: &Definition, qualified_name: &str) -> bool {
        if self.is_pointer_backed_newtype(definition) {
            return false;
        }
        if definition.is_display() {
            return true;
        }
        let Some(methods) = self.get_own_methods(qualified_name) else {
            return false;
        };
        ["string", "String"].iter().any(|name| {
            methods
                .get(*name)
                .is_some_and(|method| method.ty.is_stringer_signature(|ty| self.peel_alias(ty)))
                && !definition.is_ufcs_method(name)
        })
    }

    fn is_pointer_backed_newtype(&self, definition: &Definition) -> bool {
        definition.is_pointer_backed_newtype(|id| self.get_definition(id))
    }

    pub(crate) fn get_all_methods(&self, ty: &Type) -> Methods {
        methods_for_type(ty, &HashMap::default(), |id| self.get_definition(id))
    }

    pub(crate) fn get_method_for_type(&self, ty: &Type, name: &str) -> Option<Method> {
        method_for_type(ty, &HashMap::default(), |id| self.get_definition(id), name)
    }

    pub(crate) fn type_has_any_method(&self, ty: &Type) -> bool {
        type_has_any_method(ty, |id| self.get_definition(id))
    }
}

impl ConstantSource for Store {
    fn definition(&self, qualified_name: &str) -> Option<&Definition> {
        self.get_definition(qualified_name)
    }
}

#[cfg(test)]
mod clone_tests {
    use super::*;

    #[test]
    fn clone_has_an_independent_file_id_counter() {
        let mut store = Store::new();
        let mut cloned = store.clone();

        assert_eq!(store.new_file_id(), cloned.new_file_id());
    }

    #[test]
    fn clone_detaches_a_package_before_mutation() {
        let mut store = Store::new();
        store.add_package("m");
        let mut cloned = store.clone();

        cloned.store_file(File::new_cached("m", "cloned.lis", "", "", 42));

        assert!(store.get_file(42).is_none());
        assert!(!store.file_packages.contains_key(&42));
        assert_eq!(cloned.file_packages[&42], "m");
    }

    #[test]
    fn stored_files_are_indexed_without_a_package_threshold_or_dense_ids() {
        let mut store = Store::new();
        store.add_package("m");

        for file_id in [42, u32::MAX] {
            store.store_file(File::new_cached("m", "sample.lis", "", "", file_id));

            assert_eq!(store.file_packages[&file_id], "m");
            assert_eq!(store.get_file(file_id).unwrap().id, file_id);
            assert_eq!(store.get_file_mut(file_id).unwrap().id, file_id);
        }
        assert_eq!(store.file_packages.len(), 2);
    }

    #[test]
    fn registration_view_has_independent_file_ownership() {
        let mut store = Store::new();
        store.add_package("m");
        store.store_file(File::new_cached("m", "original.lis", "", "", 42));
        let mut view = store.registration_view();

        view.store_file(File::new_cached("m", "worker.lis", "", "", 43));

        assert_eq!(view.file_packages[&42], "m");
        assert_eq!(view.file_packages[&43], "m");
        assert_eq!(view.get_file(42).unwrap().name, "original.lis");
        assert_eq!(view.get_file(43).unwrap().name, "worker.lis");
        assert!(!store.file_packages.contains_key(&43));
        assert!(store.get_file(43).is_none());
    }

    #[test]
    fn prebuilt_package_files_are_available_by_id() {
        let mut store = Store::new();
        let mut package = Package::new("cached");
        package
            .files
            .insert(42, File::new_cached("cached", "cached.lis", "", "", 42));

        store.insert_prebuilt_package(package);

        assert_eq!(store.file_packages[&42], "cached");
        assert_eq!(
            store.get_file(42).map(|file| file.name.as_str()),
            Some("cached.lis")
        );
    }

    #[test]
    fn qualified_go_name_uses_longest_registered_package_prefix() {
        let mut store = Store::new();
        store.add_package("go:gopkg.in/yaml");
        store.add_package("go:gopkg.in/yaml.v3");

        assert_eq!(
            store.package_for_qualified_name("go:gopkg.in/yaml.v3.Node"),
            Some("go:gopkg.in/yaml.v3"),
        );
    }

    #[test]
    fn inserted_files_are_indexed() {
        let mut store = Store::new();
        store.add_package("large");

        store.store_file(File::new_cached("large", "large.lis", "", "", 42));

        assert_eq!(
            store.get_file(42).map(|file| file.name.as_str()),
            Some("large.lis")
        );
        assert_eq!(store.get_file_mut(42).unwrap().name, "large.lis");
    }

    #[test]
    fn moving_a_file_updates_both_owners() {
        let mut store = Store::new();
        store.add_package("before");
        store.add_package("after");
        store.store_file(File::new_cached("before", "before.lis", "", "", 42));

        store.store_file(File::new_cached("after", "after.lis", "", "", 42));
        assert!(store.get_package("before").unwrap().files.is_empty());

        assert_eq!(store.get_file(42).unwrap().package_id, "after");
        assert_eq!(store.get_file_mut(42).unwrap().package_id, "after");
    }

    #[test]
    fn mutating_a_file_detaches_only_its_package() {
        let mut store = Store::new();
        store.add_package("large");
        store.store_file(File::new_cached("large", "before.lis", "", "", 42));
        let cloned = store.clone();

        store.get_file_mut(42).unwrap().name = "after.lis".to_string();

        assert_eq!(
            store.get_file(42).map(|file| file.name.as_str()),
            Some("after.lis")
        );
        assert_eq!(cloned.get_file(42).unwrap().name, "before.lis");
        assert!(Arc::ptr_eq(
            &store.packages["prelude"],
            &cloned.packages["prelude"]
        ));
    }

    #[test]
    fn replacing_a_package_replaces_its_file_ownership() {
        let mut store = Store::new();
        store.add_package("cached");
        store.store_file(File::new_cached("cached", "before.lis", "", "", 42));
        let mut replacement = Package::new("cached");
        replacement
            .files
            .insert(43, File::new_cached("cached", "after.lis", "", "", 43));

        store.insert_prebuilt_package(replacement);

        assert!(!store.file_packages.contains_key(&42));
        assert_eq!(store.file_packages[&43], "cached");
        assert!(store.get_file(42).is_none());
        assert!(store.get_file_mut(42).is_none());
        assert_eq!(store.get_file(43).unwrap().name, "after.lis");
    }
}

#[cfg(test)]
mod closed_domain_tests {
    use super::*;
    use syntax::ast::{
        Annotation, Generic, Span, StructFieldDefinition, StructFieldKind, StructFields,
    };
    use syntax::program::{AliasKind, Attributes, TypeAttribute, Visibility};
    use syntax::program::{ConstantValue, ValueKind};
    use syntax::types::{CompoundKind, Symbol};

    fn nominal_int(id: &str) -> Type {
        Type::Nominal {
            id: Symbol::from_raw(id),
            params: vec![],
            writable: false,
        }
    }

    #[test]
    fn test_classification_is_derived_from_the_stored_file() {
        let mut store = Store::new();
        store.add_package("m");
        store.store_file(File::new_cached("m", "sample.test.lis", "", "", 42));

        assert!(store.is_test_file(42));

        store.store_file(File::new_cached("m", "sample.lis", "", "", 42));

        assert!(!store.is_test_file(42));
    }

    fn struct_def(ty: Type, closed_domain: bool) -> Definition {
        let mut attributes = Attributes::default();
        if closed_domain {
            attributes.insert(TypeAttribute::ClosedDomain);
        }
        Definition {
            visibility: Visibility::Public,
            ty,
            name_span: None,
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![],
                fields: StructFields::Tuple(vec![StructFieldDefinition {
                    doc: None,
                    name: "0".into(),
                    name_span: Span::dummy(),
                    annotation: Annotation::Unknown,
                    visibility: Visibility::Private,
                    ty: Type::Simple(SimpleKind::Int),
                    kind: StructFieldKind::Named { attributes: vec![] },
                }]),
                methods: Default::default(),
                attributes,
            },
        }
    }

    #[test]
    fn tuple_struct_constructor_is_derived_from_fields_and_type() {
        let definition = struct_def(nominal_int("m.Point"), false);

        let constructor = definition
            .constructor_type()
            .expect("tuple structs have constructors");
        let function = constructor
            .as_function_type()
            .expect("constructor should be callable");

        assert_eq!(function.params.len(), 1);
        assert_eq!(function.params[0].ty, Type::Simple(SimpleKind::Int));
        assert_eq!(function.return_type.as_ref(), &definition.ty);
    }

    fn int_const(ty: Type, value: u64) -> Definition {
        Definition {
            visibility: Visibility::Public,
            ty,
            name_span: None,
            doc: None,
            body: DefinitionBody::Value {
                kind: ValueKind::Constant(ConstantValue::Integer { value, text: None }),
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                go_type_param_recipe: None,
                go_type_terms: Vec::new(),
                superseded_by: None,
                impl_receiver: None,
            },
        }
    }

    fn insert(store: &mut Store, package: &str, name: &str, def: Definition) {
        store.add_package(package);
        store
            .edit_package(package)
            .unwrap()
            .definitions
            .insert(Symbol::from_raw(name), def);
    }

    #[test]
    fn tagged_type_with_members_is_derived_and_sorted() {
        let mut store = Store::new();
        let ty = nominal_int("m.Weekday");
        insert(&mut store, "m", "m.Weekday", struct_def(ty.clone(), true));
        insert(&mut store, "m", "m.Saturday", int_const(ty.clone(), 6));
        insert(&mut store, "m", "m.Sunday", int_const(ty.clone(), 0));

        let domain = store
            .closed_domain("m.Weekday")
            .expect("tagged type with members should have a derived domain");
        assert_eq!(domain.base(), SimpleKind::Int);
        assert_eq!(domain.type_display(), "Weekday");
        let names: Vec<&str> = domain
            .members()
            .iter()
            .map(ClosedMember::display_name)
            .collect();
        assert_eq!(names, vec!["Sunday", "Saturday"]);
    }

    #[test]
    fn untagged_type_is_absent() {
        let mut store = Store::new();
        let ty = nominal_int("m.Plain");
        insert(&mut store, "m", "m.Plain", struct_def(ty.clone(), false));
        insert(&mut store, "m", "m.One", int_const(ty, 1));

        assert!(store.closed_domain("m.Plain").is_none());
    }

    #[test]
    fn tagged_type_without_members_records_no_domain() {
        let mut store = Store::new();
        insert(
            &mut store,
            "m",
            "m.Empty",
            struct_def(nominal_int("m.Empty"), true),
        );

        assert!(store.closed_domain("m.Empty").is_none());
    }

    #[test]
    fn const_in_other_package_does_not_widen_domain() {
        let mut store = Store::new();
        let ty = nominal_int("lib.Weekday");
        insert(
            &mut store,
            "lib",
            "lib.Weekday",
            struct_def(ty.clone(), true),
        );
        insert(&mut store, "lib", "lib.Sunday", int_const(ty.clone(), 0));
        insert(&mut store, "user", "user.Bad", int_const(ty, 99));

        let domain = store.closed_domain("lib.Weekday").unwrap();
        let names: Vec<&str> = domain
            .members()
            .iter()
            .map(ClosedMember::display_name)
            .collect();
        assert_eq!(names, vec!["Sunday"]);
    }

    #[test]
    fn generic_alias_target_is_instantiated_from_its_definition() {
        let mut store = Store::new();
        let generic = Generic::new("T", Vec::new(), Span::dummy());
        let alias_ref = Type::Nominal {
            id: Symbol::from_raw("m.Items"),
            params: vec![Type::Parameter("T".into())],
            writable: false,
        };
        insert(
            &mut store,
            "m",
            "m.Items",
            Definition {
                visibility: Visibility::Public,
                ty: Type::Forall {
                    vars: vec!["T".into()],
                    body: Box::new(alias_ref),
                },
                name_span: None,
                doc: None,
                body: DefinitionBody::TypeAlias {
                    generics: vec![generic],
                    alias: AliasKind::Transparent {
                        annotation: Annotation::Unknown,
                        target: Type::Compound {
                            kind: CompoundKind::Slice,
                            args: vec![Type::Parameter("T".into())],
                            writable: false,
                        },
                    },
                    methods: Default::default(),
                    attributes: Default::default(),
                },
            },
        );

        let occurrence = Type::Nominal {
            id: Symbol::from_raw("m.Items"),
            params: vec![Type::int()],
            writable: false,
        };
        let expected = Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![Type::int()],
            writable: false,
        };

        assert_eq!(store.underlying_type(&occurrence), Some(expected.clone()));
        assert_eq!(store.peel_alias(&occurrence), expected);
    }

    #[test]
    fn alias_peeling_transfers_the_occurrence_qualifier() {
        let mut store = Store::new();
        insert(
            &mut store,
            "m",
            "m.Bytes",
            Definition {
                visibility: Visibility::Public,
                ty: Type::Nominal {
                    id: Symbol::from_raw("m.Bytes"),
                    params: vec![],
                    writable: false,
                },
                name_span: None,
                doc: None,
                body: DefinitionBody::TypeAlias {
                    generics: vec![],
                    alias: AliasKind::Transparent {
                        annotation: Annotation::Unknown,
                        target: Type::compound(CompoundKind::Slice, vec![Type::int()]),
                    },
                    methods: Default::default(),
                    attributes: Default::default(),
                },
            },
        );

        let writable_occurrence = Type::Nominal {
            id: Symbol::from_raw("m.Bytes"),
            params: vec![],
            writable: true,
        };
        let expected = Type::qualified_compound(CompoundKind::Slice, vec![Type::int()], true);
        assert_eq!(store.peel_alias(&writable_occurrence), expected);
        assert_eq!(store.underlying_type(&writable_occurrence), Some(expected));

        let plain_occurrence = Type::Nominal {
            id: Symbol::from_raw("m.Bytes"),
            params: vec![],
            writable: false,
        };
        assert!(!store.peel_alias(&plain_occurrence).is_writable());
    }

    #[test]
    fn newtype_underlying_applies_the_projection_meet() {
        fn newtype_wrapping(name: &str, field_ty: Type) -> Definition {
            Definition {
                visibility: Visibility::Public,
                ty: nominal_int(name),
                name_span: None,
                doc: None,
                body: DefinitionBody::Struct {
                    generics: vec![],
                    fields: StructFields::Tuple(vec![StructFieldDefinition {
                        doc: None,
                        name: "0".into(),
                        name_span: Span::dummy(),
                        annotation: Annotation::Unknown,
                        visibility: Visibility::Private,
                        ty: field_ty,
                        kind: StructFieldKind::Named { attributes: vec![] },
                    }]),
                    methods: Default::default(),
                    attributes: Default::default(),
                },
            }
        }

        let writable_slice = Type::qualified_compound(CompoundKind::Slice, vec![Type::int()], true);
        let plain_slice = Type::compound(CompoundKind::Slice, vec![Type::int()]);

        let mut store = Store::new();
        insert(
            &mut store,
            "m",
            "m.Writable",
            newtype_wrapping("m.Writable", writable_slice.clone()),
        );
        insert(
            &mut store,
            "m",
            "m.Plain",
            newtype_wrapping("m.Plain", plain_slice.clone()),
        );

        let occurrence = |id: &str, writable: bool| Type::Nominal {
            id: Symbol::from_raw(id),
            params: vec![],
            writable,
        };

        // Writable owner exposes the field as declared.
        assert_eq!(
            store.peel_underlying(&occurrence("m.Writable", true)),
            writable_slice,
        );
        // Read-only owner demotes a writable field.
        assert_eq!(
            store.peel_underlying(&occurrence("m.Writable", false)),
            plain_slice,
        );
        // Writable owner never promotes a read-only field.
        assert_eq!(
            store.peel_underlying(&occurrence("m.Plain", true)),
            plain_slice,
        );
    }

    fn transparent_alias(name: &str, target: Type) -> Definition {
        Definition {
            visibility: Visibility::Public,
            ty: nominal_int(name),
            name_span: None,
            doc: None,
            body: DefinitionBody::TypeAlias {
                generics: vec![],
                alias: AliasKind::Transparent {
                    annotation: Annotation::Unknown,
                    target,
                },
                methods: Default::default(),
                attributes: Default::default(),
            },
        }
    }

    #[test]
    fn newtype_underlying_demotes_permission_hidden_by_an_alias() {
        let writable_slice = Type::qualified_compound(CompoundKind::Slice, vec![Type::int()], true);
        let plain_slice = Type::compound(CompoundKind::Slice, vec![Type::int()]);

        let mut store = Store::new();
        insert(
            &mut store,
            "m",
            "m.Items",
            transparent_alias("m.Items", writable_slice.clone()),
        );
        insert(
            &mut store,
            "m",
            "m.Wrapper",
            Definition {
                visibility: Visibility::Public,
                ty: nominal_int("m.Wrapper"),
                name_span: None,
                doc: None,
                body: DefinitionBody::Struct {
                    generics: vec![],
                    fields: StructFields::Tuple(vec![StructFieldDefinition {
                        doc: None,
                        name: "0".into(),
                        name_span: Span::dummy(),
                        annotation: Annotation::Unknown,
                        visibility: Visibility::Private,
                        ty: nominal_int("m.Items"),
                        kind: StructFieldKind::Named { attributes: vec![] },
                    }]),
                    methods: Default::default(),
                    attributes: Default::default(),
                },
            },
        );

        let occurrence = |writable: bool| Type::Nominal {
            id: Symbol::from_raw("m.Wrapper"),
            params: vec![],
            writable,
        };

        assert_eq!(store.peel_underlying(&occurrence(false)), plain_slice);
        assert_eq!(store.peel_underlying(&occurrence(true)), writable_slice);
    }

    #[test]
    fn binding_demotion_stays_shallow_behind_an_alias() {
        let writable_slice = Type::qualified_compound(CompoundKind::Slice, vec![Type::int()], true);
        let plain_slice = Type::compound(CompoundKind::Slice, vec![Type::int()]);

        let mut store = Store::new();
        insert(
            &mut store,
            "m",
            "m.Pair",
            transparent_alias(
                "m.Pair",
                Type::Tuple(vec![writable_slice.clone(), Type::int()]),
            ),
        );
        insert(
            &mut store,
            "m",
            "m.Rows",
            transparent_alias("m.Rows", writable_slice),
        );

        assert_eq!(
            store.demoted_at_binding(&nominal_int("m.Pair")),
            nominal_int("m.Pair"),
        );
        assert_eq!(
            store.demoted_at_binding(&nominal_int("m.Rows")),
            plain_slice,
        );
    }

    #[test]
    fn generic_alias_peeling_keeps_writable_type_arguments() {
        let mut store = Store::new();
        let generic = Generic::new("T", Vec::new(), Span::dummy());
        let alias_ref = Type::Nominal {
            id: Symbol::from_raw("m.Rows"),
            params: vec![Type::Parameter("T".into())],
            writable: false,
        };
        insert(
            &mut store,
            "m",
            "m.Rows",
            Definition {
                visibility: Visibility::Public,
                ty: Type::Forall {
                    vars: vec!["T".into()],
                    body: Box::new(alias_ref),
                },
                name_span: None,
                doc: None,
                body: DefinitionBody::TypeAlias {
                    generics: vec![generic],
                    alias: AliasKind::Transparent {
                        annotation: Annotation::Unknown,
                        target: Type::compound(
                            CompoundKind::Slice,
                            vec![Type::Parameter("T".into())],
                        ),
                    },
                    methods: Default::default(),
                    attributes: Default::default(),
                },
            },
        );

        let writable_arg = Type::qualified_compound(CompoundKind::Slice, vec![Type::int()], true);
        let occurrence = Type::Nominal {
            id: Symbol::from_raw("m.Rows"),
            params: vec![writable_arg.clone()],
            writable: true,
        };
        let expected = Type::qualified_compound(CompoundKind::Slice, vec![writable_arg], true);
        assert_eq!(store.peel_alias(&occurrence), expected);
    }

    #[test]
    fn newtype_representation_comes_from_its_definition() {
        let mut store = Store::new();
        let occurrence = nominal_int("m.Count");
        insert(
            &mut store,
            "m",
            "m.Count",
            struct_def(occurrence.clone(), false),
        );

        assert_eq!(store.peel_underlying(&occurrence), Type::int());
    }

    #[test]
    fn peeling_references_and_aliases_terminates_on_recursive_aliases() {
        fn recursive_alias(name: &str, target: &str) -> Definition {
            Definition {
                visibility: Visibility::Public,
                ty: nominal_int(name),
                name_span: None,
                doc: None,
                body: DefinitionBody::TypeAlias {
                    generics: vec![],
                    alias: AliasKind::Transparent {
                        annotation: Annotation::Unknown,
                        target: Type::Compound {
                            kind: CompoundKind::Ref,
                            args: vec![nominal_int(target)],
                            writable: false,
                        },
                    },
                    methods: Default::default(),
                    attributes: Default::default(),
                },
            }
        }

        let mut store = Store::new();
        insert(&mut store, "m", "m.A", recursive_alias("m.A", "m.B"));
        insert(&mut store, "m", "m.B", recursive_alias("m.B", "m.A"));

        let (peeled, behind_ref) = store.peel_refs_and_aliases(&nominal_int("m.A"));

        assert!(behind_ref);
        assert_eq!(peeled, nominal_int("m.B"));
    }
    #[test]
    fn annotation_normalization_reaches_array_elements() {
        let mut store = Store::new();
        let nominal = nominal_int("m.Count");
        insert(
            &mut store,
            "m",
            "m.Count",
            struct_def(nominal.clone(), false),
        );
        let input = Type::Array {
            length: 2,
            element: Box::new(nominal.clone().make_writable()),
        };
        let expected = Type::Array {
            length: 2,
            element: Box::new(nominal),
        };
        assert_eq!(store.normalized_annotation_type(&input), expected);
    }
}
