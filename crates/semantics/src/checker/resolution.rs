use super::*;

#[derive(Debug, Default)]
pub(super) struct ImportState {
    pub(super) prefixed: HashMap<String, PrefixedImport>,
    /// Modules whose exports are available without prefix (current module and prelude)
    pub(super) unprefixed_imports: HashSet<String>,
}

#[derive(Debug)]
pub(super) enum PrefixedImport {
    Namespace {
        module_id: String,
    },
    /// A typedef's self-prefix resolves qualified names but is not itself a value.
    LookupOnly {
        module_id: String,
    },
    Failed,
}

impl ImportState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn module_id(&self, prefix: &str) -> Option<&str> {
        match self.prefixed.get(prefix)? {
            PrefixedImport::Namespace { module_id } | PrefixedImport::LookupOnly { module_id } => {
                Some(module_id)
            }
            PrefixedImport::Failed => None,
        }
    }

    pub(super) fn namespace(&self, prefix: &str) -> Option<&str> {
        match self.prefixed.get(prefix)? {
            PrefixedImport::Namespace { module_id } => Some(module_id),
            PrefixedImport::LookupOnly { .. } | PrefixedImport::Failed => None,
        }
    }

    pub(super) fn modules(&self) -> impl Iterator<Item = (&str, &str)> {
        self.prefixed.iter().filter_map(|(prefix, import)| {
            let module_id = match import {
                PrefixedImport::Namespace { module_id }
                | PrefixedImport::LookupOnly { module_id } => module_id,
                PrefixedImport::Failed => return None,
            };
            Some((prefix.as_str(), module_id.as_str()))
        })
    }

    pub(super) fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.prefixed.values().filter_map(|import| match import {
            PrefixedImport::Namespace { module_id } => Some(module_id.as_str()),
            PrefixedImport::LookupOnly { .. } | PrefixedImport::Failed => None,
        })
    }

    pub(super) fn is_failed(&self, prefix: &str) -> bool {
        matches!(self.prefixed.get(prefix), Some(PrefixedImport::Failed))
    }
}

impl TaskState {
    /// Resolve a simple name (e.g., "Sunday") to a public definition in an imported module.
    /// First tries direct match (`module_id.name`), then falls back to searching
    /// for nested definitions (e.g., `module_id.Weekday.Sunday`) preferring top-level
    /// over nested when both share the same simple name.
    pub(super) fn resolve_in_imported_module<'m>(
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
                // Only match if it's nested (contains a dot), direct was tried above
                if rest.contains('.') {
                    return Some((qn.to_string(), definition));
                }
            }
        }

        None
    }

    pub(super) fn lookup_qualified_name(
        &self,
        store: &Store,
        type_name: &str,
    ) -> Option<EcoString> {
        self.lookup_qualified_name_in_scope(store, type_name, false)
    }

    pub(super) fn lookup_qualified_name_in_type_position(
        &self,
        store: &Store,
        type_name: &str,
    ) -> Option<EcoString> {
        self.lookup_qualified_name_in_scope(store, type_name, true)
    }

    /// Whether the file being checked is a `.test.lis` file.
    pub(super) fn current_file_is_test(&self, store: &Store) -> bool {
        self.cursor
            .file_id
            .is_some_and(|file_id| store.is_test_file(file_id))
    }

    /// A test-file definition is visible only to test files of the same module.
    pub(super) fn test_definition_visible(
        &self,
        store: &Store,
        definition: &Definition,
        module_id: &str,
        in_test_file: bool,
    ) -> bool {
        !store.is_test_definition(definition)
            || (in_test_file && module_id == self.cursor.module_id)
    }

    pub(super) fn lookup_qualified_name_in_scope(
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

    pub(super) fn get_definition_name_span(
        &self,
        store: &Store,
        qualified_name: &str,
    ) -> Option<Span> {
        store.get_definition(qualified_name)?.name_span
    }

    pub(super) fn is_const_name(&self, store: &Store, qualified_name: &str) -> bool {
        if qualified_name.starts_with("go:") {
            return false;
        }
        store.is_const(qualified_name)
    }

    pub(super) fn is_const_var(&self, store: &Store, var_name: &str) -> bool {
        if self.scopes.lookup_value(var_name).is_some() {
            return self.scopes.lookup_const(var_name);
        }
        self.lookup_qualified_name(store, var_name)
            .is_some_and(|qname| self.is_const_name(store, &qname))
    }

    /// Track that `name` (at the start of `span`) refers to the definition at `qualified_name`.
    pub(super) fn track_name_usage(
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

    pub(super) fn lookup_generic_index(&self, type_name: &str) -> Option<usize> {
        self.scopes.lookup_type_param(type_name)
    }

    /// Resolves the value type for a definition. Returns the constructor type for
    /// structs with constructors (tuple structs) and for type aliases pointing to them.
    pub(super) fn resolve_definition_value_type(
        &self,
        store: &Store,
        definition: &Definition,
    ) -> Type {
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

    pub(super) fn lookup_type(&self, store: &Store, value_name: &str) -> Option<Type> {
        if let Some(ty) = self.scopes.lookup_value(value_name) {
            return Some(ty.clone());
        }

        if let Some(module_id) = self.imports.namespace(value_name) {
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

    pub(super) fn is_enum_type(&self, store: &Store, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        let Some(definition) = store.get_definition(id) else {
            return false;
        };
        matches!(definition.body, DefinitionBody::Enum { .. })
    }

    pub(super) fn resolve_type_name(
        &mut self,
        store: &Store,
        type_name: &str,
    ) -> Option<(String, Type)> {
        if self.scopes.lookup_type_param(type_name).is_some() {
            return None;
        }

        let qualified_name = self.lookup_qualified_name_in_type_position(store, type_name)?;
        let ty = store.get_type(&qualified_name)?.clone();

        Some((qualified_name.to_string(), ty))
    }

    pub(super) fn resolve_type_from_prelude(
        &self,
        store: &Store,
        type_name: &str,
    ) -> Option<(String, Type)> {
        let qualified_name = format!("prelude.{}", type_name);
        let ty = store.get_type(&qualified_name)?.clone();
        Some((qualified_name, ty))
    }

    pub(super) fn get_all_methods(&mut self, store: &Store, ty: &Type) -> MethodSignatures {
        if let Type::Parameter(name) = ty {
            let trait_bounds = self.scopes.collect_all_trait_bounds();
            let qualified_name = self.qualify_name(name);
            return store.get_methods_from_bounds(&qualified_name, &trait_bounds);
        }

        let resolved = ty.strip_refs().resolve_in(&self.env);
        match &resolved {
            Type::Nominal { .. } | Type::Compound { .. } | Type::Simple(_) | Type::Array { .. } => {
            }
            _ => return MethodSignatures::default(),
        }

        let peeled = store.peel_alias(&resolved);
        if let Type::Nominal { id, .. } = &peeled
            && store.get_interface(id).is_some()
        {
            let empty = HashMap::default();
            store.get_all_methods(&peeled, &empty)
        } else if promotion::has_direct_embed(store, &resolved) {
            promotion::promoted_method_set(store, &resolved)
        } else {
            let empty = HashMap::default();
            store.get_all_methods(&resolved, &empty)
        }
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

    pub(super) fn put_unprefixed_module_in_scope(&mut self, store: &Store, module_id: &str) {
        self.put_module_in_scope(store, module_id, None)
    }

    pub fn put_imported_modules_in_scope(&mut self, store: &Store, imports: &[FileImport]) {
        let mut seen_aliases: HashMap<String, String> = HashMap::default(); // alias -> path
        let mut seen_paths: HashSet<String> = HashSet::default();

        for import in imports {
            if seen_paths.contains(import.name.as_str()) {
                self.sink.push(diagnostics::infer::duplicate_import_path(
                    crate::loader::import_display_name(&import.name),
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
                    crate::loader::import_display_name(existing_path),
                    crate::loader::import_display_name(&import.name),
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

    pub(super) fn put_module_in_scope(
        &mut self,
        store: &Store,
        module_id: &str,
        prefix: Option<String>,
    ) {
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

        self.imports.prefixed.insert(
            prefix,
            PrefixedImport::Namespace {
                module_id: imported_module_id,
            },
        );
    }
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
