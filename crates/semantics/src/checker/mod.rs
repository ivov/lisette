pub mod freeze;
pub mod infer;
pub mod promotion;
pub(crate) mod registration;
pub mod scopes;
mod sealing;
pub mod type_env;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::facts::{BindingIdAllocator, Facts};
use crate::store::Store;
use diagnostics::LocalSink;
use ecow::EcoString;
use registration::derived_attributes::DerivedAttribute;
use scopes::Scopes;
use syntax::ast::Visibility as AstVisibility;
use syntax::ast::{Annotation, Expression, Generic, ImportAlias, Span, StructFieldDefinition};
use syntax::program::{
    Definition, DefinitionBody, File, FileImport, MethodSignatures, Module, NativeTypeKind,
    go_import_default_name,
};
use syntax::types::{Bound, SubstitutionMap, Symbol, Type, substitute};

pub use infer::expressions::comparison::{check_never_comparable, check_not_comparable};
pub use type_env::{EnvResolve, TypeEnv, VarState};

#[derive(Debug, Clone)]
pub struct Cursor {
    pub module_id: String,
    file_id: Option<u32>,
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
    fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub struct ImportState {
    prefixed: HashMap<String, PrefixedImport>,
    /// Modules whose exports are available without prefix (current module and prelude)
    unprefixed_imports: HashSet<String>,
}

#[derive(Debug)]
enum PrefixedImport {
    Namespace {
        module_id: String,
        fields: Arc<[StructFieldDefinition]>,
    },
    /// A typedef's self-prefix resolves qualified names but is not itself a value.
    LookupOnly {
        module_id: String,
    },
    Failed,
}

impl ImportState {
    fn new() -> Self {
        Self::default()
    }

    fn module_id(&self, prefix: &str) -> Option<&str> {
        match self.prefixed.get(prefix)? {
            PrefixedImport::Namespace { module_id, .. }
            | PrefixedImport::LookupOnly { module_id } => Some(module_id),
            PrefixedImport::Failed => None,
        }
    }

    fn namespace(&self, prefix: &str) -> Option<(&str, &Arc<[StructFieldDefinition]>)> {
        match self.prefixed.get(prefix)? {
            PrefixedImport::Namespace { module_id, fields } => Some((module_id, fields)),
            PrefixedImport::LookupOnly { .. } | PrefixedImport::Failed => None,
        }
    }

    fn modules(&self) -> impl Iterator<Item = (&str, &str)> {
        self.prefixed.iter().filter_map(|(prefix, import)| {
            let module_id = match import {
                PrefixedImport::Namespace { module_id, .. }
                | PrefixedImport::LookupOnly { module_id } => module_id,
                PrefixedImport::Failed => return None,
            };
            Some((prefix.as_str(), module_id.as_str()))
        })
    }

    fn namespaces(&self) -> impl Iterator<Item = (&str, &Arc<[StructFieldDefinition]>)> {
        self.prefixed.values().filter_map(|import| match import {
            PrefixedImport::Namespace { module_id, fields } => Some((module_id.as_str(), fields)),
            PrefixedImport::LookupOnly { .. } | PrefixedImport::Failed => None,
        })
    }

    fn is_failed(&self, prefix: &str) -> bool {
        matches!(self.prefixed.get(prefix), Some(PrefixedImport::Failed))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileContextKind {
    Standard,
    ImportedTypedef,
    Prelude,
    TestPrelude,
}

struct SavedFileContext {
    file_id: Option<u32>,
    scopes: Scopes,
    imports: ImportState,
}

type ModuleFieldMap = HashMap<EcoString, Arc<[StructFieldDefinition]>>;
type UfcsMethod = (String, String);

/// The parts of a checker task that survive after its local inference state is
/// discarded.
pub(crate) struct TaskOutput {
    facts: Facts,
    module_fields: Arc<ModuleFieldMap>,
    ufcs_methods: Arc<HashSet<UfcsMethod>>,
    typed_files: Vec<(String, File)>,
    pending_equality_attributes: Vec<DerivedAttribute>,
    pending_generic_bound_checks: Vec<(Type, Type, Span)>,
    pending_interface_bound_checks: Vec<(Type, Type, Span)>,
    sink: LocalSink,
}

/// A consistent read-only snapshot from which parallel checker tasks start.
pub(crate) struct TaskSeed {
    binding_ids: Arc<BindingIdAllocator>,
    module_fields: Arc<ModuleFieldMap>,
    ufcs_methods: Arc<HashSet<UfcsMethod>>,
}

impl TaskSeed {
    pub(crate) fn spawn(&self) -> TaskState {
        let Self {
            binding_ids,
            module_fields,
            ufcs_methods,
        } = self;
        let mut task = TaskState::new(binding_ids.clone(), LocalSink::new());
        task.module_fields = module_fields.clone();
        task.ufcs_methods = ufcs_methods.clone();
        task
    }
}

/// Per-task mutable state. Paired with `AnalysisContext` (shared read-only view).
pub struct TaskState {
    pub env: TypeEnv,
    pub(crate) scopes: Scopes,
    pub cursor: Cursor,
    imports: ImportState,
    pub sink: LocalSink,
    pub facts: Facts,
    /// Recursion guard for interface satisfaction. Prevents
    /// `collect_interface_violations` from diverging when a bound on `T`
    /// transitively requires checking `T` against the same interface.
    satisfying_stack: rustc_hash::FxHashSet<(String, String)>,
    method_cache: HashMap<EcoString, Rc<MethodSignatures>>,
    /// Per-module projections. Workers share this copy-on-write, and the field
    /// definitions themselves remain `Arc`-shared if a worker adds an entry.
    module_fields: Arc<ModuleFieldMap>,
    /// Canonical UFCS set. Workers share it copy-on-write, so reads stay cheap
    /// while task-local additions are returned as part of their output.
    ufcs_methods: Arc<HashSet<UfcsMethod>>,
    /// Typed files produced by inference.
    pub typed_files: Vec<(String, File)>,
    /// Equality synthesis waits until registration has discovered all UFCS methods.
    pub(crate) pending_equality_attributes: Vec<DerivedAttribute>,
    pub(crate) pending_generic_bound_checks: Vec<(Type, Type, Span)>,
    /// Interface bounds on concrete type arguments named in annotations. Drained
    /// once after inference, since body annotations register during it.
    pub(crate) pending_interface_bound_checks: Vec<(Type, Type, Span)>,
}

impl TaskState {
    fn new(binding_ids: Arc<BindingIdAllocator>, sink: LocalSink) -> Self {
        Self {
            env: TypeEnv::new(),
            scopes: Scopes::new(),
            cursor: Cursor::new(),
            imports: ImportState::new(),
            sink,
            facts: Facts::new(binding_ids),
            satisfying_stack: rustc_hash::FxHashSet::default(),
            method_cache: HashMap::default(),
            module_fields: Arc::default(),
            ufcs_methods: Arc::default(),
            typed_files: Vec::new(),
            pending_equality_attributes: Vec::new(),
            pending_generic_bound_checks: Vec::new(),
            pending_interface_bound_checks: Vec::new(),
        }
    }

    pub fn with_fresh_allocator() -> Self {
        Self::new(Arc::new(BindingIdAllocator::new()), LocalSink::new())
    }

    pub(crate) fn with_sink(sink: LocalSink) -> Self {
        Self::new(Arc::new(BindingIdAllocator::new()), sink)
    }

    pub(crate) fn worker_seed(&self) -> TaskSeed {
        TaskSeed {
            binding_ids: self.facts.allocator(),
            module_fields: self.module_fields.clone(),
            ufcs_methods: self.ufcs_methods.clone(),
        }
    }

    pub fn ufcs_methods(&self) -> &HashSet<UfcsMethod> {
        &self.ufcs_methods
    }

    pub fn extend_ufcs_methods(&mut self, methods: impl IntoIterator<Item = UfcsMethod>) {
        Arc::make_mut(&mut self.ufcs_methods).extend(methods);
    }

    pub fn take_ufcs_methods(&mut self) -> HashSet<UfcsMethod> {
        Arc::unwrap_or_clone(std::mem::take(&mut self.ufcs_methods))
    }

    pub fn shared_ufcs_methods(&self) -> Arc<HashSet<UfcsMethod>> {
        self.ufcs_methods.clone()
    }

    pub(crate) fn into_output(self) -> TaskOutput {
        let Self {
            env: _,
            scopes: _,
            cursor: _,
            imports: _,
            sink,
            facts,
            satisfying_stack: _,
            method_cache: _,
            module_fields,
            ufcs_methods,
            typed_files,
            pending_equality_attributes,
            pending_generic_bound_checks,
            pending_interface_bound_checks,
        } = self;
        TaskOutput {
            facts,
            module_fields,
            ufcs_methods,
            typed_files,
            pending_equality_attributes,
            pending_generic_bound_checks,
            pending_interface_bound_checks,
            sink,
        }
    }

    pub(crate) fn absorb_outputs(&mut self, outputs: Vec<TaskOutput>) {
        let mut sinks = Vec::with_capacity(outputs.len());
        for output in outputs {
            let TaskOutput {
                facts,
                module_fields,
                ufcs_methods,
                typed_files,
                pending_equality_attributes,
                pending_generic_bound_checks,
                pending_interface_bound_checks,
                sink,
            } = output;
            if !Arc::ptr_eq(&self.module_fields, &module_fields) {
                Arc::make_mut(&mut self.module_fields).extend(Arc::unwrap_or_clone(module_fields));
            }
            if !Arc::ptr_eq(&self.ufcs_methods, &ufcs_methods) {
                Arc::make_mut(&mut self.ufcs_methods).extend(Arc::unwrap_or_clone(ufcs_methods));
            }
            self.facts.merge(facts);
            self.typed_files.extend(typed_files);
            self.pending_equality_attributes
                .extend(pending_equality_attributes);
            self.pending_generic_bound_checks
                .extend(pending_generic_bound_checks);
            self.pending_interface_bound_checks
                .extend(pending_interface_bound_checks);
            sinks.push(sink);
        }
        self.sink.extend(LocalSink::merge(sinks));
    }

    fn is_ufcs_method(&self, type_id: &str, method: &str) -> bool {
        self.ufcs_methods()
            .contains(&(type_id.to_string(), method.to_string()))
    }

    pub fn new_type_var(&mut self) -> Type {
        let id = self.env.fresh();
        Type::Var { id, hint: None }
    }

    fn new_type_var_with_hint(&mut self, hint: &str) -> Type {
        let hint: EcoString = hint.into();
        let id = self.env.fresh();
        Type::Var {
            id,
            hint: Some(hint),
        }
    }

    fn type_from_literal_expression(&mut self, expression: &Expression) -> Option<Type> {
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

    fn instantiate(&mut self, ty: &Type) -> (Type, SubstitutionMap) {
        match ty {
            Type::Forall { vars, body } => {
                let map: SubstitutionMap = vars
                    .iter()
                    .map(|name| {
                        let id = self.env.fresh();
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

    fn is_d_lis(&self, store: &Store) -> bool {
        let Some(file_id) = self.cursor.file_id else {
            return false;
        };

        let Some(module) = store.get_module(&self.cursor.module_id) else {
            return false;
        };

        module.is_typedef(file_id)
    }

    fn is_lis(&self, store: &Store) -> bool {
        !self.is_d_lis(store)
    }

    fn current_module<'a>(&self, store: &'a Store) -> &'a Module {
        store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store")
    }

    fn current_module_mut<'a>(&self, store: &'a mut Store) -> &'a mut Module {
        store
            .get_module_mut(&self.cursor.module_id)
            .expect("current module must exist in store")
    }

    fn qualify_name(&self, name: &str) -> Symbol {
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

    pub(crate) fn resolve_generic_bounds(
        &mut self,
        store: &Store,
        generics: &[Generic],
        span: &Span,
    ) -> Vec<Generic> {
        let mut resolved = generics.to_vec();
        for generic in &mut resolved {
            generic.resolve_bounds_with(|bound| self.register_bound_annotation(store, bound, span));
        }
        self.record_resolved_generic_bounds(&resolved);
        resolved
    }

    fn record_resolved_generic_bounds(&mut self, generics: &[Generic]) {
        for generic in generics {
            for bound in generic
                .resolved_bounds()
                .expect("generic bounds were resolved before recording")
            {
                self.record_generic_bound(&generic.name, bound.clone());
            }
        }
    }

    fn ensure_generic_bounds(
        &mut self,
        store: &Store,
        generics: Vec<Generic>,
        span: &Span,
    ) -> Vec<Generic> {
        if generics.iter().all(Generic::bounds_are_resolved) {
            self.record_resolved_generic_bounds(&generics);
            generics
        } else {
            self.resolve_generic_bounds(store, &generics, span)
        }
    }

    fn record_generic_bound(&mut self, parameter: &str, bound: Type) {
        let qualified_parameter = self.qualify_name(parameter);
        let bounds = self
            .scopes
            .current_mut()
            .trait_bounds
            .get_or_insert_with(HashMap::default)
            .entry(qualified_parameter)
            .or_default();
        if !bounds.contains(&bound) {
            bounds.push(bound);
        }
    }

    fn parameter_satisfies_bound(&self, parameter: &str, target: infer::BuiltinBound) -> bool {
        let mut found = false;
        self.scopes.for_each_bound_on_param(parameter, |bound_ty| {
            if found {
                return;
            }
            if let Some(declared) = bound_ty
                .resolve_in(&self.env)
                .get_qualified_id()
                .and_then(infer::BuiltinBound::from_qualified_id)
                && declared.satisfies(target)
            {
                found = true;
            }
        });
        found
    }

    fn register_bound_annotation(
        &mut self,
        store: &Store,
        bound: &Annotation,
        span: &Span,
    ) -> Type {
        let resolved = self.convert_bound_to_type(store, bound, span);
        if self.is_lis(store) && store.contains_unknown(&resolved) {
            self.sink
                .push(diagnostics::infer::unknown_in_bound_position(
                    bound.get_span(),
                ));
        }
        resolved
    }

    /// Resolve a simple name (e.g., "Sunday") to a public definition in an imported module.
    /// First tries direct match (`module_id.name`), then falls back to searching
    /// for nested definitions (e.g., `module_id.Weekday.Sunday`) preferring top-level
    /// over nested when both share the same simple name.
    fn resolve_in_imported_module<'m>(
        &self,
        store: &Store,
        module: &'m Module,
        simple_name: &str,
    ) -> Option<(String, &'m Definition)> {
        let module_prefix = format!("{}.", module.id);

        // Direct match: module_id.simple_name
        let direct = format!("{}{}", module_prefix, simple_name);
        if let Some(definition) = module.definitions.get(direct.as_str())
            && definition.visibility.is_public()
            && !store.is_test_definition(definition)
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
                && definition.visibility.is_public()
                && !store.is_test_definition(definition)
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

    fn lookup_qualified_name(&self, store: &Store, type_name: &str) -> Option<EcoString> {
        self.lookup_qualified_name_in_scope(store, type_name, false)
    }

    fn lookup_qualified_name_in_type_position(
        &self,
        store: &Store,
        type_name: &str,
    ) -> Option<EcoString> {
        self.lookup_qualified_name_in_scope(store, type_name, true)
    }

    /// Whether the file being checked is a `.test.lis` file.
    fn current_file_is_test(&self, store: &Store) -> bool {
        self.cursor
            .file_id
            .is_some_and(|file_id| store.test_file_ids.contains(&file_id))
    }

    /// A test-file definition is visible only to test files of the same module.
    fn test_definition_visible(
        &self,
        store: &Store,
        definition: &Definition,
        module_id: &str,
        in_test_file: bool,
    ) -> bool {
        !store.is_test_definition(definition)
            || (in_test_file && module_id == self.cursor.module_id)
    }

    fn lookup_qualified_name_in_scope(
        &self,
        store: &Store,
        type_name: &str,
        prefer_type: bool,
    ) -> Option<EcoString> {
        if let Some((prefix, simple_name)) = type_name.split_once('.')
            && let Some(module_id) = self.imports.module_id(prefix)
            && let Some(imported_module) = store.get_module(module_id)
            && let Some((qualified_name, _)) =
                self.resolve_in_imported_module(store, imported_module, simple_name)
        {
            return Some(qualified_name.into());
        }

        let in_test_file = self.current_file_is_test(store);
        let module_ids = std::iter::once(self.cursor.module_id.as_str())
            .chain(self.imports.unprefixed_imports.iter().map(String::as_str));

        let mut value_fallback: Option<EcoString> = None;
        for module_id in module_ids {
            let Some(module) = store.get_module(module_id) else {
                continue;
            };
            let qualified_name = Symbol::from_parts(module_id, type_name);
            let Some(definition) = module.definitions.get(qualified_name.as_str()) else {
                continue;
            };
            if !self.test_definition_visible(store, definition, module_id, in_test_file) {
                continue;
            }

            if prefer_type && definition.is_value(qualified_name.as_str()) {
                if value_fallback.is_none() {
                    value_fallback = Some(qualified_name.as_eco().clone());
                }
            } else {
                return Some(qualified_name.as_eco().clone());
            }
        }

        value_fallback
    }

    fn get_definition_name_span(&self, store: &Store, qualified_name: &str) -> Option<Span> {
        store.get_definition(qualified_name)?.name_span
    }

    fn is_const_name(&self, store: &Store, qualified_name: &str) -> bool {
        if qualified_name.starts_with("go:") {
            return false;
        }
        store.is_const(qualified_name)
    }

    fn is_const_var(&self, store: &Store, var_name: &str) -> bool {
        if self.scopes.lookup_value(var_name).is_some() {
            return self.scopes.lookup_const(var_name);
        }
        self.lookup_qualified_name(store, var_name)
            .is_some_and(|qname| self.is_const_name(store, &qname))
    }

    /// Track that `name` (at the start of `span`) refers to the definition at `qualified_name`.
    fn track_name_usage(
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

    fn lookup_generic_index(&self, type_name: &str) -> Option<usize> {
        self.scopes.lookup_type_param(type_name)
    }

    /// Resolves the value type for a definition. Returns the constructor type for
    /// structs with constructors (tuple structs) and for type aliases pointing to them.
    fn resolve_definition_value_type(&self, store: &Store, definition: &Definition) -> Type {
        if let Some(constructor_ty) = definition.constructor_type() {
            return constructor_ty;
        }

        // Type alias to tuple struct should return constructor type.
        if let DefinitionBody::TypeAlias { .. } = &definition.body {
            let underlying = store.peel_alias(&definition.ty);
            if let Type::Nominal { id, .. } = &underlying
                && let Some(constructor_ty) = store
                    .get_definition(id)
                    .and_then(Definition::constructor_type)
            {
                return constructor_ty;
            }
        }

        definition.ty.clone()
    }

    fn lookup_type(&self, store: &Store, value_name: &str) -> Option<Type> {
        if let Some(ty) = self.scopes.lookup_value(value_name) {
            return Some(ty.clone());
        }

        if let Some((module_id, _)) = self.imports.namespace(value_name) {
            return Some(Type::ImportNamespace(module_id.into()));
        }

        if let Some((prefix, rest)) = value_name.split_once('.')
            && let Some(module_id) = self.imports.module_id(prefix)
            && let Some(imported_module) = store.get_module(module_id)
            && let Some((_, definition)) =
                self.resolve_in_imported_module(store, imported_module, rest)
        {
            return Some(self.resolve_definition_value_type(store, definition));
        }

        let in_test_file = self.current_file_is_test(store);
        let module = store.get_module(&self.cursor.module_id)?;
        let qualified_name = Symbol::from_parts(&module.id, value_name);

        if let Some(definition) = module.definitions.get(qualified_name.as_str())
            && self.test_definition_visible(store, definition, &module.id, in_test_file)
        {
            return Some(self.resolve_definition_value_type(store, definition));
        }

        for imported_module_id in &self.imports.unprefixed_imports {
            if let Some(imported_module) = store.get_module(imported_module_id) {
                let qualified_name = Symbol::from_parts(imported_module_id, value_name);
                if let Some(definition) = imported_module.definitions.get(qualified_name.as_str())
                    && !store.is_test_definition(definition)
                {
                    return Some(self.resolve_definition_value_type(store, definition));
                }
            }
        }

        None
    }

    fn is_enum_type(&self, store: &Store, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        let Some(definition) = store.get_definition(id) else {
            return false;
        };
        matches!(definition.body, DefinitionBody::Enum { .. })
    }

    fn resolve_type_name(&mut self, store: &Store, type_name: &str) -> Option<(String, Type)> {
        if self.scopes.lookup_type_param(type_name).is_some() {
            return None;
        }

        let qualified_name = self.lookup_qualified_name_in_type_position(store, type_name)?;
        let ty = store.get_type(&qualified_name)?.clone();

        Some((qualified_name.to_string(), ty))
    }

    fn resolve_type_from_prelude(&self, store: &Store, type_name: &str) -> Option<(String, Type)> {
        let qualified_name = format!("prelude.{}", type_name);
        let ty = store.get_type(&qualified_name)?.clone();
        Some((qualified_name, ty))
    }

    fn get_all_methods(&mut self, store: &Store, ty: &Type) -> Rc<MethodSignatures> {
        if let Type::Parameter(name) = ty {
            let trait_bounds = self.scopes.collect_all_trait_bounds();
            let qualified_name = self.qualify_name(name);
            return Rc::new(store.get_methods_from_bounds(&qualified_name, &trait_bounds));
        }

        let resolved = ty.strip_refs().resolve_in(&self.env);
        let cache_key: EcoString = match &resolved {
            Type::Nominal { id, .. } => id.as_eco().clone(),
            Type::Compound { kind, .. } => format!("prelude.{}", kind.leaf_name()).into(),
            Type::Simple(kind) => format!("prelude.{}", kind.leaf_name()).into(),
            // Array methods live on the prelude `Array` impl.
            Type::Array { .. } => "prelude.Array".into(),
            _ => return Rc::new(MethodSignatures::default()),
        };

        // Interfaces need type-arg-dependent generic substitution, skip cache.
        let peeled = store.peel_alias(&resolved);
        if let Type::Nominal { id: peeled_id, .. } = &peeled
            && store.get_interface(peeled_id).is_some()
        {
            let empty = HashMap::default();
            return Rc::new(store.get_all_methods(&peeled, &empty));
        }

        let is_embedder = promotion::has_direct_embed(store, &resolved);
        let is_generic = matches!(&resolved, Type::Nominal { params, .. } if !params.is_empty());
        let cacheable = !(is_embedder && is_generic);

        if cacheable && let Some(cached) = self.method_cache.get(cache_key.as_str()) {
            return cached.clone();
        }

        let methods = if is_embedder {
            Rc::new(promotion::promoted_method_set(store, &resolved))
        } else {
            let empty = HashMap::default();
            Rc::new(store.get_all_methods(&resolved, &empty))
        };
        if cacheable {
            self.method_cache.insert(cache_key, methods.clone());
        }
        methods
    }

    fn with_module_cursor<T>(&mut self, module_id: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        if self.cursor.module_id == module_id {
            return f(self);
        }

        let previous_module_id = std::mem::replace(&mut self.cursor.module_id, module_id.into());
        let result = f(self);
        self.cursor.module_id = previous_module_id;
        result
    }

    fn with_file_context<T>(
        &mut self,
        store: &Store,
        module_id: &str,
        file_id: u32,
        imports: &[FileImport],
        kind: FileContextKind,
        f: impl FnOnce(&mut Self, &Store) -> T,
    ) -> T {
        self.with_module_cursor(module_id, |this| {
            let saved = this.enter_file_context(store, module_id, file_id, imports, kind);
            let result = f(this, store);
            this.exit_file_context(saved);
            result
        })
    }

    pub(crate) fn with_file_context_mut<T>(
        &mut self,
        store: &mut Store,
        module_id: &str,
        file_id: u32,
        imports: &[FileImport],
        kind: FileContextKind,
        f: impl FnOnce(&mut Self, &mut Store) -> T,
    ) -> T {
        self.with_module_cursor(module_id, |this| {
            let saved = this.enter_file_context(&*store, module_id, file_id, imports, kind);
            let result = f(this, store);
            this.exit_file_context(saved);
            result
        })
    }

    fn enter_file_context(
        &mut self,
        store: &Store,
        module_id: &str,
        file_id: u32,
        imports: &[FileImport],
        kind: FileContextKind,
    ) -> SavedFileContext {
        let saved = SavedFileContext {
            file_id: self.cursor.file_id.replace(file_id),
            scopes: std::mem::take(&mut self.scopes),
            imports: std::mem::take(&mut self.imports),
        };

        match kind {
            FileContextKind::Standard => {
                self.put_prelude_in_scope(store);
                if self.current_file_is_test(store) {
                    self.put_unprefixed_module_in_scope(
                        store,
                        crate::prelude::TEST_PRELUDE_MODULE_ID,
                    );
                }
                self.put_unprefixed_module_in_scope(store, module_id);
            }
            FileContextKind::ImportedTypedef => {
                self.put_prelude_in_scope(store);
                let self_alias = store
                    .go_package_names
                    .get(module_id)
                    .cloned()
                    .unwrap_or_else(|| go_import_default_name(module_id).to_string());
                self.imports.prefixed.insert(
                    self_alias,
                    PrefixedImport::LookupOnly {
                        module_id: module_id.into(),
                    },
                );
            }
            FileContextKind::Prelude => {
                self.put_unprefixed_module_in_scope(store, module_id);
            }
            FileContextKind::TestPrelude => {
                self.put_prelude_in_scope(store);
                self.put_unprefixed_module_in_scope(store, module_id);
            }
        }
        self.put_imported_modules_in_scope(store, imports);

        saved
    }

    fn exit_file_context(&mut self, saved: SavedFileContext) {
        self.scopes = saved.scopes;
        self.imports = saved.imports;
        self.cursor.file_id = saved.file_id;
    }

    pub fn failed(&self) -> bool {
        self.sink.has_errors()
    }

    pub fn put_prelude_in_scope(&mut self, store: &Store) {
        self.put_unprefixed_module_in_scope(store, "prelude");
        if self.imports.namespace("prelude").is_some() {
            return;
        }
        self.put_module_in_scope(store, "prelude", Some("prelude".to_string()));
    }

    fn put_unprefixed_module_in_scope(&mut self, store: &Store, module_id: &str) {
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

            if matches!(import.alias, Some(ImportAlias::Blank(_))) {
                continue;
            }

            let Some(effective) = import.effective_alias(&store.go_package_names) else {
                continue;
            };

            let (reserved, span) = match &import.alias {
                Some(ImportAlias::Named(alias, alias_span)) => {
                    (is_reserved_import_alias(alias), *alias_span)
                }
                _ => (
                    NativeTypeKind::from_name(&effective).is_some(),
                    import.name_span,
                ),
            };
            if reserved {
                self.sink
                    .push(diagnostics::infer::reserved_import_alias(&effective, span));
                continue;
            }

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
                self.imports
                    .prefixed
                    .insert(effective, PrefixedImport::Failed);
                continue;
            }

            self.put_module_in_scope(store, &import.name, Some(effective));
        }
    }

    fn module_struct_fields(
        &mut self,
        store: &Store,
        module: &Module,
    ) -> Arc<[StructFieldDefinition]> {
        if let Some(fields) = self.module_fields.get(module.id.as_str()) {
            return fields.clone();
        }

        let module_prefix = format!("{}.", module.id);
        let fields: Vec<StructFieldDefinition> = module
            .definitions
            .iter()
            .filter(|(qn, _)| module.is_public(qn))
            .filter(|(_, definition)| !store.is_test_definition(definition))
            .filter(|(qn, _)| {
                qn.strip_prefix(&module_prefix)
                    .is_some_and(|rest| !rest.contains('.'))
            })
            .map(|(qn, definition)| {
                let simple_name = qn
                    .strip_prefix(&module_prefix)
                    .expect("qualified_name must start with module prefix");
                let ty = definition
                    .constructor_type()
                    .unwrap_or_else(|| definition.ty.clone());
                StructFieldDefinition {
                    doc: None,
                    attributes: vec![],
                    visibility: AstVisibility::Public,
                    name: simple_name.into(),
                    name_span: Span::dummy(),
                    annotation: Annotation::Unknown,
                    ty,
                    embedded: false,
                }
            })
            .collect();

        let shared: Arc<[StructFieldDefinition]> = fields.into();
        Arc::make_mut(&mut self.module_fields).insert(module.id.clone().into(), shared.clone());
        shared
    }

    fn put_module_in_scope(&mut self, store: &Store, module_id: &str, prefix: Option<String>) {
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

        let module_struct_fields = self.module_struct_fields(store, module);

        self.imports.prefixed.insert(
            prefix,
            PrefixedImport::Namespace {
                module_id: imported_module_id,
                fields: module_struct_fields,
            },
        );
    }

    /// Run a closure speculatively: if it returns `Err`, all type variable
    /// bindings performed during the closure are rolled back.
    fn speculatively<T, E>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, E>) -> Result<T, E> {
        self.env.begin_speculation();
        let result = f(self);
        self.env.end_speculation(result.is_err());
        result
    }
}

pub(crate) fn resolved_generic_bounds(generics: &[Generic]) -> Vec<Bound> {
    generics
        .iter()
        .flat_map(|generic| {
            generic
                .resolved_bounds()
                .expect("generic bounds must be resolved before use")
                .cloned()
                .map(|ty| Bound {
                    param_name: generic.name.clone(),
                    generic: Type::Parameter(generic.name.clone()),
                    ty,
                })
        })
        .collect()
}

/// Returns `true` if the given name is reserved and cannot be used as an import alias.
///
/// Reserved names include Go keywords, Go predeclared identifiers, Go builtins,
/// Go type constraint names, and Lisette prelude symbols.
fn is_reserved_import_alias(name: &str) -> bool {
    if NativeTypeKind::from_name(name).is_some() {
        return true;
    }
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
        | "Ordered"
        | "Some"
        | "None"
        | "Ok"
        | "Err"
        // Lisette prelude functions not already covered by Go builtins above
        | "assert_type"
        | "imaginary"
    )
}
