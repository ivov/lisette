pub mod freeze;
pub mod infer;
pub(crate) mod registration;
pub mod scopes;
pub mod type_env;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::RefCell;
use std::sync::Arc;

use crate::facts::{BindingIdAllocator, Facts};
use crate::store::Store;
use diagnostics::LocalSink;
use ecow::EcoString;
use scopes::Scopes;
use syntax::ast::Visibility as AstVisibility;
use syntax::ast::{Annotation, Expression, Generic, ImportAlias, Span, StructFieldDefinition};
use syntax::program::{Definition, File, FileImport, MethodSignatures, Module};
use syntax::types::{SubstitutionMap, Symbol, Type, substitute};

pub use type_env::{EnvResolve, Speculation, TypeEnv, VarState};

#[derive(Debug, Clone)]
pub struct Cursor {
    pub module_id: String,
    pub file_id: Option<u32>,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            module_id: "std".to_string(),
            file_id: None,
        }
    }
}

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub struct ImportState {
    /// Module prefix -> (struct fields, module type)
    pub imported_modules: HashMap<String, (Vec<StructFieldDefinition>, Type)>,
    /// Import prefix -> actual module_id in Store (e.g., "http" -> "go:net/http")
    pub prefix_to_module: HashMap<String, String>,
    /// Modules whose exports are available without prefix (current module and prelude)
    pub unprefixed_imports: HashSet<String>,
    /// Effective aliases (e.g. `mux`) of imports whose underlying module
    /// failed to load (missing typedef, undeclared, module_not_found, etc.).
    pub failed_imports: HashSet<String>,
}

impl ImportState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        // Preserve prelude entries since they never change
        let prelude = self.imported_modules.remove("prelude");
        self.imported_modules.clear();
        if let Some(p) = prelude {
            self.imported_modules.insert("prelude".to_string(), p);
        }
        let prelude_mapping = self.prefix_to_module.remove("prelude");
        self.prefix_to_module.clear();
        if let Some(m) = prelude_mapping {
            self.prefix_to_module.insert("prelude".to_string(), m);
        }
        self.unprefixed_imports.clear();
        self.failed_imports.clear();
    }
}

/// Cache for builtin types (int, bool, string, etc.) resolved from the prelude.
/// These never change once populated, so no invalidation needed.
type BuiltinCache = HashMap<String, Type>;

/// Per-task mutable state. Paired with `AnalysisContext` (shared read-only view).
pub struct TaskState<'s> {
    pub env: TypeEnv,
    pub scopes: Scopes,
    pub cursor: Cursor,
    pub imports: ImportState,
    pub builtins: BuiltinCache,
    pub sink: &'s LocalSink,
    pub facts: Facts,
    /// Recursion guard for interface satisfaction. Prevents
    /// `collect_interface_violations` from diverging when a bound on `T`
    /// transitively requires checking `T` against the same interface.
    pub satisfying_stack: rustc_hash::FxHashSet<(String, String)>,
    method_cache: RefCell<HashMap<EcoString, MethodSignatures>>,
    pub ufcs_methods: HashSet<(String, String)>,
    /// Typed files produced by inference.
    pub typed_files: Vec<(String, File)>,
    /// Reentrancy counter: > 0 while resolving a generic bound annotation.
    /// Lets `convert_to_type` admit bound-only markers (e.g. `Comparable`)
    /// without flagging them as misuse in value positions.
    pub bound_position_depth: u32,
}

impl<'s> TaskState<'s> {
    pub fn new(sink: &'s LocalSink, binding_ids: Arc<BindingIdAllocator>) -> Self {
        Self {
            env: TypeEnv::new(),
            scopes: Scopes::new(),
            cursor: Cursor::new(),
            imports: ImportState::new(),
            builtins: BuiltinCache::default(),
            sink,
            facts: Facts::new(binding_ids),
            satisfying_stack: rustc_hash::FxHashSet::default(),
            method_cache: RefCell::new(HashMap::default()),
            ufcs_methods: HashSet::default(),
            typed_files: Vec::new(),
            bound_position_depth: 0,
        }
    }

    pub fn with_fresh_allocator(sink: &'s LocalSink) -> Self {
        Self::new(sink, Arc::new(BindingIdAllocator::new()))
    }

    pub fn new_type_var(&mut self) -> Type {
        let id = self.env.fresh(None);
        Type::Var { id, hint: None }
    }

    pub fn new_type_var_with_hint(&mut self, hint: &str) -> Type {
        let hint: EcoString = hint.into();
        let id = self.env.fresh(Some(hint.clone()));
        Type::Var {
            id,
            hint: Some(hint),
        }
    }

    pub fn type_from_literal_expression(&mut self, expression: &Expression) -> Option<Type> {
        use syntax::ast::{Expression, Literal};
        match expression {
            Expression::Literal { literal, .. } => match literal {
                Literal::Integer { .. } => Some(self.type_int()),
                Literal::Float { .. } => Some(self.type_float()),
                Literal::Boolean(_) => Some(self.type_bool()),
                Literal::String { .. } => Some(self.type_string()),
                Literal::Char(_) => Some(self.type_char()),
                _ => None,
            },
            Expression::Unary { expression, .. } => self.type_from_literal_expression(expression),
            _ => None,
        }
    }

    pub fn instantiate(&mut self, ty: &Type) -> (Type, SubstitutionMap) {
        match ty {
            Type::Forall { vars, body } => {
                let map: SubstitutionMap = vars
                    .iter()
                    .map(|name| {
                        let id = self.env.fresh(Some(name.clone()));
                        let fresh_var = Type::Var {
                            id,
                            hint: Some(name.clone()),
                        };
                        (name.clone(), fresh_var)
                    })
                    .collect();

                (substitute(body, &map), map)
            }
            _ => (ty.clone(), HashMap::default()),
        }
    }

    pub fn new_file_id(&mut self, store: &Store) -> u32 {
        store.new_file_id()
    }

    pub fn is_d_lis(&self, store: &Store) -> bool {
        let Some(file_id) = self.cursor.file_id else {
            return false;
        };

        let Some(module) = store.get_module(&self.cursor.module_id) else {
            return false;
        };

        module.typedefs.contains_key(&file_id)
    }

    pub fn is_lis(&self, store: &Store) -> bool {
        !self.is_d_lis(store)
    }

    pub(crate) fn qualify_name(&self, name: &str) -> Symbol {
        Symbol::from_parts(&self.cursor.module_id, name)
    }

    pub(crate) fn put_in_scope(&mut self, generics: &[Generic]) {
        for (index, generic) in generics.iter().enumerate() {
            self.scopes
                .current_mut()
                .type_params
                .get_or_insert_with(HashMap::default)
                .insert(generic.name.to_string(), index);
        }
    }

    /// Validate that all bound annotations on generics refer to types that exist in scope.
    pub(crate) fn validate_generic_bounds(
        &mut self,
        store: &Store,
        generics: &[Generic],
        span: &Span,
    ) {
        for g in generics {
            for b in &g.bounds {
                self.convert_bound_to_type(store, b, span);
            }
        }
    }

    /// Resolve a simple name (e.g., "Sunday") to a public definition in an imported module.
    /// First tries direct match (`module_id.name`), then falls back to searching
    /// for nested definitions (e.g., `module_id.Weekday.Sunday`) preferring top-level
    /// over nested when both share the same simple name.
    fn resolve_in_imported_module<'m>(
        &self,
        module: &'m Module,
        simple_name: &str,
    ) -> Option<(String, &'m Definition)> {
        let module_prefix = format!("{}.", module.id);

        // Direct match: module_id.simple_name
        let direct = format!("{}{}", module_prefix, simple_name);
        if let Some(definition) = module.definitions.get(direct.as_str())
            && definition.visibility().is_public()
        {
            return Some((direct, definition));
        }

        // Nested match: find a public definition whose simple name matches,
        // e.g., module_id.EnumType.VariantName where simple_name = "VariantName".
        // Skip if a top-level definition with the same simple name exists
        // (handles transitive import collisions like go:net/http).
        let suffix = format!(".{}", simple_name);
        for (qn, definition) in &module.definitions {
            if qn.ends_with(suffix.as_str())
                && qn.starts_with(module_prefix.as_str())
                && definition.visibility().is_public()
            {
                let rest = &qn[module_prefix.len()..];
                // Only match if it's nested (contains a dot) — direct was tried above
                if rest.contains('.') {
                    return Some((qn.to_string(), definition));
                }
            }
        }

        None
    }

    pub(crate) fn lookup_qualified_name(&self, store: &Store, type_name: &str) -> Option<String> {
        if let Some((prefix, simple_name)) = type_name.split_once('.')
            && let Some(module_id) = self.imports.prefix_to_module.get(prefix)
            && let Some(imported_module) = store.get_module(module_id)
            && let Some((qualified_name, _)) =
                self.resolve_in_imported_module(imported_module, simple_name)
        {
            return Some(qualified_name);
        }

        let module = store.get_module(&self.cursor.module_id)?;
        let qualified_name = Symbol::from_parts(&module.id, type_name);

        if module.definitions.contains_key(qualified_name.as_str()) {
            return Some(qualified_name.to_string());
        }

        for imported_module_id in &self.imports.unprefixed_imports {
            if let Some(imported_module) = store.get_module(imported_module_id) {
                let qualified_name = Symbol::from_parts(imported_module_id, type_name);
                if imported_module
                    .definitions
                    .contains_key(qualified_name.as_str())
                {
                    return Some(qualified_name.to_string());
                }
            }
        }

        None
    }

    pub(crate) fn get_definition_name_span(
        &self,
        store: &Store,
        qualified_name: &str,
    ) -> Option<Span> {
        store.get_definition(qualified_name)?.name_span()
    }

    pub(crate) fn is_const_name(&self, store: &Store, qualified_name: &str) -> bool {
        if qualified_name.starts_with("go:") {
            return false;
        }
        store
            .module_for_qualified_name(qualified_name)
            .and_then(|module_id| store.get_module(module_id))
            .is_some_and(|module| module.const_names.contains(qualified_name))
    }

    pub(crate) fn is_const_var(&self, store: &Store, var_name: &str) -> bool {
        self.scopes.lookup_binding_id(var_name).is_none()
            && self
                .lookup_qualified_name(store, var_name)
                .is_some_and(|qname| self.is_const_name(store, &qname))
    }

    /// Track that `name` (at the start of `span`) refers to the definition at `qualified_name`.
    pub(crate) fn track_name_usage(
        &mut self,
        store: &Store,
        qualified_name: &str,
        span: &Span,
        name_len: u32,
    ) {
        if let Some(definition_span) = self.get_definition_name_span(store, qualified_name) {
            let usage_span = Span::new(span.file_id, span.byte_offset, name_len);
            self.facts.add_usage(usage_span, definition_span);
        }
    }

    pub(crate) fn lookup_generic_index(&self, type_name: &str) -> Option<usize> {
        self.scopes.lookup_type_param(type_name)
    }

    /// Resolves the value type for a definition. Returns the constructor type for
    /// structs with constructors (tuple structs) and for type aliases pointing to them.
    fn resolve_definition_value_type(&self, store: &Store, definition: &Definition) -> Type {
        if let Definition::Struct {
            constructor: Some(ctor_ty),
            ..
        } = definition
        {
            return ctor_ty.clone();
        }

        // Type alias to tuple struct should return constructor type.
        if let Definition::TypeAlias { ty: alias_ty, .. } = definition {
            let underlying = match alias_ty {
                Type::Forall { body, .. } => body.as_ref(),
                other => other,
            };
            if let Type::Nominal { id, .. } = underlying
                && let Some(Definition::Struct {
                    constructor: Some(ctor_ty),
                    ..
                }) = store.get_definition(id)
            {
                return ctor_ty.clone();
            }
        }

        definition.ty().clone()
    }

    pub(crate) fn lookup_type(&self, store: &Store, value_name: &str) -> Option<Type> {
        if let Some(ty) = self.scopes.lookup_value(value_name) {
            return Some(ty.clone());
        }

        if let Some((_definition, ty)) = self.imports.imported_modules.get(value_name) {
            return Some(ty.clone());
        }

        if let Some((prefix, rest)) = value_name.split_once('.')
            && let Some(module_id) = self.imports.prefix_to_module.get(prefix)
            && let Some(imported_module) = store.get_module(module_id)
            && let Some((_, definition)) = self.resolve_in_imported_module(imported_module, rest)
        {
            return Some(self.resolve_definition_value_type(store, definition));
        }

        let module = store.get_module(&self.cursor.module_id)?;
        let qualified_name = Symbol::from_parts(&module.id, value_name);

        if let Some(definition) = module.definitions.get(qualified_name.as_str()) {
            return Some(self.resolve_definition_value_type(store, definition));
        }

        for imported_module_id in &self.imports.unprefixed_imports {
            if let Some(imported_module) = store.get_module(imported_module_id) {
                let qualified_name = Symbol::from_parts(imported_module_id, value_name);
                if let Some(definition) = imported_module.definitions.get(qualified_name.as_str()) {
                    return Some(self.resolve_definition_value_type(store, definition));
                }
            }
        }

        None
    }

    pub(crate) fn is_enum_type(&self, store: &Store, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        let Some(definition) = store.get_definition(id) else {
            return false;
        };
        matches!(
            definition,
            Definition::Enum { .. } | Definition::ValueEnum { .. }
        )
    }

    pub(crate) fn resolve_type_name(
        &mut self,
        store: &Store,
        type_name: &str,
    ) -> Option<(String, Type)> {
        if self.scopes.lookup_type_param(type_name).is_some() {
            return None;
        }

        let qualified_name = self.lookup_qualified_name(store, type_name)?;
        let ty = store.get_type(&qualified_name)?.clone();

        Some((qualified_name, ty))
    }

    pub(crate) fn resolve_type_from_prelude(
        &self,
        store: &Store,
        type_name: &str,
    ) -> Option<(String, Type)> {
        let qualified_name = format!("prelude.{}", type_name);
        let ty = store.get_type(&qualified_name)?.clone();
        Some((qualified_name, ty))
    }

    pub(crate) fn get_all_methods(&self, store: &Store, ty: &Type) -> MethodSignatures {
        if let Type::Parameter(name) = ty {
            let trait_bounds = self.scopes.collect_all_trait_bounds();
            let qualified_name = self.qualify_name(name);
            return store.get_methods_from_bounds(&qualified_name, &trait_bounds);
        }

        let resolved = ty.strip_refs().resolve_in(&self.env);
        let cache_key: EcoString = match &resolved {
            Type::Nominal { id, .. } => id.as_eco().clone(),
            Type::Compound { kind, .. } => format!("prelude.{}", kind.leaf_name()).into(),
            Type::Simple(kind) => format!("prelude.{}", kind.leaf_name()).into(),
            _ => return MethodSignatures::default(),
        };

        // Interfaces need type-arg-dependent generic substitution, skip cache.
        let peeled = store.peel_alias(&resolved);
        if let Type::Nominal { id: peeled_id, .. } = &peeled
            && store.get_interface(peeled_id).is_some()
        {
            let empty = HashMap::default();
            return store.get_all_methods(&peeled, &empty);
        }

        if let Some(cached) = self.method_cache.borrow().get(cache_key.as_str()) {
            return cached.clone();
        }

        let empty = HashMap::default();
        // Pass the env-resolved type so the store's env-less `resolve()` stays
        // identity-safe: `Type::Var` chains are chased once here rather than
        // silently returning empty methods in the store.
        let methods = store.get_all_methods(&resolved, &empty);
        self.method_cache
            .borrow_mut()
            .insert(cache_key, methods.clone());
        methods
    }

    pub fn reset_scopes(&mut self) {
        self.scopes.reset();
        self.imports.clear();
    }

    pub fn failed(&self) -> bool {
        self.sink.has_errors()
    }

    pub fn put_prelude_in_scope(&mut self, store: &Store) {
        self.put_unprefixed_module_in_scope(store, "prelude");
        if self.imports.imported_modules.contains_key("prelude") {
            return;
        }
        self.put_module_in_scope(store, "prelude", Some("prelude".to_string()));
    }

    pub fn put_unprefixed_module_in_scope(&mut self, store: &Store, module_id: &str) {
        self.put_module_in_scope(store, module_id, None)
    }

    pub fn put_imported_modules_in_scope(&mut self, store: &Store, imports: &[FileImport]) {
        let mut seen_aliases: HashMap<String, String> = HashMap::default(); // alias -> path
        let mut seen_paths: HashSet<String> = HashSet::default();

        for import in imports {
            if seen_paths.contains(import.name.as_str()) {
                self.sink.push(diagnostics::infer::duplicate_import_path(
                    &import.name,
                    import.name_span,
                ));
                continue;
            }
            seen_paths.insert(import.name.to_string());

            if let Some(ImportAlias::Blank(blank_span)) = &import.alias {
                if !import.name.starts_with("go:") {
                    self.sink
                        .push(diagnostics::infer::blank_import_non_go(*blank_span));
                }
                continue;
            }

            if let Some(ImportAlias::Named(alias, alias_span)) = &import.alias
                && is_reserved_import_alias(alias)
            {
                self.sink.push(diagnostics::infer::reserved_import_alias(
                    alias,
                    *alias_span,
                ));
                continue;
            }

            let Some(effective) = import.effective_alias(&store.go_package_names) else {
                continue;
            };

            if let Some(existing_path) = seen_aliases.get(&effective)
                && existing_path != &import.name
            {
                self.sink.push(diagnostics::infer::import_conflict(
                    &effective,
                    existing_path,
                    &import.name,
                    import.name_span,
                ));
                continue;
            }

            seen_aliases.insert(effective.clone(), import.name.to_string());

            let module = store.get_module(&import.name);
            if module.is_none() || module.is_some_and(Module::is_empty_stub) {
                self.imports.failed_imports.insert(effective);
                continue;
            }

            self.put_module_in_scope(store, &import.name, Some(effective));
        }
    }

    pub fn put_module_in_scope(&mut self, store: &Store, module_id: &str, prefix: Option<String>) {
        let Some(prefix) = prefix else {
            self.imports
                .unprefixed_imports
                .insert(module_id.to_string());
            return;
        };

        let module = store
            .get_module(module_id)
            .expect("module must exist when putting in scope");

        let imported_module_id = module.id.clone();
        let module_prefix = format!("{}.", module.id);

        let module_struct_fields: Vec<_> = module
            .definitions
            .iter()
            .filter(|(qn, _)| module.is_public(qn))
            .filter(|(qn, _)| {
                qn.strip_prefix(&module_prefix)
                    .is_some_and(|rest| !rest.contains('.'))
            })
            .map(|(qn, definition)| {
                let simple_name = qn
                    .strip_prefix(&module_prefix)
                    .expect("qualified_name must start with module prefix");
                let ty = if let Definition::Struct {
                    constructor: Some(ctor_ty),
                    ..
                } = definition
                {
                    ctor_ty.clone()
                } else {
                    definition.ty().clone()
                };
                StructFieldDefinition {
                    doc: None,
                    attributes: vec![],
                    visibility: AstVisibility::Public,
                    name: simple_name.into(),
                    name_span: Span::dummy(),
                    annotation: Annotation::Unknown,
                    ty,
                }
            })
            .collect();

        let ty = Type::ImportNamespace(imported_module_id.clone().into());

        self.imports
            .imported_modules
            .insert(prefix.clone(), (module_struct_fields, ty));
        self.imports
            .prefix_to_module
            .insert(prefix, imported_module_id);
    }

    /// Run a closure speculatively: if it returns `Err`, all type variable
    /// bindings performed during the closure are rolled back.
    pub(crate) fn speculatively<T, E>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let spec = self.env.begin_speculation();
        let result = f(self);
        self.env.end_speculation(spec, result.is_err());
        result
    }
}

/// Returns `true` if the given name is reserved and cannot be used as an import alias.
///
/// Reserved names include Go keywords, Go predeclared identifiers, Go builtins,
/// Go type constraint names, and Lisette prelude symbols.
fn is_reserved_import_alias(name: &str) -> bool {
    matches!(
        name,
        // Go keywords
        "break"
        | "case"
        | "chan"
        | "const"
        | "continue"
        | "default"
        | "defer"
        | "else"
        | "fallthrough"
        | "for"
        | "func"
        | "go"
        | "goto"
        | "if"
        | "interface"
        | "map"
        | "package"
        | "range"
        | "return"
        | "select"
        | "struct"
        | "switch"
        | "type"
        | "var"
        // Go predeclared identifiers
        | "nil"
        | "iota"
        | "true"
        | "false"
        // Go predeclared types
        | "bool"
        | "byte"
        | "complex64"
        | "complex128"
        | "error"
        | "float32"
        | "float64"
        | "int"
        | "int8"
        | "int16"
        | "int32"
        | "int64"
        | "rune"
        | "string"
        | "uint"
        | "uint8"
        | "uint16"
        | "uint32"
        | "uint64"
        | "uintptr"
        // Go builtins
        | "append"
        | "cap"
        | "clear"
        | "close"
        | "complex"
        | "copy"
        | "delete"
        | "imag"
        | "len"
        | "make"
        | "max"
        | "min"
        | "new"
        | "panic"
        | "print"
        | "println"
        | "real"
        | "recover"
        // Go type constraints
        | "any"
        | "comparable"
        // Special Go identifiers
        | "init"
        | "main"
        // Lisette prelude types and constructors
        | "Option"
        | "Result"
        | "Comparable"
        | "Some"
        | "None"
        | "Ok"
        | "Err"
    )
}
