use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::atomic::{AtomicU32, Ordering};

use syntax::ast::{EnumVariant, Expression, StructFieldDefinition};
use syntax::program::{Definition, File, Interface, MethodSignatures, Module, ModuleId};
use syntax::types::{SubstitutionMap, Symbol, Type, substitute};

pub const ENTRY_MODULE_ID: &str = "_entry_";
pub const ENTRY_FILE_ID: u32 = 0;

pub struct Store {
    pub modules: HashMap<String, Module>,
    pub module_ids: Vec<ModuleId>,
    /// file ID -> module ID
    pub files: HashMap<u32, String>,
    /// Go module ID -> Go package name, from the typedef `// Package:` directive.
    /// Present only when the package name differs from the final path segment.
    pub go_package_names: HashMap<String, String>,
    visited_modules: HashSet<String>,
    /// File ID counter. Starts at 2 because 0 is reserved for entry, 1 for prelude.
    next_file_id: AtomicU32,
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
            (prelude_module.id.clone(), prelude_module),
            (nominal_module.id.clone(), nominal_module),
        ]
        .into_iter()
        .collect();

        let module_ids = vec!["prelude".to_string()];

        Self {
            files: Default::default(),
            modules,
            module_ids,
            go_package_names: Default::default(),
            visited_modules: Default::default(),
            next_file_id: AtomicU32::new(2), // 0 = entrypoint, 1 = prelude
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

    pub fn store_entry_file(&mut self, filename: &str, source: &str, ast: Vec<Expression>) {
        self.store_file(
            ENTRY_MODULE_ID,
            File {
                id: ENTRY_FILE_ID,
                module_id: ENTRY_MODULE_ID.to_string(),
                name: filename.to_string(),
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
        let module = self.modules.get_mut(&module_id)?;
        module
            .files
            .get_mut(&file_id)
            .or_else(|| module.typedefs.get_mut(&file_id))
    }

    pub fn get_module(&self, module_id: &str) -> Option<&Module> {
        self.modules.get(module_id)
    }

    pub fn has(&self, module_id: &str) -> bool {
        self.modules.contains_key(module_id)
    }

    pub fn add_module(&mut self, module_id: &str) {
        if self.modules.contains_key(module_id) {
            return;
        }

        self.modules
            .insert(module_id.to_string(), Module::new(module_id));
        self.module_ids.push(module_id.to_string());
    }

    pub fn get_module_mut(&mut self, module_id: &str) -> Option<&mut Module> {
        self.modules.get_mut(module_id)
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
        if !qualified_name.starts_with("go:") || !qualified_name.contains('/') {
            let (module_name, _) = qualified_name.split_once('.')?;
            return Some(module_name);
        }

        let mut best: Option<&str> = None;
        for module_id in self.modules.keys() {
            if qualified_name.starts_with(module_id.as_str())
                && qualified_name.as_bytes().get(module_id.len()) == Some(&b'.')
                && best
                    .as_ref()
                    .is_none_or(|prev| module_id.len() > prev.len())
            {
                best = Some(module_id.as_str());
            }
        }
        best
    }

    pub fn variants_of(&self, qualified_name: &str) -> Option<&[EnumVariant]> {
        match self.get_definition(qualified_name)? {
            Definition::Enum { variants, .. } => Some(variants),
            _ => None,
        }
    }

    pub fn variant_of(&self, enum_qualified: &str, variant_name: &str) -> Option<&EnumVariant> {
        self.variants_of(enum_qualified)?
            .iter()
            .find(|v| v.name == variant_name)
    }

    pub fn value_variants_of(
        &self,
        qualified_name: &str,
    ) -> Option<&[syntax::ast::ValueEnumVariant]> {
        match self.get_definition(qualified_name)? {
            Definition::ValueEnum { variants, .. } => Some(variants),
            _ => None,
        }
    }

    pub fn fields_of(&self, qualified_name: &str) -> Option<&[StructFieldDefinition]> {
        match self.get_definition(qualified_name)? {
            Definition::Struct { fields, .. } => Some(fields),
            _ => None,
        }
    }

    pub fn struct_kind(&self, qualified_name: &str) -> Option<syntax::ast::StructKind> {
        match self.get_definition(qualified_name)? {
            Definition::Struct { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    pub fn struct_constructor(&self, qualified_name: &str) -> Option<&Type> {
        match self.get_definition(qualified_name)? {
            Definition::Struct { constructor, .. } => constructor.as_ref(),
            _ => None,
        }
    }

    pub fn parent_interfaces_of(&self, qualified_name: &str) -> Option<&[Type]> {
        match self.get_definition(qualified_name)? {
            Definition::Interface { definition, .. } => Some(&definition.parents),
            _ => None,
        }
    }

    pub fn get_type(&self, qualified_name: &str) -> Option<&Type> {
        self.get_definition(qualified_name)
            .map(|definition| definition.ty())
    }

    pub fn get_interface(&self, qualified_name: &str) -> Option<&Interface> {
        match self.get_definition(qualified_name)? {
            Definition::Interface { definition, .. } => Some(definition),
            _ => None,
        }
    }

    pub fn peel_alias(&self, ty: &Type) -> Type {
        syntax::types::peel_alias(ty, |id| {
            self.get_definition(id)
                .is_some_and(Definition::is_type_alias)
        })
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
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| self.peel_alias_deep(p)).collect(),
                param_mutability,
                bounds,
                return_type: Box::new(self.peel_alias_deep(&return_type)),
            },
            other => other,
        }
    }

    pub fn get_own_methods(&self, qualified_name: &str) -> Option<&MethodSignatures> {
        match self.get_definition(qualified_name)? {
            Definition::Struct { methods, .. } => Some(methods),
            Definition::TypeAlias { methods, .. } => Some(methods),
            Definition::Enum { methods, .. } => Some(methods),
            Definition::ValueEnum { methods, .. } => Some(methods),
            _ => None,
        }
    }

    pub fn get_all_methods(
        &self,
        ty: &Type,
        trait_bounds: &HashMap<Symbol, Vec<Type>>,
    ) -> MethodSignatures {
        let stripped = ty.strip_refs();
        let Some(qualified_name) = method_lookup_key(&stripped) else {
            return MethodSignatures::default();
        };

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
                for (name, method_ty) in self.get_all_methods(parent, trait_bounds) {
                    all_interface_methods.insert(name, method_ty);
                }
            }

            return all_interface_methods;
        }

        if let Some(bound_types) = trait_bounds.get(&qualified_name) {
            return bound_types
                .iter()
                .flat_map(|interface_ty| self.get_all_methods(interface_ty, trait_bounds))
                .collect();
        }

        let mut methods = self
            .get_own_methods(&qualified_name)
            .cloned()
            .unwrap_or_default();

        // Type aliases inherit methods from the underlying type.
        if let Some(Definition::TypeAlias { ty: alias_ty, .. }) =
            self.get_definition(&qualified_name)
        {
            let underlying = match &alias_ty {
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
                for (name, method_ty) in self.get_all_methods(&alias_ty, trait_bounds) {
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
