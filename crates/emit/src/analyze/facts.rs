use std::sync::Arc;

use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{BindingId, Pattern, RestPattern, Span};
use syntax::program::{
    Definition, DefinitionBody, EqualityIndex, ModuleId, MutationInfo, ResolvedDefinitions,
    TestIndex, UnusedInfo,
};
use syntax::types::{Symbol, Type};

use crate::abi::callable::CallableReturnAbi;
use crate::abi::catalog::GoSlotDescriptor;
use crate::classify_go_return_type;
use crate::context::lowering::LineIndex;
use crate::names::go_name;
use crate::{EmitOptions, GlobalEmitData};

pub(crate) struct EmitFactsConfig<'a> {
    pub(crate) definitions: &'a HashMap<Symbol, Definition>,
    pub(crate) const_names: &'a HashSet<Symbol>,
    pub(crate) unused: &'a UnusedInfo,
    pub(crate) mutations: &'a MutationInfo,
    pub(crate) ufcs_methods: &'a HashSet<(String, String)>,
    pub(crate) equality_index: &'a EqualityIndex,
    pub(crate) test_index: &'a TestIndex,
    pub(crate) go_package_names: &'a HashMap<String, String>,
    pub(crate) go_module_ids: &'a HashSet<String>,
    pub(crate) entry_module: ModuleId,
    pub(crate) entry_package_name: &'a str,
    pub(crate) go_module: String,
    pub(crate) options: EmitOptions,
    pub(crate) line_indexes: Arc<HashMap<u32, LineIndex>>,
    pub(crate) globals: Arc<GlobalEmitData>,
    pub(crate) resolved_definitions: &'a ResolvedDefinitions,
    pub(crate) current_module: ModuleId,
}

pub(crate) struct EmitFacts<'a> {
    definitions: &'a HashMap<Symbol, Definition>,
    const_names: &'a HashSet<Symbol>,
    unused: &'a UnusedInfo,
    mutations: &'a MutationInfo,
    ufcs_methods: &'a HashSet<(String, String)>,
    equality_index: &'a EqualityIndex,
    test_index: &'a TestIndex,
    go_package_names: &'a HashMap<String, String>,
    go_module_ids: &'a HashSet<String>,
    entry_module: ModuleId,
    entry_package_name: &'a str,
    go_module: String,
    options: EmitOptions,
    line_indexes: Arc<HashMap<u32, LineIndex>>,
    globals: Arc<GlobalEmitData>,
    pub(crate) resolved_definitions: &'a ResolvedDefinitions,
    current_module: ModuleId,
}

impl<'a> EmitFacts<'a> {
    pub(crate) fn new(config: EmitFactsConfig<'a>) -> Self {
        Self {
            definitions: config.definitions,
            const_names: config.const_names,
            unused: config.unused,
            mutations: config.mutations,
            ufcs_methods: config.ufcs_methods,
            equality_index: config.equality_index,
            test_index: config.test_index,
            go_package_names: config.go_package_names,
            go_module_ids: config.go_module_ids,
            entry_module: config.entry_module,
            entry_package_name: config.entry_package_name,
            go_module: config.go_module,
            options: config.options,
            line_indexes: config.line_indexes,
            globals: config.globals,
            resolved_definitions: config.resolved_definitions,
            current_module: config.current_module,
        }
    }

    pub(crate) fn module_for_qualified_name<'b>(&self, id: &'b str) -> Option<&'b str>
    where
        'a: 'b,
    {
        syntax::types::module_for_qualified_name(id, self.go_module_ids.iter().map(String::as_str))
    }

    pub(crate) fn definition(&self, id: &str) -> Option<&'a Definition> {
        self.definitions.get(id)
    }

    pub(crate) fn is_const(&self, qualified_name: &str) -> bool {
        self.const_names.contains(qualified_name)
    }

    pub(crate) fn iter_definitions(&self) -> impl Iterator<Item = (&'a Symbol, &'a Definition)> {
        self.definitions.iter()
    }

    pub(crate) fn classify_go_return_type(
        &self,
        return_ty: &Type,
        go_hints: &[String],
    ) -> Option<CallableReturnAbi> {
        classify_go_return_type(self.definitions, return_ty, go_hints)
    }

    pub(crate) fn peel_alias(&self, ty: &Type) -> Type {
        peel_alias(self.definitions, ty)
    }

    pub(crate) fn strip_and_peel(&self, ty: &Type) -> Type {
        self.peel_alias(&ty.strip_refs())
    }

    pub(crate) fn resolve_embed_target(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        loop {
            let next = self.peel_alias(&current.strip_refs());
            if next == current {
                return next;
            }
            current = next;
        }
    }

    pub(crate) fn as_interface(&self, ty: &Type) -> Option<String> {
        as_interface(self.definitions, ty)
    }

    pub(crate) fn is_interface(&self, ty: &Type) -> bool {
        as_interface(self.definitions, ty).is_some()
    }

    pub(crate) fn is_nilable_go_type(&self, ty: &Type) -> bool {
        is_nilable_go_type(self.definitions, ty)
    }

    pub(crate) fn is_nullable_option(&self, ty: &Type) -> bool {
        is_nullable_option(self.definitions, ty)
    }

    pub(crate) fn resolve_to_function_type(&self, ty: &Type) -> Option<Type> {
        resolve_to_function_type(self.definitions, ty)
    }

    pub(crate) fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        self.unused.is_unused_binding(pattern)
    }

    pub(crate) fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        self.unused.is_unused_rest_binding(rest)
    }

    pub(crate) fn is_unused_definition(&self, span: &Span) -> bool {
        self.unused.is_unused_definition(span)
    }

    pub(crate) fn unused_imports_for_current_module(&self) -> &'a HashSet<EcoString> {
        static EMPTY: std::sync::LazyLock<HashSet<EcoString>> =
            std::sync::LazyLock::new(HashSet::default);
        self.unused
            .imports_by_module
            .get(self.current_module.as_str())
            .unwrap_or(&EMPTY)
    }

    pub(crate) fn is_mutated(&self, id: BindingId) -> bool {
        self.mutations.is_mutated(id)
    }

    pub(crate) fn is_alias_mutated(&self, id: BindingId) -> bool {
        self.mutations.is_alias_mutated(id)
    }

    pub(crate) fn is_ufcs_method(&self, qualified_type: &str, method: &str) -> bool {
        self.ufcs_methods
            .contains(&(qualified_type.to_string(), method.to_string()))
    }

    pub(crate) fn usable_equals_from(&self, id: &str) -> bool {
        self.equality_index.usable_from(id, &self.current_module)
    }

    pub(crate) fn synthesizes_equals(&self, id: &str) -> bool {
        self.equality_index.is_synthesized(id)
    }

    pub(crate) fn is_test(&self, qualified_name: &str) -> bool {
        self.test_index.contains_qualified(qualified_name)
    }

    pub(crate) fn current_module(&self) -> &str {
        &self.current_module
    }

    pub(crate) fn set_current_module(&mut self, module_id: &str) {
        self.current_module = module_id.to_string();
    }

    pub(crate) fn is_current_module(&self, module: &str) -> bool {
        module == self.current_module.as_str()
    }

    pub(crate) fn is_foreign_module(&self, module: &str) -> bool {
        !self.is_current_module(module) && module != go_name::PRELUDE_MODULE
    }

    pub(crate) fn is_entry_module(&self, module: &str) -> bool {
        module == self.entry_module.as_str()
    }

    pub(crate) fn entry_package_name(&self) -> &str {
        self.entry_package_name
    }

    pub(crate) fn qualified_current(&self, name: &str) -> String {
        format!("{}.{}", self.current_module, name)
    }

    pub(crate) fn qualified_current_member(&self, ty: &str, member: &str) -> String {
        format!("{}.{}.{}", self.current_module, ty, member)
    }

    pub(crate) fn go_module(&self) -> &str {
        &self.go_module
    }

    pub(crate) fn go_import_path(&self, module: &str) -> String {
        if module == go_name::TEST_PRELUDE_MODULE {
            return go_name::TESTKIT_IMPORT_PATH.to_string();
        }
        format!("{}/{}", self.go_module, module)
    }

    pub(crate) fn go_package_name(&self, module: &str) -> Option<&str> {
        self.go_package_names.get(module).map(String::as_str)
    }

    pub(crate) fn go_package_names(&self) -> &'a HashMap<String, String> {
        self.go_package_names
    }

    pub(crate) fn go_module_ids(&self) -> &'a HashSet<String> {
        self.go_module_ids
    }

    pub(crate) fn has_global_exported_method_name(&self, method: &str) -> bool {
        self.globals.exported_method_names.contains(method)
    }

    pub(crate) fn make_function_name(&self, key: &str) -> Option<&str> {
        self.globals
            .make_function_names
            .get(key)
            .map(String::as_str)
    }

    pub(crate) fn go_callable_return(&self, qualified_name: &str) -> Option<&CallableReturnAbi> {
        self.globals
            .go_abi_catalog
            .callable_return_abi(qualified_name)
    }

    pub(crate) fn go_callable_parameter(
        &self,
        qualified_name: &str,
        index: usize,
    ) -> Option<&GoSlotDescriptor> {
        self.globals
            .go_abi_catalog
            .callable_parameter(qualified_name, index)
    }

    pub(crate) fn go_callable_return_slot(
        &self,
        qualified_name: &str,
    ) -> Option<&GoSlotDescriptor> {
        self.globals
            .go_abi_catalog
            .callable_return_slot(qualified_name)
    }

    pub(crate) fn go_field(&self, owner: &str, field: &str) -> Option<&GoSlotDescriptor> {
        self.globals.go_abi_catalog.field(owner, field)
    }

    pub(crate) fn is_go_imported_type(&self, qualified_name: &str) -> bool {
        self.globals.go_abi_catalog.is_imported_type(qualified_name)
    }

    pub(crate) fn sourcemap_enabled(&self) -> bool {
        self.options.sourcemap
    }

    pub(crate) fn emit_tests_enabled(&self) -> bool {
        self.options.emit_tests
    }

    pub(crate) fn line_index(&self, file_id: u32) -> Option<&LineIndex> {
        self.line_indexes.get(&file_id)
    }
}

pub(crate) fn is_nullable_option(definitions: &HashMap<Symbol, Definition>, ty: &Type) -> bool {
    ty.is_option() && is_nilable_go_type(definitions, &ty.ok_type())
}

pub(crate) fn is_nilable_go_type(definitions: &HashMap<Symbol, Definition>, ty: &Type) -> bool {
    syntax::types::is_nilable_go_type(ty, |id| definitions.get(id))
}

pub(crate) fn as_interface(definitions: &HashMap<Symbol, Definition>, ty: &Type) -> Option<String> {
    let Type::Nominal { id, .. } = peel_alias(definitions, ty) else {
        return None;
    };
    matches!(
        definitions.get(id.as_str()).map(|d| &d.body),
        Some(DefinitionBody::Interface { .. })
    )
    .then(|| id.to_string())
}

pub(crate) fn resolve_to_function_type(
    definitions: &HashMap<Symbol, Definition>,
    ty: &Type,
) -> Option<Type> {
    fn as_function(ty: &Type) -> Option<Type> {
        if matches!(ty, Type::Function(_)) {
            return Some(ty.clone());
        }
        ty.get_underlying()
            .filter(|u| matches!(u, Type::Function(_)))
            .cloned()
    }
    as_function(ty).or_else(|| as_function(&peel_alias(definitions, ty)))
}

pub(crate) fn peel_alias(definitions: &HashMap<Symbol, Definition>, ty: &Type) -> Type {
    syntax::types::peel_alias(ty, |id| {
        definitions.get(id).is_some_and(Definition::is_type_alias)
    })
}
