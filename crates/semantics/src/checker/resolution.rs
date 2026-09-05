use super::*;
use crate::loader;
use std::iter;

#[derive(Debug, Default)]
pub(super) struct ImportState {
    pub(super) prefixed: HashMap<String, PrefixedImport>,
    /// Packages whose exports are available without prefix (current package and prelude)
    pub(super) unprefixed_imports: HashSet<String>,
    imported_resolutions: HashMap<EcoString, HashMap<EcoString, Option<Symbol>>>,
}

#[derive(Debug)]
pub(super) enum PrefixedImport {
    Namespace {
        package_id: String,
    },
    /// A typedef's self-prefix resolves qualified names but is not itself a value.
    LookupOnly {
        package_id: String,
    },
    Failed,
}

impl ImportState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn package_id(&self, prefix: &str) -> Option<&str> {
        match self.prefixed.get(prefix)? {
            PrefixedImport::Namespace { package_id }
            | PrefixedImport::LookupOnly { package_id } => Some(package_id),
            PrefixedImport::Failed => None,
        }
    }

    pub(super) fn namespace(&self, prefix: &str) -> Option<&str> {
        match self.prefixed.get(prefix)? {
            PrefixedImport::Namespace { package_id } => Some(package_id),
            PrefixedImport::LookupOnly { .. } | PrefixedImport::Failed => None,
        }
    }

    pub(super) fn packages(&self) -> impl Iterator<Item = (&str, &str)> {
        self.prefixed.iter().filter_map(|(prefix, import)| {
            let package_id = match import {
                PrefixedImport::Namespace { package_id }
                | PrefixedImport::LookupOnly { package_id } => package_id,
                PrefixedImport::Failed => return None,
            };
            Some((prefix.as_str(), package_id.as_str()))
        })
    }

    pub(super) fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.prefixed.values().filter_map(|import| match import {
            PrefixedImport::Namespace { package_id } => Some(package_id.as_str()),
            PrefixedImport::LookupOnly { .. } | PrefixedImport::Failed => None,
        })
    }

    pub(super) fn is_failed(&self, prefix: &str) -> bool {
        matches!(self.prefixed.get(prefix), Some(PrefixedImport::Failed))
    }
}

impl TaskState {
    pub(super) fn may_name_uninferred_export(&self, store: &Store, name: &str) -> bool {
        let Some((prefix, member)) = name.split_once('.') else {
            return false;
        };
        self.imports
            .package_id(prefix)
            .is_some_and(|package_id| store.uninferred_package_may_export(package_id, member))
    }

    /// Resolve a simple name (e.g., "Sunday") to a public definition in an imported package.
    /// First tries direct match (`package_id.name`), then falls back to searching
    /// for nested definitions (e.g., `package_id.Weekday.Sunday`) preferring top-level
    /// over nested when both share the same simple name.
    pub(super) fn resolve_in_imported_package<'m>(
        &mut self,
        store: &Store,
        package: &'m Package,
        simple_name: &str,
    ) -> Option<(String, &'m Definition)> {
        if let Some(cached) = self
            .imports
            .imported_resolutions
            .get(package.id.as_str())
            .and_then(|resolutions| resolutions.get(simple_name))
        {
            let qualified_name = cached.as_ref()?;
            let definition = package.definitions.get(qualified_name)?;
            return Some((qualified_name.to_string(), definition));
        }

        // Direct match: package_id.simple_name
        let direct = Symbol::from_parts(&package.id, simple_name);
        let package_prefix = format!("{}.", package.id);
        let resolved = if let Some(definition) = package.definitions.get(direct.as_str())
            && definition.visibility.is_public()
            && !store.is_test_definition(definition)
        {
            Some(direct)
        } else {
            let suffix = format!(".{}", simple_name);
            package
                .definitions
                .iter()
                .find(|(qualified_name, definition)| {
                    qualified_name.ends_with(suffix.as_str())
                        && qualified_name.starts_with(package_prefix.as_str())
                        && definition.visibility.is_public()
                        && !store.is_test_definition(definition)
                        && qualified_name[package_prefix.len()..].contains('.')
                })
                .map(|(qualified_name, _)| qualified_name.clone())
        };

        let result = resolved.as_ref().and_then(|qualified_name| {
            package
                .definitions
                .get(qualified_name)
                .map(|definition| (qualified_name.to_string(), definition))
        });
        self.imports
            .imported_resolutions
            .entry(package.id.as_str().into())
            .or_default()
            .insert(simple_name.into(), resolved);
        result
    }

    pub(super) fn lookup_qualified_name(
        &mut self,
        store: &Store,
        type_name: &str,
    ) -> Option<EcoString> {
        self.lookup_qualified_name_in_scope(store, type_name, false)
    }

    pub(super) fn lookup_qualified_name_in_type_position(
        &mut self,
        store: &Store,
        type_name: &str,
    ) -> Option<EcoString> {
        self.lookup_qualified_name_in_scope(store, type_name, true)
    }

    /// Whether the file being checked is a `.test.lis` file.
    pub(super) fn current_file_is_test(&self, store: &Store) -> bool {
        self.cursor
            .file_id()
            .is_some_and(|file_id| store.is_test_file(file_id))
    }

    /// A test-file definition is visible only to test files of the same package.
    pub(super) fn test_definition_visible(
        &self,
        store: &Store,
        definition: &Definition,
        package_id: &str,
        in_test_file: bool,
    ) -> bool {
        !store.is_test_definition(definition)
            || (in_test_file && package_id == self.cursor.package_id())
    }

    pub(super) fn lookup_qualified_name_in_scope(
        &mut self,
        store: &Store,
        type_name: &str,
        prefer_type: bool,
    ) -> Option<EcoString> {
        if let Some((prefix, simple_name)) = type_name.split_once('.')
            && let Some(package_id) = self.imports.package_id(prefix)
            && let Some(imported_package) = store.get_package(package_id)
            && let Some((qualified_name, _)) =
                self.resolve_in_imported_package(store, imported_package, simple_name)
        {
            return Some(qualified_name.into());
        }

        let in_test_file = self.current_file_is_test(store);
        let package_ids = iter::once(self.cursor.package_id())
            .chain(self.imports.unprefixed_imports.iter().map(String::as_str));

        let mut value_fallback: Option<EcoString> = None;
        for package_id in package_ids {
            let Some(package) = store.get_package(package_id) else {
                continue;
            };
            let qualified_name = Symbol::from_parts(package_id, type_name);
            let Some(definition) = package.definitions.get(qualified_name.as_str()) else {
                continue;
            };
            if !self.test_definition_visible(store, definition, package_id, in_test_file) {
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

    pub(super) fn is_const_var(&mut self, store: &Store, var_name: &str) -> bool {
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

    pub(super) fn lookup_type(&mut self, store: &Store, value_name: &str) -> Option<Type> {
        if let Some(ty) = self.scopes.lookup_value(value_name) {
            return Some(ty.clone());
        }

        if let Some(package_id) = self.imports.namespace(value_name) {
            return Some(Type::ImportNamespace(package_id.into()));
        }

        if let Some((prefix, rest)) = value_name.split_once('.')
            && let Some(imported_package) = self
                .imports
                .package_id(prefix)
                .and_then(|package_id| store.get_package(package_id))
        {
            if let Some((_, definition)) =
                self.resolve_in_imported_package(store, imported_package, rest)
            {
                return Some(self.resolve_definition_value_type(store, definition));
            }
            if let Some((owner, method)) = rest.rsplit_once('.') {
                let owner = Symbol::from_parts(&imported_package.id, owner);
                if let Some(method) = store.get_method(&owner, method) {
                    return Some(method.ty.clone());
                }
            }
        }

        let in_test_file = self.current_file_is_test(store);
        let package = store.get_package(self.cursor.package_id())?;
        let qualified_name = Symbol::from_parts(&package.id, value_name);

        if let Some(definition) = package.definitions.get(qualified_name.as_str())
            && self.test_definition_visible(store, definition, &package.id, in_test_file)
        {
            return Some(self.resolve_definition_value_type(store, definition));
        }
        if let Some((owner, method)) = value_name.rsplit_once('.') {
            let owner = Symbol::from_parts(&package.id, owner);
            if let Some(method) = store.get_method(&owner, method) {
                return Some(method.ty.clone());
            }
        }

        for imported_package_id in &self.imports.unprefixed_imports {
            if let Some(imported_package) = store.get_package(imported_package_id) {
                let qualified_name = Symbol::from_parts(imported_package_id, value_name);
                if let Some(definition) = imported_package.definitions.get(qualified_name.as_str())
                    && !store.is_test_definition(definition)
                {
                    return Some(self.resolve_definition_value_type(store, definition));
                }
                if let Some((owner, method)) = value_name.rsplit_once('.') {
                    let owner = Symbol::from_parts(imported_package_id, owner);
                    if let Some(method) = store.get_method(&owner, method) {
                        return Some(method.ty.clone());
                    }
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

    pub(super) fn get_all_methods(&mut self, store: &Store, ty: &Type) -> Methods {
        if let Type::Parameter(name) = ty {
            let trait_bounds = self.scopes.collect_all_trait_bounds();
            let qualified_name = self.qualify_name(name);
            return store.get_methods_from_bounds(&qualified_name, trait_bounds);
        }

        let resolved = ty.strip_refs().resolve_in(&self.env);
        match &resolved {
            Type::Nominal { .. } | Type::Compound { .. } | Type::Simple(_) | Type::Array { .. } => {
            }
            _ => return Methods::default(),
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

    pub(super) fn method_of_type(
        &mut self,
        store: &Store,
        ty: &Type,
        name: &str,
    ) -> Option<Method> {
        if let Type::Parameter(parameter) = ty {
            let trait_bounds = self.scopes.collect_all_trait_bounds();
            let qualified_name = self.qualify_name(parameter);
            return store.get_method_from_bounds(&qualified_name, trait_bounds, name);
        }

        let resolved = ty.strip_refs().resolve_in(&self.env);
        match &resolved {
            Type::Nominal { .. } | Type::Compound { .. } | Type::Simple(_) | Type::Array { .. } => {
            }
            _ => return None,
        }

        let peeled = store.peel_alias(&resolved);
        if let Type::Nominal { id, .. } = &peeled
            && store.get_interface(id).is_some()
        {
            let empty = HashMap::default();
            store.get_method_for_type(&peeled, &empty, name)
        } else if promotion::has_direct_embed(store, &resolved) {
            promotion::promoted_method(store, &resolved, name)
        } else {
            let empty = HashMap::default();
            store.get_method_for_type(&resolved, &empty, name)
        }
    }

    pub fn failed(&self) -> bool {
        self.sink.has_errors()
    }

    pub fn put_prelude_in_scope(&mut self, store: &Store) {
        self.put_unprefixed_package_in_scope(store, "prelude");
        if self.imports.namespace("prelude").is_some() {
            return;
        }
        self.put_package_in_scope(store, "prelude", Some("prelude".to_string()));
    }

    pub(super) fn put_unprefixed_package_in_scope(&mut self, store: &Store, package_id: &str) {
        self.put_package_in_scope(store, package_id, None)
    }

    pub fn put_imported_packages_in_scope(&mut self, store: &Store, imports: &[FileImport]) {
        let mut seen_aliases: HashMap<String, String> = HashMap::default(); // alias -> path
        let mut seen_paths: HashSet<String> = HashSet::default();

        for import in imports {
            if seen_paths.contains(import.name.as_str()) {
                self.sink.push(diagnostics::infer::duplicate_import_path(
                    loader::import_display_name(&import.name),
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
                    loader::import_display_name(existing_path),
                    loader::import_display_name(&import.name),
                    import.name_span,
                ));
                continue;
            }

            seen_aliases.insert(effective.clone(), import.name.to_string());

            let package = store.get_package(&import.name);
            if package.is_none() || package.is_some_and(Package::is_empty_stub) {
                self.imports
                    .prefixed
                    .insert(effective, PrefixedImport::Failed);
                continue;
            }

            self.put_package_in_scope(store, &import.name, Some(effective));
        }
    }

    pub(super) fn put_package_in_scope(
        &mut self,
        store: &Store,
        package_id: &str,
        prefix: Option<String>,
    ) {
        let Some(prefix) = prefix else {
            self.imports
                .unprefixed_imports
                .insert(package_id.to_string());
            return;
        };

        let package = store
            .get_package(package_id)
            .expect("package must exist when putting in scope");

        let imported_package_id = package.id.clone();

        self.imports.prefixed.insert(
            prefix,
            PrefixedImport::Namespace {
                package_id: imported_package_id,
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
