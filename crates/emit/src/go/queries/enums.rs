use rustc_hash::FxHashMap as HashMap;

use crate::Emitter;
use crate::go::control_flow::fallible;
use crate::go::definitions::enum_layout::{EnumLayout, FieldTypeMap};
use crate::go::names::go_name;
use syntax::program::Definition;
use syntax::types::{Type, substitute};

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
                    let mut go_type = self.go_type(&field.ty).code;

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
        match ty.resolve() {
            Type::Constructor { id, .. } => id == enum_id,
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
        let resolved = ty.resolve();

        if resolved.is_option() {
            return match variant {
                "Some" => fallible::OPTION_SOME_FIELD.to_string(),
                _ => variant.to_string(),
            };
        }

        if resolved.is_result() {
            return match (variant, index) {
                ("Ok", 0) => fallible::RESULT_OK_FIELD.to_string(),
                ("Err", 0) => fallible::RESULT_ERR_FIELD.to_string(),
                _ => variant.to_string(),
            };
        }

        if resolved.is_partial() {
            return match (variant, index) {
                ("Ok", 0) => fallible::PARTIAL_OK_FIELD.to_string(),
                ("Err", 0) => fallible::PARTIAL_ERR_FIELD.to_string(),
                ("Both", 0) => fallible::PARTIAL_OK_FIELD.to_string(),
                ("Both", 1) => fallible::PARTIAL_ERR_FIELD.to_string(),
                _ => variant.to_string(),
            };
        }

        if let Type::Constructor { id, .. } = &resolved
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
        let resolved = ty.resolve();
        if let Type::Constructor { id, .. } = &resolved
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
        let resolved = ty.resolve();
        if let Type::Constructor { id, .. } = &resolved
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
        let resolved = ty.resolve();
        if let Type::Constructor {
            id, params: args, ..
        } = &resolved
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
                    let concrete = substitute(&field.ty.resolve(), &sub_map);
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
        let resolved = ty.resolve();
        if let Type::Constructor { id, .. } = &resolved
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
