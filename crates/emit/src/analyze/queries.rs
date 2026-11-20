use crate::EmitFacts;
use crate::Planner;
use crate::control_flow::fallible;
use crate::ir::values::GoExpression;
use crate::names::go_name;
use crate::names::source_name::SourceName;
use syntax::ast::{Expression, Literal, Pattern, RestPattern, StructFieldDefinition, StructFields};
use syntax::go_names;
use syntax::program::{Definition, DefinitionBody, DotAccessResolution};
use syntax::types;
use syntax::types::{Symbol, Type, substitute};

impl Planner<'_> {
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

    pub(crate) fn struct_field_is_exported(&self, ty: &Type, field: &str) -> bool {
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

    pub(crate) fn peel_alias_id(&self, id: &str) -> String {
        let nominal = Type::Nominal {
            id: id.into(),
            params: Vec::new(),
            writable: false,
        };
        self.peel_alias(&nominal)
            .get_qualified_id()
            .unwrap_or(id)
            .to_string()
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

    pub(crate) fn is_enum_field_unit(&self, ty: &Type, variant: &str, index: usize) -> bool {
        if let Type::Nominal {
            id, params: args, ..
        } = ty
            && let Some(Definition {
                body:
                    DefinitionBody::Enum {
                        generics, variants, ..
                    },
                ..
            }) = self.definition(id.as_str())
        {
            let sub_map: types::SubstitutionMap = generics
                .iter()
                .map(|g| g.name.clone())
                .zip(args.iter().cloned())
                .collect();
            for v in variants {
                if v.name == variant
                    && let Some(field) = v.fields.iter().nth(index)
                {
                    let concrete = substitute(&field.ty, &sub_map);
                    return self.resolves_to_unit(&concrete) || concrete.is_never();
                }
            }
        }
        false
    }

    pub(crate) fn enum_struct_field_index(
        &self,
        ty: &Type,
        variant: &str,
        field_name: &str,
    ) -> Option<usize> {
        if let Type::Nominal { id, .. } = ty
            && let Some(Definition {
                body: DefinitionBody::Enum { variants, .. },
                ..
            }) = self.definition(id.as_str())
        {
            for v in variants {
                if v.name == variant {
                    return v.fields.iter().position(|f| f.name == field_name);
                }
            }
        }
        None
    }

    pub(crate) fn resolve_nominal(&self, ty: &Type) -> Option<ResolvedNominal<'a>> {
        let Type::Nominal { id, .. } = self.strip_and_peel(ty) else {
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

    pub(crate) fn has_field(&self, struct_ty: &Type, field_name: &str) -> bool {
        let Some(resolved) = self.resolve_nominal(struct_ty) else {
            return false;
        };
        matches!(
            &resolved.definition.body,
            DefinitionBody::Struct { fields, .. }
                if fields.iter().any(|f| f.name == field_name)
        )
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
        let resolved = self.resolve_nominal(ty)?;
        if !resolved.definition.is_newtype() {
            return None;
        }
        let DefinitionBody::Struct {
            fields: StructFields::Tuple(fields),
            ..
        } = &resolved.definition.body
        else {
            return None;
        };
        Some(fields[0].ty.clone())
    }

    pub(crate) fn as_enum(&self, ty: &Type) -> Option<String> {
        let resolved = self.resolve_nominal(ty)?;
        matches!(&resolved.definition.body, DefinitionBody::Enum { .. })
            .then(|| resolved.id.to_string())
    }

    pub(crate) fn enum_tuple_field_name(&self, ty: &Type, variant: &str, index: usize) -> String {
        let ty = &self.emit_shape_ty(ty);
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
}
