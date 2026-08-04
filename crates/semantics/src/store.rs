use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{EnumVariant, Expression, StructFieldDefinition};
use syntax::program::{
    Definition, DefinitionBody, EqualityIndex, File, Interface, MethodSignatures, Module,
    TestIndex, UninferredExports,
};
use syntax::types::{SimpleKind, SubstitutionMap, Symbol, Type, substitute};

pub use crate::closed_domain::{ClosedDomain, ClosedMember, DomainValue};
pub use syntax::ENTRY_MODULE_ID;
pub(crate) const ENTRY_FILE_ID: u32 = 0;

pub struct Store {
    /// `Arc` so registration workers share a read view; [`Arc::make_mut`]
    /// writes stay zero-copy while a module has a single owner.
    pub modules: HashMap<String, Arc<Module>>,
    /// Go module ID -> package name from the typedef `// Package:` directive.
    pub go_package_names: HashMap<String, String>,
    /// File ID counter. Starts at 2 because 0 is reserved for entry, 1 for prelude.
    next_file_id: AtomicU32,
    pub equality_index: EqualityIndex,
    pub test_index: TestIndex,
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            modules: self.modules.clone(),
            go_package_names: self.go_package_names.clone(),
            next_file_id: AtomicU32::new(self.next_file_id.load(Ordering::Relaxed)),
            equality_index: self.equality_index.clone(),
            test_index: self.test_index.clone(),
        }
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        let prelude_module = Module::new("prelude");
        let nominal_module = Module::nominal();

        let modules = vec![
            (prelude_module.id.clone(), Arc::new(prelude_module)),
            (nominal_module.id.clone(), Arc::new(nominal_module)),
        ]
        .into_iter()
        .collect();

        Self {
            modules,
            go_package_names: Default::default(),
            next_file_id: AtomicU32::new(2), // 0 = entrypoint, 1 = prelude
            equality_index: Default::default(),
            test_index: Default::default(),
        }
    }

    pub fn new_file_id(&self) -> u32 {
        self.next_file_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn reserve_file_ids(&self, count: u32) -> u32 {
        self.next_file_id.fetch_add(count, Ordering::Relaxed)
    }

    pub(crate) fn entry_module_id(&self) -> &'static str {
        ENTRY_MODULE_ID
    }

    /// Creates the entry module (empty for a library, whose root files load as siblings).
    pub(crate) fn init_entry_module(&mut self) {
        self.add_module(ENTRY_MODULE_ID);
    }

    pub(crate) fn store_entry_file(
        &mut self,
        filename: &str,
        display_path: &str,
        source: &str,
        ast: Vec<Expression>,
        file_comment: Option<String>,
    ) {
        self.store_file(File {
            id: ENTRY_FILE_ID,
            module_id: ENTRY_MODULE_ID.to_string(),
            name: filename.to_string(),
            display_path: display_path.to_string(),
            source_path: None,
            source: source.to_string(),
            items: ast,
            file_comment,
        });
    }

    pub fn store_module(&mut self, module_id: &str, files: Vec<File>) {
        self.add_module(module_id);

        for file in files {
            assert_eq!(file.module_id, module_id);
            self.store_file(file);
        }
    }

    pub(crate) fn store_uninferred_module(
        &mut self,
        module_id: &str,
        files: Vec<File>,
        exports: UninferredExports,
    ) {
        self.store_module(module_id, files);
        if let Some(module) = self.get_module_mut(module_id) {
            module.uninferred_exports = Some(exports);
        }
    }

    /// Stores a file in its owning module.
    pub fn store_file(&mut self, file: File) {
        let module_id = file.module_id.clone();

        let module = self
            .get_module_mut(&module_id)
            .expect("module must exist to store file");
        module.files.insert(file.id, file);
    }

    pub fn get_file(&self, file_id: u32) -> Option<&File> {
        self.modules
            .values()
            .find_map(|module| module.get_file(file_id))
    }

    pub(crate) fn get_file_mut(&mut self, file_id: u32) -> Option<&mut File> {
        let module_id = self.modules.iter().find_map(|(module_id, module)| {
            module
                .files
                .contains_key(&file_id)
                .then(|| module_id.clone())
        })?;
        let module = Arc::make_mut(self.modules.get_mut(&module_id)?);
        module.files.get_mut(&file_id)
    }

    pub fn get_module(&self, module_id: &str) -> Option<&Module> {
        self.modules.get(module_id).map(Arc::as_ref)
    }

    pub(crate) fn has(&self, module_id: &str) -> bool {
        self.modules.contains_key(module_id)
    }

    pub(crate) fn uninferred_module_may_export(&self, module_id: &str, member: &str) -> bool {
        self.modules
            .get(module_id)
            .and_then(|module| module.uninferred_exports.as_ref())
            .is_some_and(|exports| exports.may_contain(member))
    }

    pub fn add_module(&mut self, module_id: &str) {
        if self.modules.contains_key(module_id) {
            return;
        }

        self.modules
            .insert(module_id.to_string(), Arc::new(Module::new(module_id)));
    }

    pub fn get_module_mut(&mut self, module_id: &str) -> Option<&mut Module> {
        self.modules.get_mut(module_id).map(Arc::make_mut)
    }

    /// Inserts a worker-built module (e.g. cache-decoded).
    pub(crate) fn insert_prebuilt_module(&mut self, module: Module) {
        self.modules.insert(module.id.clone(), Arc::new(module));
    }

    /// `Arc`-bump snapshot for a registration worker, which inserts its own
    /// detached module before use.
    pub(crate) fn registration_view(&self) -> Store {
        Store {
            modules: self.modules.clone(),
            go_package_names: self.go_package_names.clone(),
            next_file_id: AtomicU32::new(self.next_file_id.load(Ordering::Relaxed)),
            equality_index: EqualityIndex::default(),
            test_index: TestIndex::default(),
        }
    }

    pub fn get_definition(&self, qualified_name: &str) -> Option<&Definition> {
        let module_name = self.module_for_qualified_name(qualified_name)?;

        self.get_module(module_name)?
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

    pub fn module_for_qualified_name<'a>(&'a self, qualified_name: &'a str) -> Option<&'a str> {
        syntax::types::module_for_qualified_name(
            qualified_name,
            self.modules.keys().map(String::as_str),
        )
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
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { fields, .. } => Some(fields.as_slice()),
            _ => None,
        }
    }

    pub fn struct_kind(&self, qualified_name: &str) -> Option<syntax::ast::StructKind> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { fields, .. } => Some(fields.kind()),
            _ => None,
        }
    }

    pub fn deep_struct_kind(&self, ty: &Type) -> Option<syntax::ast::StructKind> {
        self.struct_kind(self.deep_resolve_alias(ty).get_qualified_id()?)
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

    pub(crate) fn is_interface(&self, ty: &Type) -> bool {
        matches!(ty, Type::Nominal { id, .. } if self.get_interface(id.as_str()).is_some())
    }

    pub fn is_nilable_go_type(&self, ty: &Type) -> bool {
        syntax::types::is_nilable_go_type(ty, |id| self.get_definition(id))
    }

    pub fn peel_alias(&self, ty: &Type) -> Type {
        syntax::types::peel_alias(ty, |id| self.get_definition(id))
    }

    pub fn underlying_type(&self, ty: &Type) -> Option<Type> {
        syntax::types::underlying_type(ty, |id| self.get_definition(id))
    }

    pub fn peel_underlying(&self, ty: &Type) -> Type {
        syntax::types::peel_underlying(ty, |id| self.get_definition(id))
    }

    pub fn underlying_simple_kind(&self, ty: &Type) -> Option<SimpleKind> {
        syntax::types::underlying_simple_kind(ty, |id| self.get_definition(id))
    }

    pub fn underlying_numeric_type(&self, ty: &Type) -> Option<Type> {
        syntax::types::underlying_numeric_type(ty, |id| self.get_definition(id))
    }

    pub fn literal_adaptation_target(&self, ty: &Type) -> Option<Type> {
        syntax::types::literal_adaptation_target(ty, |id| self.get_definition(id))
    }

    pub fn is_numeric_compatible_with(&self, left: &Type, right: &Type) -> bool {
        syntax::types::is_numeric_compatible_with(left, right, |id| self.get_definition(id))
    }

    pub fn is_aliased_numeric_type(&self, ty: &Type) -> bool {
        syntax::types::is_aliased_numeric_type(ty, |id| self.get_definition(id))
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
        syntax::types::has_byte_or_rune_slice_underlying(ty, |id| self.get_definition(id))
    }

    pub fn is_orderable(&self, ty: &Type) -> bool {
        syntax::types::is_orderable(ty, |id| self.get_definition(id))
    }

    pub fn satisfies_ordered_constraint(&self, ty: &Type) -> bool {
        syntax::types::satisfies_ordered_constraint(ty, |id| self.get_definition(id))
    }

    pub fn resolves_to_unknown(&self, ty: &Type) -> bool {
        syntax::types::resolves_to_unknown(ty, |id| self.get_definition(id))
    }

    pub fn contains_unknown(&self, ty: &Type) -> bool {
        syntax::types::contains_unknown(ty, |id| self.get_definition(id))
    }

    pub fn resolve_to_function_type(&self, ty: &Type) -> Option<Type> {
        let resolved = self.peel_alias(ty);
        if matches!(resolved, Type::Function(_)) {
            return Some(resolved);
        }
        self.underlying_type(ty)
            .filter(|underlying| matches!(underlying, Type::Function(_)))
    }

    pub fn peel_refs_and_aliases(&self, ty: &Type) -> (Type, bool) {
        let mut current = self.peel_alias(ty);
        let mut behind_ref = false;
        let mut seen = HashSet::default();
        while current.is_ref() {
            behind_ref = true;
            let stripped = current.strip_refs();
            if let Type::Nominal { id, .. } = stripped.unwrap_forall()
                && !seen.insert(id.clone())
            {
                return (stripped, behind_ref);
            }
            current = self.peel_alias(&stripped);
        }
        (current, behind_ref)
    }

    pub fn deep_resolve_alias(&self, ty: &Type) -> Type {
        self.peel_alias(ty)
    }

    pub(crate) fn peel_alias_deep(&self, ty: &Type) -> Type {
        match self.peel_alias(ty) {
            Type::Compound { kind, args } => Type::Compound {
                kind,
                args: args.iter().map(|a| self.peel_alias_deep(a)).collect(),
            },
            Type::Tuple(elements) => {
                Type::Tuple(elements.iter().map(|e| self.peel_alias_deep(e)).collect())
            }
            Type::Array { length, element } => Type::Array {
                length,
                element: Box::new(self.peel_alias_deep(&element)),
            },
            Type::Nominal { id, params } => Type::Nominal {
                id,
                params: params.iter().map(|p| self.peel_alias_deep(p)).collect(),
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

    pub fn get_own_methods(&self, qualified_name: &str) -> Option<&MethodSignatures> {
        self.get_definition(qualified_name)?.methods()
    }

    pub fn is_ufcs_method(&self, qualified_type: &str, method: &str) -> bool {
        self.get_definition(qualified_type)
            .is_some_and(|definition| definition.is_ufcs_method(method))
    }

    pub(crate) fn get_all_methods(
        &self,
        ty: &Type,
        trait_bounds: &HashMap<Symbol, Vec<Type>>,
    ) -> MethodSignatures {
        let mut visited = HashSet::default();
        self.get_all_methods_recursive(ty, trait_bounds, &mut visited)
    }

    fn get_all_methods_recursive(
        &self,
        ty: &Type,
        trait_bounds: &HashMap<Symbol, Vec<Type>>,
        visited: &mut HashSet<String>,
    ) -> MethodSignatures {
        let stripped = ty.strip_refs();
        let Some(qualified_name) = method_lookup_key(&stripped) else {
            return MethodSignatures::default();
        };

        // Cyclic embeddings survive registration as an error with parents intact; guard the walk.
        if !visited.insert(qualified_name.as_str().to_string()) {
            return MethodSignatures::default();
        }

        if let Some(interface) = self.get_interface(&qualified_name) {
            let mut all_interface_methods = MethodSignatures::default();

            let type_args = ty.get_type_params().unwrap_or_default();
            let map: SubstitutionMap = interface
                .generics
                .iter()
                .map(|g| g.name.clone())
                .zip(type_args.iter().cloned())
                .collect();

            for (name, method_ty) in &interface.methods {
                let substituted = substitute(method_ty, &map);
                all_interface_methods.insert(name.clone(), substituted.with_receiver_placeholder());
            }

            for parent in &interface.parents {
                for (name, method_ty) in
                    self.get_all_methods_recursive(parent, trait_bounds, visited)
                {
                    all_interface_methods.insert(name, method_ty);
                }
            }

            return all_interface_methods;
        }

        if let Some(bound_types) = trait_bounds.get(&qualified_name) {
            return bound_types
                .iter()
                .flat_map(|interface_ty| {
                    self.get_all_methods_recursive(interface_ty, trait_bounds, visited)
                })
                .collect();
        }

        let mut methods = self
            .get_own_methods(&qualified_name)
            .cloned()
            .unwrap_or_default();

        // Type aliases inherit methods from the underlying type.
        if let Some(definition) = self.get_definition(&qualified_name)
            && matches!(definition.body, DefinitionBody::TypeAlias { .. })
        {
            let underlying = self.peel_alias(&stripped);
            if underlying != stripped {
                for (name, method_ty) in
                    self.get_all_methods_recursive(&underlying, trait_bounds, visited)
                {
                    methods.entry(name).or_insert(method_ty);
                }
            }
        }

        methods
    }

    pub(crate) fn get_methods_from_bounds(
        &self,
        qualified_name: &str,
        trait_bounds: &HashMap<Symbol, Vec<Type>>,
    ) -> MethodSignatures {
        if let Some(bound_types) = trait_bounds.get(qualified_name) {
            return bound_types
                .iter()
                .flat_map(|interface_ty| self.get_all_methods(interface_ty, trait_bounds))
                .collect();
        }
        MethodSignatures::default()
    }
}

/// Return the qualified name used to look up methods/fields for a given type.
/// For `Type::Compound` and `Type::Simple`, this is the prelude-qualified name
/// (e.g. `Type::Compound { Slice, .. }` → `"prelude.Slice"`).
fn method_lookup_key(ty: &Type) -> Option<Symbol> {
    match ty {
        Type::Nominal { id, .. } => Some(id.clone()),
        Type::Compound { kind, .. } => Some(Symbol::from_parts("prelude", kind.leaf_name())),
        Type::Simple(kind) => Some(Symbol::from_parts("prelude", kind.leaf_name())),
        // Array methods live on the prelude `Array` impl.
        Type::Array { .. } => Some(Symbol::from_parts("prelude", "Array")),
        _ => None,
    }
}

#[cfg(test)]
mod clone_tests {
    use super::*;

    #[test]
    fn clone_has_an_independent_file_id_counter() {
        let store = Store::new();
        let cloned = store.clone();

        assert_eq!(store.new_file_id(), cloned.new_file_id());
    }

    #[test]
    fn clone_detaches_a_module_before_mutation() {
        let mut store = Store::new();
        store.add_module("m");
        let mut cloned = store.clone();

        cloned.store_file(File::new_cached("m", "cloned.lis", "", "", 42));

        assert!(store.get_file(42).is_none());
    }
}

#[cfg(test)]
mod closed_domain_tests {
    use super::*;
    use syntax::ast::{
        Annotation, Generic, Literal, Span, StructFieldDefinition, StructFieldKind, StructFields,
    };
    use syntax::program::{AliasKind, Attributes, TypeAttribute, Visibility};
    use syntax::types::CompoundKind;

    fn nominal_int(id: &str) -> Type {
        Type::Nominal {
            id: Symbol::from_raw(id),
            params: vec![],
        }
    }

    #[test]
    fn test_classification_is_derived_from_the_stored_file() {
        let mut store = Store::new();
        store.add_module("m");
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
                    name_span: syntax::ast::Span::dummy(),
                    annotation: syntax::ast::Annotation::Unknown,
                    visibility: syntax::ast::Visibility::Private,
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
                kind: syntax::program::ValueKind::Constant(Literal::Integer { value, text: None }),
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                go_type_param_recipe: None,
            },
        }
    }

    fn insert(store: &mut Store, module: &str, name: &str, def: Definition) {
        store.add_module(module);
        store
            .get_module_mut(module)
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
    fn const_in_other_module_does_not_widen_domain() {
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
        };
        let expected = Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![Type::int()],
        };

        assert_eq!(store.underlying_type(&occurrence), Some(expected.clone()));
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
}
