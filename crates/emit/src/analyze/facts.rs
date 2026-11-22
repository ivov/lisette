use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::program::{
    CallOperation, CheckedExpression, CheckedValueUse, EqualityRecipe, FieldSelection, ValueUseKey,
};

use ecow::EcoString;
use syntax::ast::{BindingId, Expression, Pattern, RestPattern, Span};
use syntax::program::{
    AliasKind, Definition, DefinitionBody, EmitInput, ImportKey, Method, PackageId,
};
use syntax::types;
use syntax::types::{CompoundKind, SimpleKind};
use syntax::types::{Symbol, Type};

use crate::EnumLayout;
use crate::SharedEmitContext;
use crate::abi::catalog::{GoAbiCatalog, GoSlotDescriptor};
use crate::abi::is_prelude_container_type;
use crate::names::go_name;
use crate::state::lowering::LineIndex;
use crate::types::go_type::returns_go_void;

#[derive(Clone)]
pub(crate) struct EmitFacts<'a> {
    input: &'a EmitInput,
    shared: &'a SharedEmitContext<'a>,
    current_package: PackageId,
}

impl<'a> EmitFacts<'a> {
    pub(crate) fn new(
        input: &'a EmitInput,
        shared: &'a SharedEmitContext<'a>,
        current_package: PackageId,
    ) -> Self {
        Self {
            input,
            shared,
            current_package,
        }
    }

    pub(crate) fn package_for_qualified_name<'b>(&self, id: &'b str) -> Option<&'b str>
    where
        'a: 'b,
    {
        types::package_for_qualified_name(id, |package| self.input.go_package_ids.contains(package))
    }

    pub(crate) fn definition(&self, id: &str) -> Option<&'a Definition> {
        self.input.definitions.get(id)
    }

    pub(crate) fn call_operation(&self, expression: &Expression) -> Option<&'a CallOperation> {
        let Expression::Call {
            expression: callee, ..
        } = expression
        else {
            return None;
        };
        Some(&self.checked(callee).callee().operation)
    }

    pub(crate) fn checked<'e>(&self, expression: &'e Expression) -> CheckedExpression<'e, 'a> {
        self.input.checked_bodies.occurrence(expression)
    }

    pub(crate) fn value_use_slot(&self, expression: &Expression) -> Option<&'a Type> {
        self.input
            .checked_bodies
            .value_use(&ValueUseKey::value(expression))
            .map(|value_use| &value_use.slot)
            .filter(|slot| !slot.is_ignored())
    }

    pub(crate) fn value_uses(
        &self,
    ) -> impl Iterator<Item = (&'a ValueUseKey, &'a CheckedValueUse)> {
        self.input.checked_bodies.uses()
    }

    pub(crate) fn pattern_field(&self, name_span: Span) -> &'a FieldSelection {
        self.input.checked_bodies.pattern_field(name_span)
    }

    pub(crate) fn variant_pattern_fields(&self, pattern_span: Span) -> Vec<&'a FieldSelection> {
        self.input
            .checked_bodies
            .variant_pattern_fields(pattern_span)
    }

    pub(crate) fn method(&self, owner: &str, name: &str) -> Option<&'a Method> {
        self.definition(owner)?.methods()?.get(name)
    }

    pub(crate) fn const_emits_go_const(&self, symbol: &str) -> bool {
        self.definition(symbol)
            .is_some_and(Definition::is_compile_time_const)
    }

    pub(crate) fn is_const(&self, qualified_name: &str) -> bool {
        self.definition(qualified_name)
            .is_some_and(Definition::is_const)
    }

    pub(crate) fn peel_alias(&self, ty: &Type) -> Type {
        peel_alias(&self.input.definitions, ty)
    }

    pub(crate) fn alias_emits_nothing(&self, name: &str) -> bool {
        self.type_parameter_alias_index(&self.qualified_current(name))
            .is_some()
    }

    pub(crate) fn type_parameter_alias_index(&self, id: &str) -> Option<usize> {
        self.type_parameter_alias_index_inner(id, &mut HashSet::default())
    }

    fn type_parameter_alias_index_inner(
        &self,
        id: &str,
        visiting: &mut HashSet<String>,
    ) -> Option<usize> {
        let DefinitionBody::TypeAlias {
            generics,
            alias: AliasKind::Transparent { target, .. },
            ..
        } = &self.definition(id)?.body
        else {
            return None;
        };
        if !visiting.insert(id.to_string()) {
            return None;
        }
        let mut target = target;
        while let Type::Nominal { id, params, .. } = target {
            let index = self.type_parameter_alias_index_inner(id, visiting)?;
            target = params.get(index)?;
        }
        visiting.remove(id);
        let Type::Parameter(name) = target else {
            return None;
        };
        generics.iter().position(|generic| generic.name == *name)
    }

    pub(crate) fn underlying_type(&self, ty: &Type) -> Option<Type> {
        types::underlying_type(ty, |id| self.definition(id))
    }

    pub(crate) fn underlying_simple_kind(&self, ty: &Type) -> Option<SimpleKind> {
        types::underlying_simple_kind(ty, |id| self.definition(id))
    }

    pub(crate) fn resolves_to_unknown(&self, ty: &Type) -> bool {
        types::resolves_to_unknown(ty, |id| self.definition(id))
    }

    pub(crate) fn resolves_to_unit(&self, ty: &Type) -> bool {
        types::resolves_to_unit(ty, |id| self.definition(id))
    }

    pub(crate) fn is_prelude_error(&self, ty: &Type) -> bool {
        types::is_prelude_error(ty, |id| self.definition(id))
    }

    pub(crate) fn has_no_go_value(&self, ty: &Type) -> bool {
        self.resolves_to_unit(ty) || ty.is_variable() || ty.is_placeholder()
    }

    pub(crate) fn is_lisette_enum(&self, ty: &Type) -> bool {
        types::is_lisette_enum(ty, |id| self.definition(id))
    }

    pub(crate) fn go_zero_is_lisette_zero(&self, ty: &Type) -> bool {
        let ty = self.peel_alias(ty);
        match &ty {
            Type::Simple(_) | Type::Parameter(_) => true,
            Type::Compound { kind, .. } => {
                matches!(kind, CompoundKind::Slice | CompoundKind::EnumeratedSlice)
            }
            Type::Tuple(elements) => elements.iter().all(|e| self.go_zero_is_lisette_zero(e)),
            Type::Array { length, element } => {
                *length == 0 || self.go_zero_is_lisette_zero(element)
            }
            Type::Nominal { id, .. } => {
                if self.is_option(&ty)
                    || self.is_lisette_enum(&ty)
                    || go_name::is_go_import(id.as_str())
                        && (self.is_interface(&ty) || self.resolve_to_function_type(&ty).is_some())
                {
                    return true;
                }
                if let Some(underlying) = self.newtype_underlying(&ty) {
                    return self.go_zero_is_lisette_zero(&underlying);
                }
                if matches!(
                    self.definition(id.as_str()).map(|d| &d.body),
                    Some(DefinitionBody::Struct { .. })
                ) && let Some(fields) = self.struct_fields(&ty)
                {
                    return fields
                        .iter()
                        .all(|(_, selection)| self.go_zero_is_lisette_zero(&selection.field_ty()));
                }
                self.underlying_type(&ty)
                    .is_some_and(|underlying| self.go_zero_is_lisette_zero(&underlying))
            }
            _ => false,
        }
    }

    pub(crate) fn converts(&self, expression: &Expression) -> bool {
        self.checked(expression).value_use().adaptation.converts()
    }

    pub(crate) fn is_reference(&self, ty: &Type) -> bool {
        types::is_reference(ty, |id| self.definition(id))
    }

    pub(crate) fn points_to_ref(&self, ty: &Type) -> bool {
        types::points_to_ref(ty, |id| self.definition(id))
    }

    pub(crate) fn is_option(&self, ty: &Type) -> bool {
        types::is_option(ty, |id| self.definition(id))
    }

    pub(crate) fn is_partial(&self, ty: &Type) -> bool {
        types::is_partial(ty, |id| self.definition(id))
    }

    pub(crate) fn is_tuple(&self, ty: &Type) -> bool {
        types::is_tuple(ty, |id| self.definition(id))
    }

    pub(crate) fn ok_payload(&self, ty: &Type) -> Type {
        types::ok_payload(ty, |id| self.definition(id))
    }

    pub(crate) fn err_payload(&self, ty: &Type) -> Type {
        types::err_payload(ty, |id| self.definition(id))
    }

    pub(crate) fn is_never(&self, ty: &Type) -> bool {
        self.peel_alias(ty).is_never()
    }

    pub(crate) fn is_result(&self, ty: &Type) -> bool {
        self.peel_alias(ty).is_result()
    }

    pub(crate) fn simple_kind(&self, ty: &Type) -> Option<SimpleKind> {
        types::simple_kind(ty, |id| self.definition(id))
    }

    pub(crate) fn without_references(&self, ty: &Type) -> Type {
        ty.strip_refs()
    }

    pub(crate) fn compound_parts(&self, ty: &Type) -> Option<(CompoundKind, Vec<Type>)> {
        match self.peel_alias(ty) {
            Type::Compound { kind, args, .. } => Some((kind, args)),
            _ => None,
        }
    }

    pub(crate) fn peel_go_alias(&self, ty: &Type) -> Type {
        let mut current = ty.unwrap_forall().clone();
        let mut seen: HashSet<Symbol> = HashSet::default();
        while let Type::Nominal {
            id,
            params,
            writable,
        } = &current
        {
            if self.declares_go_function_type(&current) {
                break;
            }
            let Some(target) = self
                .definition(id)
                .and_then(|definition| definition.instantiate_alias_target(params, *writable))
            else {
                break;
            };
            if !seen.insert(id.clone()) {
                break;
            }
            current = target.unwrap_forall().clone();
        }
        current
    }

    pub(crate) fn without_go_aliases(&self, ty: &Type) -> Type {
        self.peel_go_alias(ty)
            .map_children(|child| self.without_go_aliases(child))
    }

    pub(crate) fn is_transparent_alias(&self, ty: &Type) -> bool {
        let peeled = self.peel_go_alias(ty);
        peeled != *ty.unwrap_forall() && peeled == self.peel_alias(ty)
    }

    pub(crate) fn is_go_named_function_type(&self, ty: &Type) -> bool {
        self.declares_go_function_type(&self.peel_go_alias(ty))
    }

    fn declares_go_function_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Nominal { id, .. } if go_name::is_go_import(id))
            && self.resolve_to_function_type(ty).is_some()
    }

    pub(crate) fn same_type(&self, left: &Type, right: &Type) -> bool {
        left == right || self.without_go_aliases(left) == self.without_go_aliases(right)
    }

    pub(crate) fn same_types(&self, left: &[Type], right: &[Type]) -> bool {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(left, right)| self.same_type(left, right))
    }

    pub(crate) fn returns_go_void(&self, ty: &Type) -> bool {
        returns_go_void(&self.peel_alias(ty))
    }

    pub(crate) fn is_reference_underneath(&self, ty: &Type) -> bool {
        self.underlying(ty).is_ref()
    }

    pub(crate) fn array_length_behind_refs(&self, ty: &Type) -> Option<u64> {
        match self.receiver_shape(ty) {
            Type::Array { length, .. } => Some(length),
            _ => None,
        }
    }

    pub(crate) fn tuple_elements(&self, ty: &Type) -> Option<Vec<Type>> {
        match self.peel_alias(ty) {
            Type::Tuple(elements) => Some(elements),
            _ => None,
        }
    }

    pub(crate) fn clone_container(&self, ty: &Type) -> Option<Type> {
        let shape = self.receiver_shape(ty);
        self.is_clone_container(&shape).then_some(shape)
    }

    pub(crate) fn array_parts(&self, ty: &Type) -> Option<(u64, Type)> {
        match self.peel_alias(ty) {
            Type::Array { length, element } => Some((length, *element)),
            _ => None,
        }
    }

    pub(crate) fn prelude_container_arguments(&self, ty: &Type) -> Option<Vec<Type>> {
        match self.peel_alias(ty) {
            peeled @ Type::Nominal { .. } if is_prelude_container_type(&peeled) => {
                peeled.get_type_params().map(<[Type]>::to_vec)
            }
            _ => None,
        }
    }

    pub(crate) fn is_lowered_result_type(&self, ty: &Type) -> bool {
        self.prelude_container_arguments(ty).is_some()
            || self
                .tuple_elements(ty)
                .is_some_and(|elements| elements.len() >= 2)
    }

    pub(crate) fn tuple_behind_refs(&self, ty: &Type) -> Option<Vec<Type>> {
        match self.receiver_shape(ty) {
            Type::Tuple(elements) => Some(elements),
            _ => None,
        }
    }

    pub(crate) fn receiver_nominal_id(&self, ty: &Type) -> Option<Symbol> {
        match self.receiver_shape(ty) {
            Type::Nominal { id, .. } => Some(id),
            _ => None,
        }
    }

    pub(crate) fn type_parameter_behind_refs(&self, ty: &Type) -> Option<EcoString> {
        match ty.strip_refs() {
            Type::Parameter(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn is_ref_to_type_parameter(&self, ty: &Type) -> bool {
        let peeled = self.peel_alias(ty);
        peeled.is_ref()
            && peeled
                .inner()
                .is_some_and(|inner| matches!(self.peel_alias(&inner), Type::Parameter(_)))
    }

    pub(crate) fn is_option_or_result(&self, ty: &Type) -> bool {
        types::is_option_or_result(ty, |id| self.definition(id))
    }

    pub(crate) fn has_tuple_payload(&self, ty: &Type) -> bool {
        matches!(
            self.peel_alias(&self.peel_alias(ty).ok_type()),
            Type::Tuple(_)
        )
    }

    pub(crate) fn tuple_element_types(&self, ty: &Type) -> Vec<Type> {
        types::tuple_element_types(ty, |id| self.definition(id))
    }

    pub(crate) fn is_prelude_nominal(&self, ty: &Type) -> bool {
        matches!(self.receiver_shape(ty), Type::Nominal { id, .. } if go_name::is_prelude_id(&id))
    }

    pub(crate) fn receiver_shape(&self, ty: &Type) -> Type {
        types::receiver_shape(ty, |id| self.definition(id))
    }

    pub(crate) fn as_interface(&self, ty: &Type) -> Option<String> {
        types::as_interface(ty, |id| self.definition(id)).map(|id| id.to_string())
    }

    pub(crate) fn is_interface(&self, ty: &Type) -> bool {
        self.as_interface(ty).is_some()
    }

    pub(crate) fn is_interface_or_unknown(&self, ty: &Type) -> bool {
        self.is_interface(ty) || self.resolves_to_unknown(ty)
    }

    pub(crate) fn is_nilable_go_type(&self, ty: &Type) -> bool {
        is_nilable_go_type(&self.input.definitions, ty)
    }

    pub(crate) fn go_nil_form(&self, ty: &Type) -> types::GoNilForm {
        types::go_nil_form(ty, |id| self.input.definitions.get(id))
    }

    pub(crate) fn underlying(&self, ty: &Type) -> Type {
        types::peel_underlying(ty, |id| self.input.definitions.get(id))
    }

    pub(crate) fn partial_ok_is_nilable(&self, ty: &Type) -> bool {
        types::partial_ok_is_nilable(ty, |id| self.input.definitions.get(id))
    }

    pub(crate) fn resolve_to_function_type(&self, ty: &Type) -> Option<Type> {
        types::resolve_to_function_type(ty, |id| self.definition(id))
    }

    pub(crate) fn function_alias_declaration(&self, ty: &Type) -> Option<Type> {
        self.resolve_to_function_type(self.alias_declaration(ty)?)
    }

    pub(crate) fn mentions_function_alias(&self, ty: &Type) -> bool {
        if self.function_alias_declaration(ty).is_some() {
            return true;
        }
        let Some(Type::Function(function)) = self.resolve_to_function_type(ty) else {
            return false;
        };
        function
            .params
            .iter()
            .any(|param| self.mentions_function_alias(&param.ty))
            || self.mentions_function_alias(&function.return_type)
    }

    pub(crate) fn alias_declaration(&self, ty: &Type) -> Option<&Type> {
        let Type::Nominal { id, .. } = ty.unwrap_forall() else {
            return None;
        };
        let DefinitionBody::TypeAlias {
            alias: AliasKind::Transparent { target, .. },
            ..
        } = &self.definition(id)?.body
        else {
            return None;
        };
        Some(target)
    }

    pub(crate) fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        self.input.bindings.is_unused_binding(pattern)
    }

    pub(crate) fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        self.input.bindings.is_unused_rest_binding(rest)
    }

    pub(crate) fn is_unused_import(&self, key: ImportKey) -> bool {
        self.input.unused_imports.is_unused_import(key)
    }

    pub(crate) fn is_mutated(&self, id: BindingId) -> bool {
        self.input.bindings.is_mutated(id)
    }

    pub(crate) fn is_alias_mutated(&self, id: BindingId) -> bool {
        self.input.bindings.is_alias_mutated(id)
    }

    fn is_ufcs_method(&self, qualified_type: &str, method: &str) -> bool {
        self.definition(qualified_type)
            .is_some_and(|definition| definition.is_ufcs_method(method))
    }

    pub(crate) fn emits_go_method(&self, qualified_type: &str, method: &str) -> bool {
        self.method(qualified_type, method).is_some()
            && !self.is_ufcs_method(qualified_type, method)
    }

    pub(crate) fn synthesized_field_equalities(&self, id: &str) -> &'a [EqualityRecipe] {
        self.input.equality_index.synthesized_fields(id)
    }

    pub(crate) fn synthesizes_equals(&self, id: &str) -> bool {
        self.input.equality_index.is_synthesized(id)
    }

    pub(crate) fn is_test(&self, qualified_name: &str) -> bool {
        self.input.test_index.contains_qualified(qualified_name)
    }

    pub(crate) fn current_package(&self) -> &str {
        &self.current_package
    }

    pub(crate) fn is_current_package(&self, package: &str) -> bool {
        package == self.current_package.as_str()
    }

    pub(crate) fn is_foreign_package(&self, package: &str) -> bool {
        !self.is_current_package(package) && package != go_name::PRELUDE_PACKAGE
    }

    pub(crate) fn is_entry_package(&self, package: &str) -> bool {
        package == self.input.entry_package_id
    }

    pub(crate) fn entry_package_name(&self) -> &str {
        self.shared.entry_package_name
    }

    pub(crate) fn qualified_current(&self, name: &str) -> String {
        format!("{}.{}", self.current_package, name)
    }

    pub(crate) fn go_module(&self) -> &str {
        self.shared.go_module
    }

    pub(crate) fn go_import_path(&self, package: &str) -> String {
        if package == go_name::TEST_PRELUDE_PACKAGE {
            return go_name::TESTKIT_IMPORT_PATH.to_string();
        }
        if self.is_entry_package(package) {
            return self.shared.go_module.to_string();
        }
        go_name::package_go_path(self.shared.go_module, package)
    }

    pub(crate) fn go_package_names(&self) -> &'a HashMap<String, String> {
        &self.input.go_package_names
    }

    pub(crate) fn has_global_exported_method_name(&self, method: &str) -> bool {
        self.shared.globals.exported_method_names.contains(method)
    }

    pub(crate) fn method_uses_tagged_return(&self, method: &str) -> bool {
        self.shared
            .globals
            .tagged_method_names
            .contains(&go_name::snake_to_camel(method))
    }

    pub(crate) fn enum_layout(&self, enum_id: &str) -> Option<&'a EnumLayout> {
        self.shared.globals.enum_layouts.get(enum_id)
    }

    fn make_function_name<'e>(
        &self,
        enum_id: &'e str,
        variant_name: &str,
    ) -> Option<(&'e str, String)> {
        let definition = self.definition(enum_id)?;
        let DefinitionBody::Enum { variants, .. } = &definition.body else {
            return None;
        };
        variants
            .iter()
            .any(|variant| variant.name == variant_name)
            .then(|| {
                let enum_name = types::unqualified_name(enum_id);
                (
                    enum_id,
                    go_name::enum_make_function(enum_name, variant_name),
                )
            })
    }

    pub(crate) fn variant_make_function<'e>(&self, symbol: &'e str) -> Option<(&'e str, String)>
    where
        'a: 'e,
    {
        if let Some((enum_id, variant)) = symbol.rsplit_once('.')
            && let Some(make_function) = self.make_function_name(enum_id, variant)
        {
            return Some(make_function);
        }
        let variant = symbol.strip_prefix(go_name::PRELUDE_PREFIX)?;
        if variant.contains('.') {
            return None;
        }
        let constructor_ty = &self.definition(symbol)?.ty;
        let enum_ty = match constructor_ty.unwrap_forall() {
            Type::Function(function) => function.return_type.as_ref(),
            ty => ty,
        };
        let Type::Nominal { id: enum_id, .. } = enum_ty else {
            return None;
        };
        self.make_function_name(enum_id.as_str(), variant)
    }

    pub(crate) fn go_abi_catalog(&self) -> &'a GoAbiCatalog {
        &self.shared.globals.go_abi_catalog
    }

    pub(crate) fn go_callable_parameter(
        &self,
        qualified_name: &str,
        index: usize,
    ) -> Option<&GoSlotDescriptor> {
        self.shared
            .globals
            .go_abi_catalog
            .callable_parameter(qualified_name, index)
    }

    pub(crate) fn go_callable_return_slot(
        &self,
        qualified_name: &str,
    ) -> Option<&GoSlotDescriptor> {
        self.shared
            .globals
            .go_abi_catalog
            .callable_return_slot(qualified_name)
    }

    pub(crate) fn go_field(&self, owner: &str, field: &str) -> Option<&GoSlotDescriptor> {
        self.shared.globals.go_abi_catalog.field(owner, field)
    }

    pub(crate) fn go_variable(&self, qualified_name: &str) -> Option<&GoSlotDescriptor> {
        self.shared.globals.go_abi_catalog.variable(qualified_name)
    }

    pub(crate) fn is_go_imported_type(&self, qualified_name: &str) -> bool {
        self.shared
            .globals
            .go_abi_catalog
            .is_imported_type(qualified_name)
    }

    pub(crate) fn tests_enabled(&self) -> bool {
        self.shared.emit_tests
    }

    pub(crate) fn sourcemap(&self) -> bool {
        self.shared.line_indexes.is_some()
    }

    pub(crate) fn line_index(&self, file_id: u32) -> Option<&LineIndex> {
        self.shared.line_indexes.as_ref()?.get(&file_id)
    }
}

fn is_nilable_go_type(definitions: &HashMap<Symbol, Definition>, ty: &Type) -> bool {
    types::is_nilable_go_type(ty, |id| definitions.get(id))
}

fn peel_alias(definitions: &HashMap<Symbol, Definition>, ty: &Type) -> Type {
    types::peel_alias(ty, |id| definitions.get(id))
}
