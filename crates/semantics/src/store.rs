use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{EnumVariant, Expression, Literal, StructFieldDefinition};
use syntax::program::{
    Definition, DefinitionBody, File, Interface, MethodSignatures, Module, ModuleId, UsableEquals,
};
use syntax::types::{SimpleKind, SubstitutionMap, Symbol, Type, substitute};

pub const ENTRY_MODULE_ID: &str = "_entry_";
pub const ENTRY_FILE_ID: u32 = 0;

#[derive(Debug, Clone)]
pub struct ClosedMember {
    /// Qualified the way the user writes it (e.g. `time.Sunday`), for the diagnostic.
    pub display_name: EcoString,
    /// The member's source literal, for rendering the valid-set hint.
    pub literal: Literal,
    /// The comparable form, derived once so membership and sort never disagree.
    pub value: DomainValue,
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
pub fn is_unsigned_base(base: SimpleKind) -> bool {
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
    pub module_ids: Vec<ModuleId>,
    /// file ID -> module ID
    pub files: HashMap<u32, String>,
    /// Go module ID -> Go package name, from the typedef `// Package:` directive.
    /// Present only when the package name differs from the final path segment.
    pub go_package_names: HashMap<String, String>,
    /// File ID -> on-disk path of the `.d.lis` typedef. Lets the LSP map go: typedef
    /// file IDs to the actual cache path so go-to-definition can navigate there.
    pub typedef_paths: HashMap<u32, PathBuf>,
    visited_modules: HashSet<String>,
    /// Types whose `equals` carries bounds the type does not imply; excluded from `usable_equals`.
    pub equals_bound_mismatch: HashSet<String>,
    pub usable_equals: UsableEquals,
    /// File ID counter. Starts at 2 because 0 is reserved for entry, 1 for prelude.
    next_file_id: AtomicU32,
    /// Closed-domain index, keyed by the type's qualified name (the `id` in
    /// `Type::Nominal`). Built once after registration by `build_closed_domains`.
    pub closed_domains: HashMap<Symbol, ClosedDomain>,
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

        let module_ids = vec!["prelude".to_string()];

        Self {
            files: Default::default(),
            modules,
            module_ids,
            go_package_names: Default::default(),
            typedef_paths: Default::default(),
            visited_modules: Default::default(),
            equals_bound_mismatch: Default::default(),
            usable_equals: Default::default(),
            next_file_id: AtomicU32::new(2), // 0 = entrypoint, 1 = prelude
            closed_domains: Default::default(),
        }
    }

    pub fn new_file_id(&self) -> u32 {
        self.next_file_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn register_file(&mut self, file_id: u32, module_id: &str) {
        self.files.insert(file_id, module_id.to_string());
    }

    pub fn entry_module_id(&self) -> &'static str {
        ENTRY_MODULE_ID
    }

    /// Initializes the entry module with reserved file ID 0.
    pub fn init_entry_module(&mut self) {
        self.add_module(ENTRY_MODULE_ID);
        self.register_file(ENTRY_FILE_ID, ENTRY_MODULE_ID);
    }

    pub fn store_entry_file(
        &mut self,
        filename: &str,
        display_path: &str,
        source: &str,
        ast: Vec<Expression>,
    ) {
        self.store_file(
            ENTRY_MODULE_ID,
            File {
                id: ENTRY_FILE_ID,
                module_id: ENTRY_MODULE_ID.to_string(),
                name: filename.to_string(),
                display_path: display_path.to_string(),
                source: source.to_string(),
                items: ast,
            },
        );
    }

    pub fn store_module(&mut self, module_id: &str, files: Vec<File>) {
        self.mark_visited(module_id);
        self.add_module(module_id);

        for file in files {
            self.store_file(module_id, file);
        }
    }

    /// Stores a file in the module and registers the file_id -> module_id mapping.
    /// .d.lis files go to `typedefs`, .lis files go to `files`.
    pub fn store_file(&mut self, module_id: &str, file: File) {
        self.files.insert(file.id, module_id.to_string());

        let module = self
            .get_module_mut(module_id)
            .expect("module must exist to store file");

        if file.is_d_lis() {
            module.typedefs.insert(file.id, file);
        } else {
            module.files.insert(file.id, file);
        }
    }

    pub fn get_file(&self, file_id: u32) -> Option<&File> {
        let module_id = self.files.get(&file_id)?;
        let module = self.get_module(module_id)?;
        module
            .get_file(file_id)
            .or_else(|| module.get_typedef_by_id(file_id))
    }

    pub fn get_file_mut(&mut self, file_id: u32) -> Option<&mut File> {
        let module_id = self.files.get(&file_id)?.clone();
        let module = Arc::make_mut(self.modules.get_mut(&module_id)?);
        module
            .files
            .get_mut(&file_id)
            .or_else(|| module.typedefs.get_mut(&file_id))
    }

    pub fn get_module(&self, module_id: &str) -> Option<&Module> {
        self.modules.get(module_id).map(Arc::as_ref)
    }

    pub fn has(&self, module_id: &str) -> bool {
        self.modules.contains_key(module_id)
    }

    pub fn add_module(&mut self, module_id: &str) {
        if self.modules.contains_key(module_id) {
            return;
        }

        self.modules
            .insert(module_id.to_string(), Arc::new(Module::new(module_id)));
        self.module_ids.push(module_id.to_string());
    }

    pub fn get_module_mut(&mut self, module_id: &str) -> Option<&mut Module> {
        self.modules.get_mut(module_id).map(Arc::make_mut)
    }

    /// `Arc`-bump snapshot for a registration worker, which inserts its own
    /// detached module before use.
    pub(crate) fn registration_view(&self) -> Store {
        Store {
            modules: self.modules.clone(),
            module_ids: self.module_ids.clone(),
            files: self.files.clone(),
            go_package_names: self.go_package_names.clone(),
            typedef_paths: HashMap::default(),
            visited_modules: HashSet::default(),
            equals_bound_mismatch: HashSet::default(),
            usable_equals: UsableEquals::default(),
            next_file_id: AtomicU32::new(self.next_file_id.load(Ordering::Relaxed)),
            closed_domains: HashMap::default(),
        }
    }

    pub fn is_visited(&self, module_id: &str) -> bool {
        self.visited_modules.contains(module_id)
    }

    pub fn mark_visited(&mut self, module_id: &str) {
        self.visited_modules.insert(module_id.to_string());
    }

    pub fn get_definition(&self, qualified_name: &str) -> Option<&Definition> {
        let module_name = self.module_for_qualified_name(qualified_name)?;

        self.get_module(module_name)?
            .definitions
            .get(qualified_name)
    }

    pub fn module_for_qualified_name<'a>(&'a self, qualified_name: &'a str) -> Option<&'a str> {
        syntax::types::module_for_qualified_name(
            qualified_name,
            self.modules.keys().map(String::as_str),
        )
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

    pub fn is_nominal_defined_type(&self, qualified_name: &str) -> bool {
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
                    && let Some(base) = definition.ty().underlying_simple_kind()
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
                let Type::Nominal { id, .. } = definition.ty() else {
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
                let Some(value) = DomainValue::from_literal(const_literal, *base) else {
                    continue;
                };
                members.entry(id.clone()).or_default().push(ClosedMember {
                    display_name: domain_display_name(qualified_name.as_str()).into(),
                    literal: const_literal.clone(),
                    value,
                });
            }
        }

        let mut domains: HashMap<Symbol, ClosedDomain> = HashMap::default();
        for (type_id, (base, _)) in bases {
            let Some(mut domain_members) = members.remove(&type_id) else {
                continue;
            };
            domain_members.sort_by(|a, b| a.value.cmp(&b.value));
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

    pub fn fields_of(&self, qualified_name: &str) -> Option<&[StructFieldDefinition]> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { fields, .. } => Some(fields),
            _ => None,
        }
    }

    pub fn struct_kind(&self, qualified_name: &str) -> Option<syntax::ast::StructKind> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    pub fn struct_constructor(&self, qualified_name: &str) -> Option<&Type> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Struct { constructor, .. } => constructor.as_ref(),
            _ => None,
        }
    }

    pub fn parent_interfaces_of(&self, qualified_name: &str) -> Option<&[Type]> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Interface { definition, .. } => Some(&definition.parents),
            _ => None,
        }
    }

    pub fn get_type(&self, qualified_name: &str) -> Option<&Type> {
        self.get_definition(qualified_name)
            .map(|definition| definition.ty())
    }

    pub fn get_interface(&self, qualified_name: &str) -> Option<&Interface> {
        match &self.get_definition(qualified_name)?.body {
            DefinitionBody::Interface { definition, .. } => Some(definition),
            _ => None,
        }
    }

    pub fn is_interface(&self, ty: &Type) -> bool {
        matches!(ty, Type::Nominal { id, .. } if self.get_interface(id.as_str()).is_some())
    }

    pub fn is_nilable_go_type(&self, ty: &Type) -> bool {
        if ty.is_ref() || matches!(ty, Type::Function(_)) {
            return true;
        }
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        if self.get_definition(id.as_str()).is_none() {
            return false;
        }
        if self.get_interface(id.as_str()).is_some() {
            return true;
        }
        match ty.get_underlying() {
            Some(Type::Function(_)) => true,
            Some(u) if u.is_ref() => true,
            _ => false,
        }
    }

    pub fn peel_alias(&self, ty: &Type) -> Type {
        syntax::types::peel_alias(ty, |id| {
            self.get_definition(id)
                .is_some_and(Definition::is_type_alias)
        })
    }

    pub fn deep_resolve_alias(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        let mut seen: HashSet<Symbol> = HashSet::default();
        loop {
            let Type::Nominal { id, params, .. } = &current else {
                return current;
            };
            if !seen.insert(id.clone()) {
                return current;
            }
            let Some(def) = self.get_definition(id.as_str()) else {
                return current;
            };
            if !matches!(def.body, DefinitionBody::TypeAlias { .. }) {
                return current;
            }
            let def_ty = &def.ty;
            let (vars, body) = match def_ty {
                Type::Forall { vars, body } => (vars.clone(), body.as_ref().clone()),
                other => (vec![], other.clone()),
            };
            let map: SubstitutionMap = vars.iter().cloned().zip(params.iter().cloned()).collect();
            current = substitute(&body, &map);
        }
    }

    pub fn peel_alias_deep(&self, ty: &Type) -> Type {
        match self.peel_alias(ty) {
            Type::Compound { kind, args } => Type::Compound {
                kind,
                args: args.iter().map(|a| self.peel_alias_deep(a)).collect(),
            },
            Type::Tuple(elements) => {
                Type::Tuple(elements.iter().map(|e| self.peel_alias_deep(e)).collect())
            }
            Type::Nominal {
                id,
                params,
                underlying_ty,
            } => Type::Nominal {
                id,
                params: params.iter().map(|p| self.peel_alias_deep(p)).collect(),
                underlying_ty,
            },
            Type::Function(f) => {
                let new_params = f.params.iter().map(|p| self.peel_alias_deep(p)).collect();
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

    pub fn get_all_methods(
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
            let alias_ty = &definition.ty;
            let underlying = match alias_ty {
                Type::Forall { body, .. } => body.as_ref(),
                other => other,
            };
            let underlying_key = match underlying {
                Type::Nominal { id, .. } => Some(id.as_str().to_string()),
                Type::Simple(kind) => Some(format!("prelude.{}", kind.leaf_name())),
                Type::Compound { kind, .. } => Some(format!("prelude.{}", kind.leaf_name())),
                _ => None,
            };
            // Follow only when the alias body names a different type. For
            // opaque prelude natives (e.g. `type Map<K, V>`) the body points
            // to itself — following would loop.
            if let Some(k) = underlying_key
                && k != qualified_name.as_str()
            {
                let alias_ty = alias_ty.clone();
                for (name, method_ty) in
                    self.get_all_methods_recursive(&alias_ty, trait_bounds, visited)
                {
                    methods.entry(name).or_insert(method_ty);
                }
            }
        }

        methods
    }

    pub fn get_methods_from_bounds(
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
        _ => None,
    }
}

#[cfg(test)]
mod closed_domain_tests {
    use super::*;
    use syntax::ast::StructKind;
    use syntax::program::{Attributes, TypeAttribute, Visibility};

    fn nominal_int(id: &str) -> Type {
        Type::Nominal {
            id: Symbol::from_raw(id),
            params: vec![],
            underlying_ty: Some(Box::new(Type::Simple(SimpleKind::Int))),
        }
    }

    fn struct_def(ty: Type, closed_domain: bool) -> Definition {
        let mut attributes = Attributes::default();
        if closed_domain {
            attributes.insert(TypeAttribute::ClosedDomain, ());
        }
        Definition {
            visibility: Visibility::Public,
            ty,
            name: None,
            name_span: None,
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![],
                fields: vec![],
                kind: StructKind::Tuple,
                methods: Default::default(),
                constructor: None,
                attributes,
            },
        }
    }

    fn int_const(ty: Type, value: u64) -> Definition {
        Definition {
            visibility: Visibility::Public,
            ty,
            name: None,
            name_span: None,
            doc: None,
            body: DefinitionBody::Value {
                allowed_lints: vec![],
                go_hints: vec![],
                go_name: None,
                const_value: Some(Literal::Integer { value, text: None }),
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
}
