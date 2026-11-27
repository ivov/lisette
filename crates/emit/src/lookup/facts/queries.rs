use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::ir::prelude as fallible;
use crate::target::ir::prelude::Fallible;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::{self, GoExport, MemberSpelling};
use crate::target::names::source_name::SourceName;
use crate::target::types::enum_layout::VariantLayout;
use std::borrow::Cow;
use syntax::ast::{Expression, Literal, Pattern, RestPattern, StructFieldDefinition, StructFields};
use syntax::go_names;
use syntax::program::{Definition, DefinitionBody, DotAccessResolution, constructor_fields};
use syntax::program::{FieldSelection, GoHint, ReceiverCoercion};
use syntax::program::{NativeTypeKind, PRELUDE_PACKAGE_ID, is_go_import, is_prelude_symbol};
use syntax::types::{CompoundKind, build_substitution_map};
use syntax::types::{Symbol, Type};

pub(crate) struct ResolvedNominal<'a> {
    pub(crate) id: Symbol,
    pub(crate) definition: &'a Definition,
}

pub(crate) fn is_scalar_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_),
            ..
        }
    )
}

pub(crate) fn is_order_sensitive(expression: &Expression) -> bool {
    !(is_scalar_literal(expression) || matches!(expression, Expression::Identifier { .. }))
}

pub(crate) fn contains_call(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Call { .. } | Expression::Task { .. }
    ) || expression.children().into_iter().any(contains_call)
}

impl<'a> EmitFacts<'a> {
    pub(crate) fn field_read_cannot_panic(&self, ty: &Type) -> bool {
        !self.is_nilable_go_type(ty) && !ty.is_variable() && !ty.is_placeholder()
    }

    pub(crate) fn is_enum_field_recursive(&self, ty: &Type, variant: &str, index: usize) -> bool {
        self.enum_variant_layout(ty, variant)
            .and_then(|variant_layout| variant_layout.fields.get(index))
            .is_some_and(|field| field.is_recursive())
    }

    fn enum_variant_layout(&self, ty: &Type, variant: &str) -> Option<&'a VariantLayout> {
        let Type::Nominal { id, .. } = self.receiver_shape(ty) else {
            return None;
        };
        if is_prelude_symbol(&id) {
            return None;
        }
        Some(self.enum_layout(&id).variant(variant))
    }

    pub(crate) fn enum_struct_field_index(
        &self,
        ty: &Type,
        variant: &str,
        field_name: &str,
    ) -> Option<usize> {
        let Type::Nominal { id, .. } = ty else {
            return None;
        };
        let index = constructor_fields(id, Some(variant), ty, self.type_query_lookup())
            .unwrap_or_else(|| panic!("internal error: `{id}` has no variant `{variant}`"))
            .fields
            .iter()
            .position(|field| field.name == field_name)
            .unwrap_or_else(|| {
                panic!(
                    "internal error: the variant `{variant}` of `{id}` has no field `{field_name}`"
                )
            });
        Some(index)
    }

    pub(crate) fn resolve_nominal(&self, ty: &Type) -> Option<ResolvedNominal<'a>> {
        let Type::Nominal { id, .. } = self.receiver_shape(ty) else {
            return None;
        };
        let definition = self.definition(&id);
        Some(ResolvedNominal { id, definition })
    }

    pub(crate) fn embedded_field(
        &self,
        struct_ty: &Type,
        field_name: &str,
    ) -> Option<&StructFieldDefinition> {
        let resolved = self.resolve_nominal(struct_ty)?;
        let DefinitionBody::Struct { fields, .. } = &resolved.definition.body else {
            return None;
        };
        fields
            .iter()
            .find(|f| f.name == field_name && f.is_embedded())
    }

    pub(crate) fn field_access(
        &self,
        base: GoExpression,
        ty: &Type,
        field: String,
    ) -> GoExpression {
        if self.field_read_cannot_panic(ty) {
            GoExpression::value_field(base, field)
        } else {
            GoExpression::selector(base, field)
        }
    }

    pub(crate) fn is_tuple_struct_type(&self, ty: &Type) -> bool {
        self.resolve_nominal(ty).is_some_and(|resolved| {
            matches!(
                &resolved.definition.body,
                DefinitionBody::Struct {
                    fields: StructFields::Tuple(_),
                    ..
                }
            )
        })
    }

    pub(crate) fn newtype_underlying(&self, ty: &Type) -> Option<Type> {
        self.resolve_nominal(ty)?
            .definition
            .newtype_field()
            .cloned()
    }

    pub(crate) fn as_enum(&self, ty: &Type) -> Option<Symbol> {
        let resolved = self.resolve_nominal(ty)?;
        matches!(&resolved.definition.body, DefinitionBody::Enum { .. }).then_some(resolved.id)
    }

    pub(crate) fn enum_tuple_field_name(&self, ty: &Type, variant: &str, index: usize) -> String {
        let ty = &self.receiver_shape(ty);
        if let Some(variants) = fallible::tagged_payload_fields(ty) {
            return variants
                .iter()
                .find(|tagged| tagged.name == variant)
                .and_then(|tagged| tagged.fields.get(index))
                .unwrap_or_else(|| {
                    panic!("internal error: `{variant}` of `{ty:?}` has no field {index}")
                })
                .to_string();
        }

        if let Some(field) = self
            .enum_variant_layout(ty, variant)
            .and_then(|variant_layout| variant_layout.fields.get(index))
        {
            return field.go_name.clone();
        }

        unreachable!("enum `{ty:?}` has no layout for field {index} of `{variant}`")
    }

    pub(crate) fn is_newtype_struct(&self, ty: &Type) -> bool {
        self.newtype_underlying(ty).is_some()
    }

    pub(crate) fn reads_value_member(
        &self,
        resolution: &DotAccessResolution,
        base: &Expression,
        base_ty: &Type,
    ) -> bool {
        resolution.is_field_read() && base.deref_inner().is_none() && !self.is_reference(base_ty)
    }

    pub(crate) fn is_clone_container(&self, ty: &Type) -> bool {
        matches!(
            self.peel_alias(ty),
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
                ..
            }
        )
    }

    pub(crate) fn first_type_argument(&self, ty: &Type) -> Option<Type> {
        self.receiver_shape(ty).get_type_params()?.first().cloned()
    }

    pub(crate) fn fallible(&self, ty: &Type) -> Option<Fallible> {
        Fallible::from_type(&self.peel_alias(ty))
    }

    pub(crate) fn dereference_refs(&self, ty: &Type, mut value: GoExpression) -> GoExpression {
        let mut current = self.peel_alias(ty);
        while current.is_ref() {
            value = GoExpression::dereference(value);
            let pointee = current.inner().expect("Ref<T> must have inner");
            current = self.peel_alias(&pointee);
        }
        value
    }

    pub(crate) fn dot_receiver_coercion(
        &self,
        receiver: &Expression,
        member: &str,
    ) -> Option<ReceiverCoercion> {
        self.checked(receiver).receiver_change().filter(|coercion| {
            *coercion != ReceiverCoercion::AutoAddress
                || !self.has_go_value_receiver(&receiver.get_type(), member)
        })
    }

    pub(crate) fn has_go_value_receiver(&self, receiver_ty: &Type, method: &str) -> bool {
        let receiver_ty = self.peel_alias(&receiver_ty.strip_refs());
        receiver_ty
            .nominal_id()
            .filter(|owner| is_go_import(owner))
            .and_then(|owner| self.method(owner, method))
            .is_some_and(|method| method.go_hints.contains(&GoHint::ValueMethodSet))
    }

    pub(crate) fn has_stringer_signature(&self, id: &Symbol, method_name: &str) -> bool {
        self.method(id, method_name)
            .is_some_and(|method| method.ty.is_stringer_signature(|ty| self.peel_alias(ty)))
            && self.emits_go_method(id, method_name)
    }

    pub(crate) fn struct_fields(
        &self,
        ty: &Type,
    ) -> Option<Vec<(ecow::EcoString, FieldSelection)>> {
        let stripped = ty.strip_refs();
        let Type::Nominal { id, .. } = &stripped else {
            return None;
        };
        let constructor = constructor_fields(id, None, &stripped, self.type_query_lookup())?;
        let Type::Nominal { id: owner, .. } = self.peel_alias(&stripped) else {
            return None;
        };
        let substitutions = build_substitution_map(constructor.generics, &constructor.type_args);
        Some(
            constructor
                .fields
                .into_iter()
                .map(|field| {
                    let selection = FieldSelection {
                        owner: owner.clone(),
                        declared_ty: field.declared_ty.clone(),
                        substitutions: substitutions.clone(),
                    };
                    (field.name.clone(), selection)
                })
                .collect(),
        )
    }

    pub(crate) fn short_declaration_keeps_type(&self, ty: &Type) -> bool {
        let peeled = self.peel_alias(ty);
        !self.is_go_named_function_type(ty)
            && !self.is_interface_or_unknown(&peeled)
            && !(matches!(peeled, Type::Nominal { .. })
                && self.underlying_simple_kind(&peeled).is_some())
    }

    pub(crate) fn dot_member_go_name(
        &self,
        expression: &Expression,
        expression_ty: &Type,
        member: &str,
        resolution: &DotAccessResolution,
        spelling: MemberSpelling,
    ) -> String {
        if let Some(package) = expression_ty.as_import_namespace() {
            return self.package_member_go_name(package, member);
        }
        let canonical_ty = self.receiver_shape(expression_ty);
        match resolution {
            DotAccessResolution::StructField { is_exported, .. } => {
                let export = if *is_exported {
                    GoExport::Exported
                } else {
                    GoExport::Unexported
                };
                return self.struct_field_go_name(&canonical_ty, member, export);
            }
            DotAccessResolution::InstanceMethod { is_exported, .. } => {
                let export = if *is_exported {
                    GoExport::Exported
                } else {
                    GoExport::Unexported
                };
                return self.method_selector_go_name(
                    canonical_ty.get_qualified_id(),
                    member,
                    export,
                    spelling,
                );
            }
            DotAccessResolution::Unresolved
            | DotAccessResolution::TupleStructField { .. }
            | DotAccessResolution::TupleElement { .. }
            | DotAccessResolution::PackageMember { .. }
            | DotAccessResolution::EnumVariant { .. }
            | DotAccessResolution::InstanceMethodValue { .. }
            | DotAccessResolution::StaticMethod { .. } => {}
        }
        let export = self.resolve_go_export(expression, &canonical_ty, member);
        self.member_go_name(&canonical_ty, member, export)
    }

    pub(crate) fn receiver_method_go_name(
        &self,
        receiver: &Type,
        method: &str,
        export: GoExport,
        spelling: MemberSpelling,
    ) -> String {
        let owner = receiver.strip_refs();
        self.method_selector_go_name(owner.get_qualified_id(), method, export, spelling)
    }

    pub(crate) fn receiver_go_target(&self, ty: &Type) -> Type {
        self.peel_go_alias(&ty.strip_refs())
    }

    pub(crate) fn is_go_abi_type(&self, ty: &Type) -> bool {
        self.resolve_nominal(ty)
            .is_some_and(|resolved| self.is_go_imported_type(resolved.id.as_str()))
    }

    fn field_is_public(&self, struct_ty: &Type, field_name: &str) -> bool {
        let resolved = self
            .resolve_nominal(struct_ty)
            .unwrap_or_else(|| panic!("internal error: `{struct_ty:?}` is not a named type"));
        self.member_is_public(&resolved, field_name)
            .unwrap_or_else(|| {
                panic!(
                    "internal error: `{}` has no member `{field_name}`",
                    resolved.id
                )
            })
    }

    fn member_is_public(&self, resolved: &ResolvedNominal<'_>, name: &str) -> Option<bool> {
        if let DefinitionBody::Struct { fields, .. } = &resolved.definition.body {
            if let Some(field) = fields.iter().find(|field| field.name == name) {
                return Some(go_names::struct_field_is_exported(
                    field,
                    resolved.definition.is_serialized(),
                ));
            }
            if let Some(method) = self.method(&resolved.id, name) {
                return Some(method.visibility.is_public());
            }
            return fields
                .iter()
                .filter(|field| field.is_embedded())
                .filter_map(|field| self.resolve_nominal(&field.ty))
                .find_map(|embedded| self.member_is_public(&embedded, name));
        }
        self.method(&resolved.id, name)
            .map(|method| method.visibility.is_public())
    }

    pub(crate) fn resolve_go_export(
        &self,
        expression: &Expression,
        expression_ty: &Type,
        member: &str,
    ) -> GoExport {
        if compute_is_exported_context(expression, expression_ty)
            || self.field_is_public(expression_ty, member)
            || self.method_needs_export(member)
        {
            GoExport::Exported
        } else {
            GoExport::Unexported
        }
    }

    fn struct_field_is_exported(&self, ty: &Type, field: &str) -> bool {
        self.field_is_public(ty, field) || type_uses_exported_members(ty)
    }

    pub(crate) fn struct_field_go_name(
        &self,
        ty: &Type,
        field: &str,
        semantic_export: GoExport,
    ) -> String {
        let export = if semantic_export == GoExport::Unexported
            && self.struct_field_is_exported(ty, field)
        {
            GoExport::Exported
        } else {
            semantic_export
        };

        self.member_go_name(ty, field, export)
    }

    pub(crate) fn type_member_go_name<'n>(&self, package: &str, name: &'n str) -> Cow<'n, str> {
        if package == PRELUDE_PACKAGE_ID || self.is_foreign_package(package) {
            go_name::escape_keyword(name)
        } else {
            go_name::escape_type_name(name)
        }
    }

    pub(crate) fn native_kind(&self, ty: &Type) -> Option<NativeTypeKind> {
        NativeTypeKind::from_type(&self.receiver_shape(ty))
    }

    pub(crate) fn native_shape(&self, ty: &Type) -> Option<NativeTypeKind> {
        self.native_kind(ty)
            .filter(|kind| *kind != NativeTypeKind::Array)
    }

    pub(crate) fn is_native_shape(&self, ty: &Type, kind: NativeTypeKind) -> bool {
        self.native_shape(ty).is_some_and(|shape| shape == kind)
    }
}

pub(crate) fn go_name_for_binding(pattern: &Pattern) -> Option<SourceName> {
    let name = match pattern {
        Pattern::Identifier { identifier, .. } => identifier,
        Pattern::AsBinding { name, .. } => name,
        Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::EnumVariant { .. }
        | Pattern::Struct { .. }
        | Pattern::Tuple { .. }
        | Pattern::WildCard { .. }
        | Pattern::Slice { .. }
        | Pattern::Or { .. } => return None,
    };
    Some(SourceName::new(name))
}

pub(crate) fn go_name_for_rest_binding(rest: &RestPattern) -> Option<SourceName> {
    let RestPattern::Bind { name, .. } = rest else {
        return None;
    };
    Some(SourceName::new(name))
}

fn type_uses_exported_members(ty: &Type) -> bool {
    let Type::Nominal { id, .. } = ty.strip_refs() else {
        return false;
    };
    is_prelude_symbol(&id) || is_go_import(&id)
}

fn compute_is_exported_context(expression: &Expression, expression_ty: &Type) -> bool {
    let is_import_namespace_identifier = matches!(
        expression,
        Expression::Identifier { ty, .. } if ty.as_import_namespace().is_some()
    );
    is_import_namespace_identifier || type_uses_exported_members(expression_ty)
}
