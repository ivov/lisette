use std::rc::Rc;

use rustc_hash::FxHashMap as HashMap;

use crate::Planner;
use crate::control_flow::fallible;
use crate::definitions::enum_layout::{EnumLayout, FieldTypeInfo, FieldTypeMap};
use crate::definitions::structs::{field_go_name_is_exported, is_raw_function_type};
use crate::names::go_name;
use syntax::ast::{Pattern, RestPattern, StructKind};
use syntax::containment::enum_payload_pointer_wrapped;
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Type, substitute};

impl Planner<'_> {
    pub(crate) fn go_name_for_binding(&self, pattern: &Pattern) -> Option<String> {
        let name = match pattern {
            Pattern::Identifier { identifier, .. } => identifier.as_str(),
            Pattern::AsBinding { name, .. } => name.as_str(),
            _ => return None,
        };
        if self.facts.is_unused_binding(pattern) {
            None
        } else {
            Some(name.to_string())
        }
    }

    pub(crate) fn go_name_for_rest_binding(&self, rest: &RestPattern) -> Option<String> {
        if let RestPattern::Bind { name, .. } = rest {
            if self.facts.is_unused_rest_binding(rest) {
                None
            } else {
                Some(name.to_string())
            }
        } else {
            None
        }
    }

    pub(crate) fn field_is_embedded(&self, struct_ty: &Type, field_name: &str) -> bool {
        let Some(resolved) = self.resolve_nominal(struct_ty) else {
            return false;
        };
        matches!(
            &resolved.definition.body,
            DefinitionBody::Struct { fields, .. }
                if fields.iter().any(|f| f.name == field_name && f.embedded)
        )
    }

    pub(crate) fn field_is_public(&self, struct_ty: &Type, field_name: &str) -> bool {
        let Some(resolved) = self.resolve_nominal(struct_ty) else {
            return false;
        };
        let id = resolved.id.as_str();

        match &resolved.definition.body {
            DefinitionBody::Struct { fields, .. } => {
                if let Some(field) = fields.iter().find(|f| f.name == field_name) {
                    return field_go_name_is_exported(field, resolved.definition.is_serialized());
                }
                let method_key = format!("{}.{}", id, field_name);
                self.facts
                    .definition(method_key.as_str())
                    .map(|d| d.visibility().is_public())
                    .unwrap_or(false)
            }
            DefinitionBody::Enum { .. } => {
                let method_key = format!("{}.{}", id, field_name);
                self.facts
                    .definition(method_key.as_str())
                    .map(|d| d.visibility().is_public())
                    .unwrap_or(false)
            }
            DefinitionBody::Interface { definition } => {
                resolved.definition.visibility.is_public()
                    && definition.methods.contains_key(field_name)
            }
            _ => false,
        }
    }

    pub(crate) fn method_needs_export(&self, method_name: &str) -> bool {
        self.facts.has_global_exported_method_name(method_name)
            || self.module.has_local_exported_method_name(method_name)
            || matches!(method_name, "string" | "goString" | "error")
    }

    pub(crate) fn type_uses_exported_members(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return false;
        };
        id.starts_with(go_name::PRELUDE_PREFIX)
            || self
                .facts
                .module_for_qualified_name(id.as_str())
                .is_some_and(|m| self.facts.is_foreign_module(m))
    }

    pub(crate) fn struct_field_is_exported(&self, ty: &Type, field: &str) -> bool {
        self.field_is_public(ty, field) || self.type_uses_exported_members(ty)
    }

    pub(crate) fn type_has_equals(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        let Some(id) = peeled.get_qualified_id() else {
            return false;
        };
        self.facts.usable_equals_from(id)
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
                    kind: StructKind::Tuple,
                    ..
                }
            )
        })
    }

    pub(crate) fn is_newtype_struct(&self, ty: &Type) -> bool {
        let Type::Nominal { params, .. } = ty.strip_refs() else {
            return false;
        };
        if !params.is_empty() {
            return false;
        }
        self.resolve_nominal(ty)
            .is_some_and(|resolved| resolved.definition.is_newtype())
    }

    pub(crate) fn get_newtype_underlying(&self, ty: &Type) -> Option<Type> {
        let resolved = self.resolve_nominal(ty)?;
        if let DefinitionBody::Struct {
            kind: StructKind::Tuple,
            fields,
            generics,
            ..
        } = &resolved.definition.body
            && fields.len() == 1
            && generics.is_empty()
        {
            return Some(fields[0].ty.clone());
        }

        None
    }

    pub(crate) fn peel_alias_id(&self, id: &str) -> String {
        syntax::types::peel_alias_id(id, |current| {
            let definition = self.facts.definition(current)?;
            if !matches!(definition.body, DefinitionBody::TypeAlias { .. }) {
                return None;
            }
            let Type::Nominal { id: next, .. } = definition.ty.unwrap_forall() else {
                return None;
            };
            Some(next.to_string())
        })
    }

    pub(crate) fn as_enum(&self, ty: &Type) -> Option<String> {
        let resolved = self.resolve_nominal(ty)?;
        matches!(&resolved.definition.body, DefinitionBody::Enum { .. })
            .then(|| resolved.id.to_string())
    }

    /// `Option<T>` where T is a concrete non-nilable Go value type, bridged
    /// as `*T`. Excludes `Option<Unknown>`/`Option<any>` (`interface{}`).
    pub(crate) fn is_non_nilable_option(&self, ty: &Type) -> bool {
        if !ty.is_option() {
            return false;
        }
        let inner = ty.ok_type();
        if inner.contains_unknown() || inner.has_name("any") {
            return false;
        }
        !self.facts.is_nilable_go_type(&inner)
    }

    /// Returns true if the Option wraps a Go interface type (not a pointer).
    /// These need `IsNilInterface` instead of `!= nil` to catch typed nils.
    pub(crate) fn is_interface_option(&self, ty: &Type) -> bool {
        if !ty.is_option() {
            return false;
        }
        let inner = ty.ok_type();
        self.facts.is_interface(&inner)
    }
}

impl Planner<'_> {
    pub(crate) fn enum_layout(&self, enum_id: &str) -> Option<Rc<EnumLayout>> {
        let file_id = self.file_namespace().file_id();
        if let Some(layout) = self.module.enum_layout(file_id, enum_id) {
            return Some(layout);
        }
        let layout = self.compute_enum_layout(enum_id)?;
        self.module
            .record_enum_layout(file_id, enum_id.to_string(), layout);
        self.module.enum_layout(file_id, enum_id)
    }

    fn compute_enum_layout(&self, enum_id: &str) -> Option<EnumLayout> {
        let Definition {
            name: Some(name),
            body: DefinitionBody::Enum {
                generics, variants, ..
            },
            ..
        } = self.facts.definition(enum_id)?
        else {
            return None;
        };

        if name == "Option" || name == "Result" || name == "Partial" {
            return None;
        }

        let mut field_types = FieldTypeMap::default();
        for (vi, variant) in variants.iter().enumerate() {
            for (fi, field) in variant.fields.iter().enumerate() {
                let mut go_type = self.go_type(&field.ty).code;
                let recursive = enum_payload_pointer_wrapped(enum_id, vi, fi, &field.ty, |id| {
                    self.facts.definition(id)
                });

                if recursive {
                    go_type = format!("*{}", go_type);
                }

                let is_function = !recursive && is_raw_function_type(&field.ty);
                field_types.insert(
                    (vi, fi),
                    FieldTypeInfo {
                        go_type,
                        is_function,
                        is_recursive: recursive,
                    },
                );
            }
        }

        Some(EnumLayout::new(enum_id, generics, variants, &field_types))
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

    pub(crate) fn enum_tuple_field_name(
        &self,
        enum_id: &str,
        variant_name: &str,
        field_index: usize,
    ) -> Option<String> {
        self.enum_layout(enum_id)?
            .tuple_field_name(variant_name, field_index)
    }

    pub(crate) fn get_enum_tuple_field_name(
        &self,
        ty: &Type,
        variant: &str,
        index: usize,
    ) -> String {
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
            && let Some(name) = self.enum_tuple_field_name(id, variant, index)
        {
            return name;
        }

        if index == 0 {
            variant.to_string()
        } else {
            format!("{}{}", variant, index)
        }
    }

    pub(crate) fn is_enum_field_pointer(&self, ty: &Type, variant: &str, index: usize) -> bool {
        if let Type::Nominal { id, .. } = ty
            && let Some(layout) = self.enum_layout(id.as_ref())
            && let Some(variant_layout) = layout.get_variant(variant)
            && let Some(field) = variant_layout.fields.get(index)
        {
            return field.go_type.starts_with('*');
        }
        false
    }

    /// True when the field's pointer comes from an explicit `Ref<T>` (not
    /// from auto-pointer recursion support). User `.*` deref relies on this.
    pub(crate) fn is_enum_field_source_ref(&self, ty: &Type, variant: &str, index: usize) -> bool {
        if let Type::Nominal { id, .. } = ty
            && let Some(Definition {
                body: DefinitionBody::Enum { variants, .. },
                ..
            }) = self.facts.definition(id.as_str())
        {
            for v in variants {
                if v.name == variant
                    && let Some(field) = v.fields.iter().nth(index)
                {
                    return field.ty.is_ref();
                }
            }
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
            }) = self.facts.definition(id.as_str())
        {
            let sub_map: HashMap<_, _> = generics
                .iter()
                .map(|g| g.name.clone())
                .zip(args.iter().cloned())
                .collect();
            for v in variants {
                if v.name == variant
                    && let Some(field) = v.fields.iter().nth(index)
                {
                    let concrete = substitute(&field.ty, &sub_map);
                    return concrete.is_unit() || concrete.is_never();
                }
            }
        }
        false
    }

    pub(crate) fn get_enum_struct_field_index(
        &self,
        ty: &Type,
        variant: &str,
        field_name: &str,
    ) -> Option<usize> {
        if let Type::Nominal { id, .. } = ty
            && let Some(Definition {
                body: DefinitionBody::Enum { variants, .. },
                ..
            }) = self.facts.definition(id.as_str())
        {
            for v in variants {
                if v.name == variant {
                    return v.fields.iter().position(|f| f.name == field_name);
                }
            }
        }
        None
    }
}
