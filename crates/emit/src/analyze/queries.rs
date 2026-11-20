use crate::EmitFacts;
use crate::Planner;
use crate::abi::ReturnSlot;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi};
use crate::control_flow::fallible;
use crate::control_flow::fallible::Fallible;
use crate::ir::values::GoExpression;
use crate::names::go_name;
use crate::names::source_name::SourceName;
use syntax::ast::{Expression, Literal, Pattern, RestPattern, StructFieldDefinition, StructFields};
use syntax::go_names;
use syntax::program::{Definition, DefinitionBody, DotAccessResolution, constructor_fields};
use syntax::program::{FieldSelection, GoHint, interface_requirements};
use syntax::types::{CompoundKind, build_substitution_map};
use syntax::types::{Symbol, Type};

impl Planner<'_> {
    pub(crate) fn member_owner_type(&self, ty: &Type) -> Type {
        self.facts.receiver_shape(ty)
    }

    pub(crate) fn field_is_public(&self, struct_ty: &Type, field_name: &str) -> bool {
        let Some(resolved) = self.facts.resolve_nominal(struct_ty) else {
            return false;
        };
        let id = resolved.id.as_str();

        match &resolved.definition.body {
            DefinitionBody::Struct { fields, .. } => {
                if let Some(field) = fields.iter().find(|f| f.name == field_name) {
                    return go_names::struct_field_is_exported(
                        field,
                        resolved.definition.is_serialized(),
                    );
                }
                self.facts
                    .method(id, field_name)
                    .map(|method| method.visibility.is_public())
                    .unwrap_or(false)
            }
            DefinitionBody::Enum { .. } => self
                .facts
                .method(id, field_name)
                .map(|method| method.visibility.is_public())
                .unwrap_or(false),
            DefinitionBody::Interface { definition } => {
                resolved.definition.visibility.is_public()
                    && definition.methods.contains_key(field_name)
            }
            _ => false,
        }
    }

    pub(crate) fn type_uses_exported_members(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return false;
        };
        go_name::is_prelude_id(&id)
            || self
                .facts
                .package_for_qualified_name(id.as_str())
                .is_some_and(|m| self.facts.is_foreign_package(m))
    }

    fn struct_field_is_exported(&self, ty: &Type, field: &str) -> bool {
        self.field_is_public(ty, field) || self.type_uses_exported_members(ty)
    }

    pub(crate) fn struct_field_go_name(
        &self,
        ty: &Type,
        field: &str,
        semantic_exported: bool,
    ) -> String {
        go_name::member_go_name(
            ty,
            field,
            semantic_exported || self.struct_field_is_exported(ty, field),
            self.facts.embedded_field(ty, field),
        )
    }
}

pub(crate) struct ResolvedNominal<'a> {
    pub(crate) id: Symbol,
    pub(crate) definition: &'a Definition,
}

fn is_scalar_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
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
    !(is_scalar_literal(expression)
        || matches!(expression.unwrap_parens(), Expression::Identifier { .. }))
}

impl<'a> EmitFacts<'a> {
    pub(crate) fn go_name_for_binding(&self, pattern: &Pattern) -> Option<SourceName> {
        let name = match pattern {
            Pattern::Identifier { identifier, .. } => identifier,
            Pattern::AsBinding { name, .. } => name,
            _ => return None,
        };
        if self.is_unused_binding(pattern) {
            None
        } else {
            Some(SourceName::new(name))
        }
    }

    pub(crate) fn go_name_for_rest_binding(&self, rest: &RestPattern) -> Option<SourceName> {
        if let RestPattern::Bind { name, .. } = rest {
            if self.is_unused_rest_binding(rest) {
                None
            } else {
                Some(SourceName::new(name))
            }
        } else {
            None
        }
    }

    pub(crate) fn method_needs_export(&self, method_name: &str) -> bool {
        self.has_global_exported_method_name(method_name)
            || matches!(method_name, "string" | "goString" | "error")
    }

    pub(crate) fn field_read_cannot_panic(&self, ty: &Type) -> bool {
        !self.is_nilable_go_type(ty) && !ty.is_variable() && !ty.is_placeholder()
    }

    pub(crate) fn enum_struct_field_name(
        &self,
        enum_id: &str,
        variant_name: &str,
        field_name: &str,
    ) -> Option<String> {
        self.enum_layout(enum_id)?
            .struct_field_name(variant_name, field_name)
    }

    pub(crate) fn is_enum_field_recursive(&self, ty: &Type, variant: &str, index: usize) -> bool {
        if let Type::Nominal { id, .. } = ty
            && let Some(layout) = self.enum_layout(id.as_ref())
            && let Some(variant_layout) = layout.variant(variant)
            && let Some(field) = variant_layout.fields.get(index)
        {
            return field.is_recursive();
        }
        false
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
        constructor_fields(id, Some(variant), ty, |id| self.definition(id))?
            .fields
            .iter()
            .position(|field| field.name == field_name)
    }

    pub(crate) fn resolve_nominal(&self, ty: &Type) -> Option<ResolvedNominal<'a>> {
        let Type::Nominal { id, .. } = self.receiver_shape(ty) else {
            return None;
        };
        let definition = self.definition(id.as_str())?;
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

    pub(crate) fn as_enum(&self, ty: &Type) -> Option<String> {
        let resolved = self.resolve_nominal(ty)?;
        matches!(&resolved.definition.body, DefinitionBody::Enum { .. })
            .then(|| resolved.id.to_string())
    }

    pub(crate) fn enum_tuple_field_name(&self, ty: &Type, variant: &str, index: usize) -> String {
        let ty = &self.receiver_shape(ty);
        if ty.is_option() {
            return match variant {
                "Some" => fallible::OPTION_SOME_FIELD.to_string(),
                _ => variant.to_string(),
            };
        }

        if ty.is_result() {
            return match (variant, index) {
                ("Ok", 0) => fallible::RESULT_OK_FIELD.to_string(),
                ("Err", 0) => fallible::RESULT_ERR_FIELD.to_string(),
                _ => variant.to_string(),
            };
        }

        if ty.is_partial() {
            return match (variant, index) {
                ("Ok", 0) => fallible::PARTIAL_OK_FIELD.to_string(),
                ("Err", 0) => fallible::PARTIAL_ERR_FIELD.to_string(),
                ("Both", 0) => fallible::PARTIAL_OK_FIELD.to_string(),
                ("Both", 1) => fallible::PARTIAL_ERR_FIELD.to_string(),
                _ => variant.to_string(),
            };
        }

        if let Type::Nominal { id, .. } = ty
            && let Some(layout) = self.enum_layout(id)
            && let Some(name) = layout.tuple_field_name(variant, index)
        {
            return name;
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

    pub(crate) fn is_plain_struct(&self, ty: &Type) -> bool {
        let ty = &self.peel_alias(ty);
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        !go_name::is_go_import(id.as_str())
            && self.newtype_underlying(ty).is_none()
            && matches!(
                self.definition(id.as_str()).map(|d| &d.body),
                Some(DefinitionBody::Struct { .. })
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

    pub(crate) fn bound_needs_dispatch(&self, bound: &Type) -> bool {
        interface_requirements(bound, |id| self.definition(id))
            .iter()
            .any(|requirement| {
                !go_name::is_go_import(&requirement.declaring_interface)
                    && requirement.ty.get_function_ret().is_some_and(|ty| {
                        matches!(
                            self.return_abi(ty, ReturnSlot::Method),
                            CallableReturnAbi::Lowered(LoweredReturnAbi::Partial { .. })
                        )
                    })
            })
    }

    pub(crate) fn has_stringer_signature(&self, name: &str, method_name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.definition(qualified.as_str())
            .and_then(Definition::methods)
            .and_then(|methods| methods.get(method_name))
            .is_some_and(|method| method.ty.is_stringer_signature(|ty| self.peel_alias(ty)))
            && self.emits_go_method(&qualified, method_name)
    }

    pub(crate) fn has_go_value_receiver(&self, receiver_ty: &Type, method: &str) -> bool {
        let receiver_ty = self.peel_alias(&receiver_ty.strip_refs());
        receiver_ty
            .get_qualified_id()
            .filter(|owner| go_name::is_go_import(owner))
            .and_then(|owner| self.method(owner, method))
            .is_some_and(|method| method.go_hints.contains(&GoHint::ValueMethodSet))
    }

    pub(crate) fn struct_fields(
        &self,
        ty: &Type,
    ) -> Option<Vec<(ecow::EcoString, FieldSelection)>> {
        let stripped = ty.strip_refs();
        let Type::Nominal { id, .. } = &stripped else {
            return None;
        };
        let constructor = constructor_fields(id, None, &stripped, |id| self.definition(id))?;
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

    pub(crate) fn empty_composite_is_go_zero(&self, ty: &Type) -> bool {
        let ty = self.peel_alias(ty);
        self.is_plain_struct(&ty) || self.is_lisette_enum(&ty)
    }

    pub(crate) fn short_declaration_keeps_type(&self, ty: &Type) -> bool {
        let peeled = self.peel_alias(ty);
        !self.is_function_alias(ty)
            && !self.is_interface_or_unknown(&peeled)
            && !(matches!(peeled, Type::Nominal { .. })
                && self.underlying_simple_kind(&peeled).is_some())
    }
}
