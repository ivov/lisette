use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{EnumVariant, Expression, Literal, StructFieldDefinition};
use syntax::program::{
    Definition, DefinitionBody, EqualityIndex, File, Interface, MethodSignatures, Module, TestIndex,
};
use syntax::types::{SimpleKind, SubstitutionMap, Symbol, Type, substitute};

pub use syntax::ENTRY_MODULE_ID;
const ENTRY_FILE_ID: u32 = 0;

#[derive(Debug, Clone)]
pub struct ClosedMember {
    /// Qualified the way the user writes it (e.g. `time.Sunday`), for the diagnostic.
    pub display_name: EcoString,
    /// The member's source literal, for rendering the valid-set hint.
    literal: Literal,
}

impl ClosedMember {
    pub fn literal(&self) -> &Literal {
        &self.literal
    }

    pub fn value(&self, base: SimpleKind) -> DomainValue {
        DomainValue::from_literal(&self.literal, base)
            .expect("closed-domain members have a literal compatible with their base")
    }
}

/// The curated valid-value set of a `#[go(closed_domain)]` named primitive.
#[derive(Debug, Clone)]
pub struct ClosedDomain {
    pub base: SimpleKind,
    pub type_display: EcoString,
    pub members: Vec<ClosedMember>,
}

/// A literal reduced to its comparable form for a closed domain's base kind.
/// Float bases are not indexed, so only integers (signed `i128`) and strings occur.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DomainValue {
    Int(i128),
    Str(String),
}

impl DomainValue {
    pub fn from_literal(literal: &Literal, base: SimpleKind) -> Option<DomainValue> {
        // `rune` is a signed integer kind, so handle it before the integer arm
        // to accept char literals as codepoints. A negative const is stored as
        // its two's-complement `u64`, so signed bases reinterpret it as `i64`.
        match base {
            SimpleKind::Rune => match literal {
                Literal::Char(text) => char_codepoint(text).map(|cp| DomainValue::Int(cp as i128)),
                Literal::Integer { value, .. } => Some(DomainValue::Int(*value as i64 as i128)),
                _ => None,
            },
            SimpleKind::String => match literal {
                Literal::String { value, .. } => Some(DomainValue::Str(value.clone())),
                _ => None,
            },
            _ if is_unsigned_base(base) => match literal {
                Literal::Integer { value, .. } => Some(DomainValue::Int(*value as i128)),
                _ => None,
            },
            _ if base.is_signed_int() => match literal {
                Literal::Integer { value, .. } => Some(DomainValue::Int(*value as i64 as i128)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// `uintptr` is an unsigned integer for value purposes but is excluded from
/// `SimpleKind::is_unsigned_int`, so it is folded in here.
fn is_unsigned_base(base: SimpleKind) -> bool {
    base.is_unsigned_int() || base == SimpleKind::Uintptr
}

/// Decodes a rune literal's inner text to a codepoint, covering the escapes the
/// lexer accepts (`\a \b \f \n \r \t \v \\ \'`, `\x` hex, and octal `\NNN`).
fn char_codepoint(text: &str) -> Option<u64> {
    let Some(rest) = text.strip_prefix('\\') else {
        return text.chars().next().map(|c| c as u64);
    };
    match rest.as_bytes().first()? {
        b'a' => Some(7),
        b'b' => Some(8),
        b'f' => Some(12),
        b'n' => Some(10),
        b'r' => Some(13),
        b't' => Some(9),
        b'v' => Some(11),
        b'\\' => Some(92),
        b'\'' => Some(39),
        b'x' => u64::from_str_radix(&rest[1..], 16).ok(),
        b'0'..=b'7' => u64::from_str_radix(rest, 8).ok(),
        _ => None,
    }
}

pub struct Store {
    /// `Arc` so registration workers share a read view; [`Arc::make_mut`]
    /// writes stay zero-copy while a module has a single owner.
    pub modules: HashMap<String, Arc<Module>>,
    /// Go module ID -> package name from the typedef `// Package:` directive.
    pub go_package_names: HashMap<String, String>,
    /// File ID -> on-disk path of the `.d.lis` typedef. Lets the LSP map go: typedef
    /// file IDs to the actual cache path so go-to-definition can navigate there.
    pub typedef_paths: HashMap<u32, PathBuf>,
    visited_modules: HashSet<String>,
    /// File ID counter. Starts at 2 because 0 is reserved for entry, 1 for prelude.
    next_file_id: AtomicU32,
    /// Closed-domain index, keyed by the type's qualified name (the `id` in
    /// `Type::Nominal`). Built once after registration by `build_closed_domains`.
    pub closed_domains: HashMap<Symbol, ClosedDomain>,
    pub equality_index: EqualityIndex,
    pub test_index: TestIndex,
    /// File IDs of `.test.lis` files, for detecting test-file context during
    /// inference after a module's `files` have been taken out.
    test_file_ids: HashSet<u32>,
    /// Read during inference to gate the binary-only `main` signature check.
    pub(crate) project_kind: crate::inference::ProjectKind,
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
            typedef_paths: Default::default(),
            visited_modules: Default::default(),
            next_file_id: AtomicU32::new(2), // 0 = entrypoint, 1 = prelude
            closed_domains: Default::default(),
            equality_index: Default::default(),
            test_index: Default::default(),
            test_file_ids: Default::default(),
            project_kind: Default::default(),
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
            source: source.to_string(),
            items: ast,
            file_comment,
        });
    }

    pub fn store_module(&mut self, module_id: &str, files: Vec<File>) {
        self.mark_visited(module_id);
        self.add_module(module_id);

        for file in files {
            assert_eq!(file.module_id, module_id);
            self.store_file(file);
        }
    }

    /// Stores a file in its owning module.
    pub fn store_file(&mut self, file: File) {
        let module_id = file.module_id.clone();
        self.update_test_file_classification(&file);

        let module = self
            .get_module_mut(&module_id)
            .expect("module must exist to store file");
        module.files.insert(file.id, file);
    }

    fn update_test_file_classification(&mut self, file: &File) {
        if file.is_test() {
            self.test_file_ids.insert(file.id);
        } else {
            self.test_file_ids.remove(&file.id);
        }
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
        let module_id = module.id.clone();
        if let Some(previous) = self.modules.remove(&module_id) {
            for file_id in previous.files.keys() {
                self.test_file_ids.remove(file_id);
            }
        }
        for file in module.files.values() {
            self.update_test_file_classification(file);
        }
        self.modules.insert(module_id.clone(), Arc::new(module));
        self.visited_modules.insert(module_id);
    }

    /// `Arc`-bump snapshot for a registration worker, which inserts its own
    /// detached module before use.
    pub(crate) fn registration_view(&self) -> Store {
        Store {
            modules: self.modules.clone(),
            go_package_names: self.go_package_names.clone(),
            typedef_paths: HashMap::default(),
            visited_modules: HashSet::default(),
            next_file_id: AtomicU32::new(self.next_file_id.load(Ordering::Relaxed)),
            closed_domains: HashMap::default(),
            equality_index: EqualityIndex::default(),
            test_index: TestIndex::default(),
            test_file_ids: self.test_file_ids.clone(),
            project_kind: self.project_kind,
        }
    }

    pub fn is_visited(&self, module_id: &str) -> bool {
        self.visited_modules.contains(module_id)
    }

    pub(crate) fn mark_visited(&mut self, module_id: &str) {
        self.visited_modules.insert(module_id.to_string());
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
            .is_some_and(|span| self.test_file_ids.contains(&span.file_id))
    }

    pub(crate) fn is_test_file(&self, file_id: u32) -> bool {
        self.test_file_ids.contains(&file_id)
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

    pub fn build_closed_domains(&mut self) {
        // type id -> (base kind, id of the module that declares the type)
        let mut bases: HashMap<Symbol, (SimpleKind, String)> = HashMap::default();
        for module in self.modules.values() {
            for (qualified_name, definition) in &module.definitions {
                // Float domains rely on exact-equality over fragile values and do
                // not occur in the Go stdlib; they are deliberately not indexed.
                if definition.is_closed_domain()
                    && let Some(base) = self.underlying_simple_kind(&definition.ty)
                    && !base.is_float()
                {
                    bases.insert(qualified_name.clone(), (base, module.id.clone()));
                }
            }
        }

        if bases.is_empty() {
            return;
        }

        let mut members: HashMap<Symbol, Vec<ClosedMember>> = HashMap::default();
        for module in self.modules.values() {
            for (qualified_name, definition) in &module.definitions {
                let Some(const_literal) = definition.const_value() else {
                    continue;
                };
                let Type::Nominal { id, .. } = &definition.ty else {
                    continue;
                };
                let Some((base, declaring_module)) = bases.get(id) else {
                    continue;
                };
                // Only consts declared alongside the type extend its domain; a
                // const of an imported closed type in user code must not widen it.
                if module.id != *declaring_module {
                    continue;
                }
                if DomainValue::from_literal(const_literal, *base).is_none() {
                    continue;
                }
                members.entry(id.clone()).or_default().push(ClosedMember {
                    display_name: domain_display_name(qualified_name.as_str()).into(),
                    literal: const_literal.clone(),
                });
            }
        }

        let mut domains: HashMap<Symbol, ClosedDomain> = HashMap::default();
        for (type_id, (base, _)) in bases {
            let Some(mut domain_members) = members.remove(&type_id) else {
                continue;
            };
            domain_members.sort_by_key(|member| member.value(base));
            domains.insert(
                type_id.clone(),
                ClosedDomain {
                    base,
                    type_display: domain_display_name(type_id.as_str()).into(),
                    members: domain_members,
                },
            );
        }

        self.closed_domains = domains;
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
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { methods, .. } => Some(methods),
            DefinitionBody::TypeAlias { methods, .. } => Some(methods),
            DefinitionBody::Enum { methods, .. } => Some(methods),
            _ => None,
        }
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

fn domain_display_name(qualified: &str) -> String {
    let Some((module, name)) = qualified.rsplit_once('.') else {
        return qualified.to_string();
    };
    match module.strip_prefix("go:") {
        Some(go_module) => {
            let package = go_module.rsplit('/').next().unwrap_or(go_module);
            format!("{package}.{name}")
        }
        None => name.to_string(),
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
mod closed_domain_tests {
    use super::*;
    use syntax::ast::{
        Annotation, Generic, Span, StructFieldDefinition, StructFieldKind, StructFields,
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
    fn storing_a_file_owns_its_test_classification() {
        let mut store = Store::new();
        store.add_module("m");
        store.store_file(File::new_cached("m", "sample.test.lis", "", "", 42));

        assert!(store.is_test_file(42));

        store.store_file(File::new_cached("m", "sample.lis", "", "", 42));

        assert!(!store.is_test_file(42));
    }

    #[test]
    fn replacing_a_module_removes_its_old_test_classification() {
        let mut store = Store::new();
        store.add_module("m");
        store.store_file(File::new_cached("m", "sample.test.lis", "", "", 42));

        store.insert_prebuilt_module(Module::new("m"));

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
            name: None,
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
            name: None,
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
    fn tagged_type_with_members_is_indexed_and_sorted() {
        let mut store = Store::new();
        let ty = nominal_int("m.Weekday");
        insert(&mut store, "m", "m.Weekday", struct_def(ty.clone(), true));
        insert(&mut store, "m", "m.Saturday", int_const(ty.clone(), 6));
        insert(&mut store, "m", "m.Sunday", int_const(ty.clone(), 0));

        store.build_closed_domains();

        let domain = store
            .closed_domains
            .get("m.Weekday")
            .expect("tagged type with members should be indexed");
        assert_eq!(domain.base, SimpleKind::Int);
        assert_eq!(domain.type_display.as_str(), "Weekday");
        let names: Vec<&str> = domain
            .members
            .iter()
            .map(|m| m.display_name.as_str())
            .collect();
        assert_eq!(names, vec!["Sunday", "Saturday"]);
    }

    #[test]
    fn untagged_type_is_absent() {
        let mut store = Store::new();
        let ty = nominal_int("m.Plain");
        insert(&mut store, "m", "m.Plain", struct_def(ty.clone(), false));
        insert(&mut store, "m", "m.One", int_const(ty, 1));

        store.build_closed_domains();

        assert!(store.closed_domains.is_empty());
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

        store.build_closed_domains();

        assert!(!store.closed_domains.contains_key("m.Empty"));
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

        store.build_closed_domains();

        let domain = store.closed_domains.get("lib.Weekday").unwrap();
        let names: Vec<&str> = domain
            .members
            .iter()
            .map(|m| m.display_name.as_str())
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
                name: Some("Items".into()),
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
                name: name.rsplit('.').next().map(Into::into),
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
