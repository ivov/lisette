use rustc_hash::FxHashMap as HashMap;

use crate::Emitter;
use crate::control_flow::fallible;
use crate::definitions::enum_layout::{EnumLayout, FieldTypeMap};
use crate::names::go_name;
use syntax::ast::{Pattern, RestPattern, StructKind};
use syntax::program::Definition;
use syntax::types::{Type, substitute};

// -- Definition queries ----------------------------------------------------

impl Emitter<'_> {
    pub(crate) fn go_name_for_binding(&self, pattern: &Pattern) -> Option<String> {
        let name = match pattern {
            Pattern::Identifier { identifier, .. } => identifier.as_str(),
            Pattern::AsBinding { name, .. } => name.as_str(),
            _ => return None,
        };
        if self.ctx.unused.is_unused_binding(pattern) {
            None
        } else {
            Some(name.to_string())
        }
    }

    pub(crate) fn go_name_for_rest_binding(&self, rest: &RestPattern) -> Option<String> {
        if let RestPattern::Bind { name, .. } = rest {
            if self.ctx.unused.is_unused_rest_binding(rest) {
                None
            } else {
                Some(name.to_string())
            }
        } else {
            None
        }
    }

    pub(crate) fn field_is_public(&self, struct_ty: &Type, field_name: &str) -> bool {
        let resolved = self.peel_alias(&struct_ty.strip_refs());

        let Type::Nominal { id, .. } = &resolved else {
            return false;
        };

        match self.ctx.definitions.get(id.as_str()) {
            Some(Definition::Struct { fields, .. }) => {
                if let Some(field) = fields.iter().find(|f| f.name == field_name) {
                    if field.visibility.is_public() {
                        return true;
                    }
                    // Also export fields that have serialization tags (e.g. #[json])
                    let tag_key = format!("{}.{}", id, field_name);
                    return self.module.tag_exported_fields.contains(&tag_key);
                }
                let method_key = format!("{}.{}", id, field_name);
                self.ctx
                    .definitions
                    .get(method_key.as_str())
                    .map(|d| d.visibility().is_public())
                    .unwrap_or(false)
            }
            Some(Definition::Enum { .. }) => {
                let method_key = format!("{}.{}", id, field_name);
                self.ctx
                    .definitions
                    .get(method_key.as_str())
                    .map(|d| d.visibility().is_public())
                    .unwrap_or(false)
            }
            Some(Definition::Interface {
                visibility,
                definition,
                ..
            }) => {
                if visibility.is_public() && definition.methods.contains_key(field_name) {
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    pub(crate) fn method_needs_export(&self, method_name: &str) -> bool {
        self.module.exported_method_names.contains(method_name)
            || matches!(method_name, "string" | "goString" | "error")
    }

    pub(crate) fn has_field(&self, struct_ty: &Type, field_name: &str) -> bool {
        let Type::Nominal { id, .. } = struct_ty.strip_refs() else {
            return false;
        };
        matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::Struct { fields, .. })
                if fields.iter().any(|f| f.name == field_name)
        )
    }

    pub(crate) fn is_tuple_struct_type(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return false;
        };

        matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::Struct {
                kind: StructKind::Tuple,
                ..
            })
        )
    }

    pub(crate) fn is_newtype_struct(&self, ty: &Type) -> bool {
        let Type::Nominal { id, params, .. } = ty.strip_refs() else {
            return false;
        };
        if !params.is_empty() {
            return false;
        }
        self.ctx
            .definitions
            .get(id.as_str())
            .is_some_and(|d| d.is_newtype())
    }

    pub(crate) fn is_go_value_enum(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return false;
        };
        matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::ValueEnum { .. })
        )
    }

    pub(crate) fn get_newtype_underlying(&self, ty: &Type) -> Option<Type> {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return None;
        };

        if let Some(Definition::Struct {
            kind: StructKind::Tuple,
            fields,
            generics,
            ..
        }) = self.ctx.definitions.get(id.as_str())
            && fields.len() == 1
            && generics.is_empty()
        {
            return Some(fields[0].ty.clone());
        }

        None
    }

    pub(crate) fn peel_alias(&self, ty: &Type) -> Type {
        let mut current = ty.unwrap_forall().clone();
        let mut seen: Vec<String> = Vec::new();
        loop {
            let Type::Nominal { id, .. } = &current else {
                return current;
            };
            if seen.iter().any(|s| s == id.as_str()) {
                return current;
            }
            let Some(Definition::TypeAlias { ty: alias_ty, .. }) =
                self.ctx.definitions.get(id.as_str())
            else {
                return current;
            };
            seen.push(id.to_string());
            current = alias_ty.unwrap_forall().clone();
        }
    }

    pub(crate) fn peel_alias_id(&self, id: &str) -> String {
        let mut current = id.to_string();
        let mut seen: Vec<String> = Vec::new();
        loop {
            if seen.iter().any(|s| s == &current) {
                return current;
            }
            let Some(Definition::TypeAlias { ty: alias_ty, .. }) =
                self.ctx.definitions.get(current.as_str())
            else {
                return current;
            };
            let Type::Nominal { id: next, .. } = alias_ty.unwrap_forall() else {
                return current;
            };
            seen.push(current);
            current = next.to_string();
        }
    }

    pub(crate) fn as_enum(&self, ty: &Type) -> Option<String> {
        let Type::Nominal { id, .. } = self.peel_alias(ty) else {
            return None;
        };

        if matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::Enum { .. })
        ) {
            Some(id.to_string())
        } else {
            None
        }
    }

    pub(crate) fn as_interface(&self, ty: &Type) -> Option<String> {
        let Type::Nominal { id, .. } = self.peel_alias(ty) else {
            return None;
        };

        if matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::Interface { .. })
        ) {
            Some(id.to_string())
        } else {
            None
        }
    }

    pub(crate) fn is_go_imported_type(ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return false;
        };
        go_name::is_go_import(&id)
    }

    pub(crate) fn is_nullable_option(&self, ty: &Type) -> bool {
        if !ty.is_option() {
            return false;
        }
        let inner = ty.ok_type();
        inner.is_ref() || self.as_interface(&inner).is_some() || self.is_go_function_alias(&inner)
    }

    /// Returns true if the Option wraps a Go interface type (not a pointer).
    /// These need `IsNilInterface` instead of `!= nil` to catch typed nils.
    pub(crate) fn is_interface_option(&self, ty: &Type) -> bool {
        if !ty.is_option() {
            return false;
        }
        let inner = ty.ok_type();
        self.as_interface(&inner).is_some()
    }

    pub(crate) fn is_go_nullable(&self, ty: &Type) -> bool {
        self.is_nullable_option(ty) || self.nullable_collection_element_ty(ty).is_some()
    }

    pub(crate) fn nullable_collection_element_ty(&self, ty: &Type) -> Option<Type> {
        if ty.has_name("Slice") {
            let elem_ty = ty.inner()?;
            if self.is_nullable_option(&elem_ty) {
                return Some(elem_ty);
            }
        } else if ty.has_name("Map") {
            let params = ty.get_type_params()?;
            let val_ty = params.get(1)?.clone();
            if self.is_nullable_option(&val_ty) {
                return Some(val_ty);
            }
        }
        None
    }
}

// -- Enum layout queries ---------------------------------------------------

impl Emitter<'_> {
    /// Pre-compute enum layouts for all known enum definitions.
    ///
    /// Must be called after `collect_impl_bounds()` since layouts need merged bounds.
    /// Replaces the previous lazy `ensure_enum_layout()` pattern.
    pub(crate) fn collect_enum_layouts(&mut self) {
        let enum_defs: Vec<_> = self
            .ctx
            .definitions
            .iter()
            .filter_map(|(id, definition)| {
                if let Definition::Enum {
                    name,
                    generics,
                    variants,
                    ..
                } = definition
                {
                    if name == "Option" || name == "Result" || name == "Partial" {
                        return None;
                    }
                    Some((id.to_string(), generics.clone(), variants.clone()))
                } else {
                    None
                }
            })
            .collect();

        for (enum_id, generics, variants) in enum_defs {
            let mut field_types = FieldTypeMap::default();
            for (vi, variant) in variants.iter().enumerate() {
                for (fi, field) in variant.fields.iter().enumerate() {
                    let mut go_type = self.go_type_as_string(&field.ty);

                    if Self::is_recursive_type(&field.ty, &enum_id) {
                        go_type = format!("*{}", go_type);
                    }

                    field_types.insert((vi, fi), go_type);
                }
            }

            let enum_name = go_name::unqualified_name(&enum_id);
            let generics = self.merge_impl_bounds(enum_name, &generics);

            let layout = EnumLayout::new(&enum_id, &generics, &variants, &field_types);
            self.module.enum_layouts.insert(enum_id, layout);
        }
    }

    fn is_recursive_type(ty: &Type, enum_id: &str) -> bool {
        match ty.unwrap_forall() {
            Type::Nominal { id, .. } => id == enum_id,
            _ => false,
        }
    }

    pub(crate) fn enum_struct_field_name(
        &self,
        enum_id: &str,
        variant_name: &str,
        field_name: &str,
    ) -> Option<String> {
        self.module
            .enum_layouts
            .get(enum_id)?
            .struct_field_name(variant_name, field_name)
    }

    pub(crate) fn enum_tuple_field_name(
        &self,
        enum_id: &str,
        variant_name: &str,
        field_index: usize,
    ) -> Option<String> {
        self.module
            .enum_layouts
            .get(enum_id)?
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
            && let Some(layout) = self.module.enum_layouts.get(id.as_ref())
            && let Some(variant_layout) = layout.get_variant(variant)
            && let Some(field) = variant_layout.fields.get(index)
        {
            return field.go_type.starts_with('*');
        }
        false
    }

    /// Check if an enum field's pointer is due to an explicit `Ref<T>` in the source,
    /// as opposed to an auto-pointer added for recursive enum support.
    ///
    /// When the user writes `Ref<T>` explicitly, the binding should remain a pointer
    /// so the user's `.*` (postfix deref) works correctly. When the pointer is added
    /// automatically for recursion, the binding should be dereferenced transparently.
    pub(crate) fn is_enum_field_source_ref(&self, ty: &Type, variant: &str, index: usize) -> bool {
        if let Type::Nominal { id, .. } = ty
            && let Some(Definition::Enum { variants, .. }) = self.ctx.definitions.get(id.as_str())
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
            && let Some(Definition::Enum {
                generics, variants, ..
            }) = self.ctx.definitions.get(id.as_str())
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
            && let Some(Definition::Enum { variants, .. }) = self.ctx.definitions.get(id.as_str())
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
