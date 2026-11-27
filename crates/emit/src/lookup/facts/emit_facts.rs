use crate::lookup::facts::callables::CallableDeclaration;
use crate::lookup::facts::definition_key::{DeclaredName, known_definition, type_query_lookup};
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::program::{
    CallOperation, CalleeName, CheckedExpression, CheckedValueUse, EqualityRecipe, FieldSelection,
    PRELUDE_PACKAGE_ID, ValueUseKey, is_go_import, is_prelude_symbol,
};

use crate::target::ir::values::Stability;
use ecow::EcoString;
use syntax::ast::{BindingId, Expression, Span};
use syntax::program::{
    AliasKind, Definition, DefinitionBody, EmitInput, ImportKey, Method, PackageId,
};
use syntax::types;
use syntax::types::{CompoundKind, SimpleKind};
use syntax::types::{Symbol, Type};

use crate::lookup::facts::catalog::{GoAbiCatalog, GoSlotDescriptor};
use crate::lookup::facts::context::LineIndex;
use crate::lookup::facts::context::SharedEmitContext;
use crate::target::ir::prelude::is_prelude_container_type;
use crate::target::names::go_name;
use crate::target::types::enum_layout::EnumLayout;
use crate::target::types::go_type::returns_go_void;

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

    pub(crate) fn callable(&self, name: &CalleeName) -> CallableDeclaration<'a> {
        match name {
            CalleeName::Definition(id) => CallableDeclaration::Definition(self.definition(id)),
            CalleeName::Method(method) => {
                CallableDeclaration::Method(self.known_method(&method.owner, &method.name))
            }
        }
    }

    pub(crate) fn definition(&self, id: &Symbol) -> &'a Definition {
        known_definition(&self.input.definitions, id)
    }

    pub(crate) fn find_definition(&self, id: &str) -> Option<&'a Definition> {
        self.input.definitions.get(id)
    }

    pub(crate) fn type_query_lookup(&self) -> impl Copy + Fn(&str) -> Option<&'a Definition> + 'a {
        type_query_lookup(&self.input.definitions)
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
        let slot = &self
            .input
            .checked_bodies
            .value_use(&ValueUseKey::value(expression))
            .expect("the checker records the use of every value")
            .slot;
        (!slot.is_ignored()).then_some(slot)
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

    pub(crate) fn method(&self, owner: &Symbol, name: &str) -> Option<&'a Method> {
        self.definition(owner).methods()?.get(name)
    }

    pub(crate) fn known_method(&self, owner: &Symbol, name: &str) -> &'a Method {
        self.method(owner, name)
            .unwrap_or_else(|| panic!("internal error: `{owner}` has no method `{name}`"))
    }

    pub(in crate::lookup::facts) fn peel_alias(&self, ty: &Type) -> Type {
        peel_alias(&self.input.definitions, ty)
    }

    pub(crate) fn alias_emits_nothing(&self, name: DeclaredName<'_>) -> bool {
        self.type_parameter_alias_index(&self.qualified_current(name))
            .is_some()
    }

    pub(crate) fn type_parameter_alias_index(&self, id: &Symbol) -> Option<usize> {
        self.type_parameter_alias_index_inner(id, &mut FxHashSet::default())
    }

    fn type_parameter_alias_index_inner(
        &self,
        id: &Symbol,
        visiting: &mut FxHashSet<Symbol>,
    ) -> Option<usize> {
        let DefinitionBody::TypeAlias {
            generics,
            alias: AliasKind::Transparent { target, .. },
            ..
        } = &self.definition(id).body
        else {
            return None;
        };

        if !visiting.insert(id.clone()) {
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

    pub(crate) fn immediate_underlying(&self, ty: &Type) -> Option<Type> {
        types::underlying_type(ty, self.type_query_lookup())
    }

    pub(crate) fn underlying_simple_kind(&self, ty: &Type) -> Option<SimpleKind> {
        types::underlying_simple_kind(ty, self.type_query_lookup())
    }

    pub(crate) fn resolves_to_unknown(&self, ty: &Type) -> bool {
        types::resolves_to_unknown(ty, self.type_query_lookup())
    }

    pub(crate) fn resolves_to_unit(&self, ty: &Type) -> bool {
        types::resolves_to_unit(ty, self.type_query_lookup())
    }

    pub(crate) fn is_prelude_error(&self, ty: &Type) -> bool {
        types::is_prelude_error(ty, self.type_query_lookup())
    }

    pub(crate) fn has_no_go_value(&self, ty: &Type) -> bool {
        self.resolves_to_unit(ty) || ty.is_variable() || ty.is_placeholder()
    }

    pub(crate) fn produces_no_go_value(&self, expression: &Expression) -> bool {
        expression
            .type_ref()
            .is_none_or(|ty| self.has_no_go_value(ty))
    }

    pub(crate) fn is_lisette_enum(&self, ty: &Type) -> bool {
        types::is_lisette_enum(ty, self.type_query_lookup())
    }

    pub(crate) fn nominal_zero(&self, ty: &Type) -> NominalZero {
        let Type::Nominal { id, params, .. } = ty else {
            return NominalZero::Opaque;
        };
        let go_import = is_go_import(id.as_str());
        if id == "prelude.Option" {
            return NominalZero::OptionNone {
                payload: params.first().cloned(),
            };
        }
        if go_import && (self.is_interface(ty) || self.resolve_to_function_type(ty).is_some()) {
            return NominalZero::GoZero;
        }
        if !go_import && matches!(self.definition(id).body, DefinitionBody::Enum { .. }) {
            return NominalZero::LisetteEnum;
        }
        if let Some(underlying) = self.newtype_underlying(ty) {
            return NominalZero::Newtype(underlying);
        }
        if go_import {
            return NominalZero::GoImported;
        }
        if let Some(fields) = self.struct_fields(ty) {
            return NominalZero::Struct(fields);
        }
        match self.immediate_underlying(ty) {
            Some(underlying) => NominalZero::Underlying(underlying),
            None => NominalZero::Opaque,
        }
    }

    fn fields_keep_go_zero(&self, fields: &[(EcoString, FieldSelection)]) -> bool {
        fields
            .iter()
            .all(|(_, selection)| self.go_zero_is_lisette_zero(&selection.field_ty()))
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
            Type::Nominal { .. } => match self.nominal_zero(&ty) {
                NominalZero::OptionNone { .. } | NominalZero::GoZero | NominalZero::LisetteEnum => {
                    true
                }
                NominalZero::Newtype(underlying) | NominalZero::Underlying(underlying) => {
                    self.go_zero_is_lisette_zero(&underlying)
                }
                NominalZero::Struct(fields) => self.fields_keep_go_zero(&fields),
                NominalZero::GoImported => match self.struct_fields(&ty) {
                    Some(fields) => self.fields_keep_go_zero(&fields),
                    None => self
                        .immediate_underlying(&ty)
                        .is_some_and(|underlying| self.go_zero_is_lisette_zero(&underlying)),
                },
                NominalZero::Opaque => false,
            },
            Type::ImportNamespace(_)
            | Type::Function(_)
            | Type::Var { .. }
            | Type::Uninferred
            | Type::Ignored
            | Type::Forall { .. }
            | Type::Never
            | Type::Error
            | Type::ReceiverPlaceholder => false,
        }
    }

    pub(crate) fn converts(&self, expression: &Expression) -> bool {
        self.checked(expression).value_use().adaptation.converts()
    }

    pub(crate) fn is_reference(&self, ty: &Type) -> bool {
        types::is_reference(ty, self.type_query_lookup())
    }

    pub(crate) fn points_to_ref(&self, ty: &Type) -> bool {
        types::points_to_ref(ty, self.type_query_lookup())
    }

    pub(crate) fn is_option(&self, ty: &Type) -> bool {
        types::is_option(ty, self.type_query_lookup())
    }

    pub(crate) fn is_partial(&self, ty: &Type) -> bool {
        types::is_partial(ty, self.type_query_lookup())
    }

    pub(crate) fn is_tuple(&self, ty: &Type) -> bool {
        types::is_tuple(ty, self.type_query_lookup())
    }

    pub(crate) fn ok_payload(&self, ty: &Type) -> Type {
        types::ok_payload(ty, self.type_query_lookup())
    }

    pub(crate) fn err_payload(&self, ty: &Type) -> Type {
        types::err_payload(ty, self.type_query_lookup())
    }

    pub(crate) fn is_never(&self, ty: &Type) -> bool {
        self.peel_alias(ty).is_never()
    }

    pub(crate) fn is_result(&self, ty: &Type) -> bool {
        self.peel_alias(ty).is_result()
    }

    pub(crate) fn peeled_option_or_result(&self, ty: &Type) -> Option<Type> {
        let peeled = self.peel_alias(ty);
        (peeled.is_option() || peeled.is_result()).then_some(peeled)
    }

    pub(crate) fn is_parameter_or_open(&self, ty: &Type) -> bool {
        matches!(
            self.peel_alias(ty).unwrap_forall(),
            Type::Parameter(_)
                | Type::Var { .. }
                | Type::Uninferred
                | Type::Error
                | Type::ReceiverPlaceholder
        )
    }

    pub(crate) fn simple_kind(&self, ty: &Type) -> Option<SimpleKind> {
        types::simple_kind(ty, self.type_query_lookup())
    }

    pub(crate) fn compound_parts(&self, ty: &Type) -> Option<(CompoundKind, Vec<Type>)> {
        let Type::Compound { kind, args, .. } = self.peel_alias(ty) else {
            return None;
        };
        Some((kind, args))
    }

    pub(crate) fn peel_go_alias(&self, ty: &Type) -> Type {
        let mut current = ty.unwrap_forall().clone();
        let mut seen: FxHashSet<Symbol> = FxHashSet::default();
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
                .instantiate_alias_target(params, *writable)
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
        matches!(ty, Type::Nominal { id, .. } if is_go_import(id))
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
        self.peel_underlying(ty).is_ref()
    }

    pub(crate) fn receiver_array_length(&self, ty: &Type) -> Option<u64> {
        let Type::Array { length, .. } = self.receiver_shape(ty) else {
            return None;
        };
        Some(length)
    }

    pub(crate) fn tuple_elements(&self, ty: &Type) -> Option<Vec<Type>> {
        let Type::Tuple(elements) = self.peel_alias(ty) else {
            return None;
        };
        Some(elements)
    }

    pub(crate) fn clone_container(&self, ty: &Type) -> Option<Type> {
        let shape = self.receiver_shape(ty);
        self.is_clone_container(&shape).then_some(shape)
    }

    pub(crate) fn array_parts(&self, ty: &Type) -> Option<(u64, Type)> {
        let Type::Array { length, element } = self.peel_alias(ty) else {
            return None;
        };
        Some((length, *element))
    }

    pub(crate) fn prelude_container_arguments(&self, ty: &Type) -> Option<Vec<Type>> {
        let peeled = self.peel_alias(ty);
        if !matches!(peeled, Type::Nominal { .. }) || !is_prelude_container_type(&peeled) {
            return None;
        }
        peeled.get_type_params().map(<[Type]>::to_vec)
    }

    pub(crate) fn is_lowered_result_type(&self, ty: &Type) -> bool {
        self.prelude_container_arguments(ty).is_some()
            || self
                .tuple_elements(ty)
                .is_some_and(|elements| elements.len() >= 2)
    }

    pub(crate) fn receiver_tuple_elements(&self, ty: &Type) -> Option<Vec<Type>> {
        let Type::Tuple(elements) = self.receiver_shape(ty) else {
            return None;
        };
        Some(elements)
    }

    pub(crate) fn receiver_nominal_id(&self, ty: &Type) -> Option<Symbol> {
        let Type::Nominal { id, .. } = self.receiver_shape(ty) else {
            return None;
        };
        Some(id)
    }

    pub(crate) fn is_ref_to_type_parameter(&self, ty: &Type) -> bool {
        let peeled = self.peel_alias(ty);
        peeled.is_ref()
            && peeled
                .inner()
                .is_some_and(|inner| matches!(self.peel_alias(&inner), Type::Parameter(_)))
    }

    pub(crate) fn is_option_or_result(&self, ty: &Type) -> bool {
        types::is_option_or_result(ty, self.type_query_lookup())
    }

    pub(crate) fn has_tuple_payload(&self, ty: &Type) -> bool {
        matches!(
            self.peel_alias(&self.peel_alias(ty).ok_type()),
            Type::Tuple(_)
        )
    }

    pub(crate) fn tuple_or_type_arguments(&self, ty: &Type) -> Vec<Type> {
        types::tuple_element_types(ty, self.type_query_lookup())
    }

    pub(crate) fn is_prelude_nominal(&self, ty: &Type) -> bool {
        matches!(self.receiver_shape(ty), Type::Nominal { id, .. } if is_prelude_symbol(&id))
    }

    pub(in crate::lookup::facts) fn receiver_shape(&self, ty: &Type) -> Type {
        types::receiver_shape(ty, self.type_query_lookup())
    }

    pub(crate) fn is_interface(&self, ty: &Type) -> bool {
        types::as_interface(ty, self.type_query_lookup()).is_some()
    }

    pub(crate) fn is_interface_or_unknown(&self, ty: &Type) -> bool {
        self.is_interface(ty) || self.resolves_to_unknown(ty)
    }

    pub(crate) fn is_nilable_go_type(&self, ty: &Type) -> bool {
        is_nilable_go_type(&self.input.definitions, ty)
    }

    pub(crate) fn go_nil_form(&self, ty: &Type) -> types::GoNilForm {
        types::go_nil_form(ty, self.type_query_lookup())
    }

    pub(in crate::lookup::facts) fn peel_underlying(&self, ty: &Type) -> Type {
        types::peel_underlying(ty, self.type_query_lookup())
    }

    pub(crate) fn partial_ok_is_nilable(&self, ty: &Type) -> bool {
        types::partial_ok_is_nilable(ty, self.type_query_lookup())
    }

    pub(crate) fn resolve_to_function_type(&self, ty: &Type) -> Option<Type> {
        types::resolve_to_function_type(ty, self.type_query_lookup())
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
        } = &self.definition(id).body
        else {
            return None;
        };

        Some(target)
    }

    pub(crate) fn is_unused_import(&self, key: ImportKey) -> bool {
        self.input.unused_imports.is_unused_import(key)
    }

    pub(in crate::lookup) fn binding_read_stability(&self, id: BindingId) -> Stability {
        if !self.input.bindings.is_mutated(id) {
            Stability::Fixed
        } else if !self.input.bindings.is_alias_mutated(id) {
            Stability::StableAcrossCalls
        } else {
            Stability::Observable
        }
    }

    pub(crate) fn hidden_binding(&self, id: BindingId) -> Option<BindingId> {
        self.input.bindings.hidden_binding(id)
    }

    pub(crate) fn emits_go_method(&self, qualified_type: &Symbol, method: &str) -> bool {
        self.method(qualified_type, method).is_some()
            && !self.definition(qualified_type).is_ufcs_method(method)
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
        !self.is_current_package(package) && package != PRELUDE_PACKAGE_ID
    }

    pub(crate) fn is_entry_package(&self, package: &str) -> bool {
        package == self.input.entry_package_id
    }

    pub(crate) fn entry_package_name(&self) -> &str {
        self.shared.entry_package_name
    }

    pub(crate) fn qualified_current(&self, name: DeclaredName<'_>) -> Symbol {
        Symbol::from_parts(&self.current_package, name.as_str())
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

    pub(crate) fn go_package_names(&self) -> &'a FxHashMap<String, String> {
        &self.input.go_package_names
    }

    pub(crate) fn method_needs_export(&self, method: &str) -> bool {
        self.shared.globals.method_policy.exports_method(method)
    }

    pub(crate) fn method_uses_tagged_return(&self, method: &str) -> bool {
        self.shared
            .globals
            .tagged_method_names
            .contains(&go_name::snake_to_camel(method))
    }

    pub(crate) fn enum_layout(&self, enum_id: &Symbol) -> &'a EnumLayout {
        self.shared
            .globals
            .enum_layouts
            .get(enum_id)
            .unwrap_or_else(|| panic!("internal error: `{enum_id}` has no enum layout"))
    }

    fn make_function_name<'e>(
        &self,
        enum_id: &'e str,
        variant_name: &str,
    ) -> Option<(&'e str, String)> {
        let definition = self.find_definition(enum_id)?;
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
        let package = self
            .package_for_qualified_name(symbol)
            .unwrap_or_else(|| panic!("internal error: `{symbol}` names no package"));
        let member = &symbol[package.len() + 1..];
        if let Some((owner, variant)) = member.rsplit_once('.') {
            let enum_id = &symbol[..symbol.len() - variant.len() - 1];
            debug_assert!(!owner.is_empty());
            return self.make_function_name(enum_id, variant);
        }
        if package != PRELUDE_PACKAGE_ID {
            return None;
        }
        let variant = member;
        let constructor_ty = &self.find_definition(symbol)?.ty;
        let enum_ty = constructor_ty
            .as_function_type()
            .map_or(constructor_ty.unwrap_forall(), |function| {
                function.return_type.as_ref()
            });
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
        qualified_name: &CalleeName,
        index: usize,
    ) -> Option<&GoSlotDescriptor> {
        self.shared
            .globals
            .go_abi_catalog
            .callable_parameter(qualified_name, index)
    }

    pub(crate) fn go_callable_return_slot(
        &self,
        qualified_name: &CalleeName,
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
        let index = self.shared.line_indexes.as_ref()?.get(&file_id);
        if index.is_none() {
            let file = self
                .input
                .files
                .get(&file_id)
                .unwrap_or_else(|| panic!("internal error: no file has the id {file_id}"));
            assert!(
                file.is_d_lis(),
                "internal error: `{}` has no line index",
                file.display_path
            );
        }
        index
    }
}

fn is_nilable_go_type(definitions: &FxHashMap<Symbol, Definition>, ty: &Type) -> bool {
    types::is_nilable_go_type(ty, type_query_lookup(definitions))
}

fn peel_alias(definitions: &FxHashMap<Symbol, Definition>, ty: &Type) -> Type {
    types::peel_alias(ty, type_query_lookup(definitions))
}

pub(crate) fn ref_stripped_type_parameter(ty: &Type) -> Option<EcoString> {
    let Type::Parameter(name) = ty.strip_refs() else {
        return None;
    };
    Some(name)
}

pub(crate) enum NominalZero {
    OptionNone { payload: Option<Type> },
    GoZero,
    LisetteEnum,
    Newtype(Type),
    GoImported,
    Struct(Vec<(EcoString, FieldSelection)>),
    Underlying(Type),
    Opaque,
}
