use crate::Emitter;
use crate::go::names::go_name;
use syntax::ast::{Pattern, RestPattern, StructKind};
use syntax::program::Definition;
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn go_name_for_binding(&self, pattern: &Pattern) -> Option<String> {
        if let Pattern::Identifier { identifier, .. } = pattern {
            if self.ctx.unused.is_unused_binding(pattern) {
                None
            } else {
                Some(identifier.to_string())
            }
        } else {
            None
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
        let resolved = self.peel_alias(&struct_ty.resolve().strip_refs());

        let Type::Constructor { id, .. } = &resolved else {
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
    }

    pub(crate) fn has_field(&self, struct_ty: &Type, field_name: &str) -> bool {
        let Type::Constructor { id, .. } = struct_ty.resolve().strip_refs() else {
            return false;
        };
        matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::Struct { fields, .. })
                if fields.iter().any(|f| f.name == field_name)
        )
    }

    pub(crate) fn is_tuple_struct_type(&self, ty: &Type) -> bool {
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
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
        let Type::Constructor { id, params, .. } = ty.resolve().strip_refs() else {
            return false;
        };

        if !params.is_empty() {
            return false;
        }

        matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::Struct {
                kind: StructKind::Tuple,
                fields,
                generics,
                ..
            }) if fields.len() == 1 && generics.is_empty()
        )
    }

    pub(crate) fn is_go_value_enum(&self, ty: &Type) -> bool {
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
            return false;
        };
        matches!(
            self.ctx.definitions.get(id.as_str()),
            Some(Definition::ValueEnum { .. })
        )
    }

    pub(crate) fn get_newtype_underlying(&self, ty: &Type) -> Option<Type> {
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
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
        let mut current = ty.resolve();
        let mut seen: Vec<String> = Vec::new();
        loop {
            let Type::Constructor { id, .. } = &current else {
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
            current = alias_ty.resolve();
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
            let Type::Constructor { id: next, .. } = alias_ty.resolve() else {
                return current;
            };
            seen.push(current);
            current = next.to_string();
        }
    }

    pub(crate) fn as_enum(&self, ty: &Type) -> Option<String> {
        let Type::Constructor { id, .. } = self.peel_alias(ty) else {
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
        let Type::Constructor { id, .. } = self.peel_alias(ty) else {
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
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
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
        let resolved = ty.resolve();
        if resolved.has_name("Slice") {
            let elem_ty = resolved.inner()?;
            if self.is_nullable_option(&elem_ty) {
                return Some(elem_ty);
            }
        } else if resolved.has_name("Map") {
            let params = resolved.get_type_params()?;
            let val_ty = params.get(1)?.clone();
            if self.is_nullable_option(&val_ty) {
                return Some(val_ty);
            }
        }
        None
    }
}
