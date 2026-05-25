use syntax::types::{CompoundKind, Symbol, Type};

use crate::Planner;
use crate::names::go_name;
use crate::types::native::NativeGoType;

#[derive(Debug, Clone)]
pub(crate) struct NativeShape {
    pub(crate) kind: NativeGoType,
    pub(crate) params: Vec<Type>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeShape {
    Range,
    RangeInclusive,
    RangeFrom,
    RangeTo,
    RangeToInclusive,
}

#[derive(Debug, Clone)]
pub(crate) struct GoImportedShape {
    #[allow(dead_code)]
    pub(crate) id: Symbol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionKind {
    Slice,
    Map,
}

#[derive(Debug, Clone)]
pub(crate) struct NullableCollectionShape {
    pub(crate) kind: CollectionKind,
    pub(crate) key_ty: Option<Type>,
    pub(crate) element_option_ty: Type,
}

impl Planner<'_> {
    /// Normalize a type for emit decisions by walking aliases and reference
    /// wrappers to a fixed point. Only peels real type aliases (not newtypes).
    pub(crate) fn emit_shape_ty(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        loop {
            let without_refs = current.strip_refs();
            let peeled = self.facts.peel_alias(&without_refs);
            if peeled == current {
                return peeled;
            }
            current = peeled;
        }
    }

    /// Classify a type as a real native/prelude collection or string after
    /// alias peeling. Stricter than `NativeTypeKind::from_type` because it
    /// only accepts `Type::Compound` shapes (not nominals whose leaf name
    /// happens to be `Slice`/`Map`/etc.) plus `SimpleKind::String`.
    pub(crate) fn native_shape(&self, ty: &Type) -> Option<NativeShape> {
        let resolved = self.emit_shape_ty(ty);
        match resolved {
            Type::Compound { kind, args } => {
                let native = match kind {
                    CompoundKind::Slice => NativeGoType::Slice,
                    CompoundKind::EnumeratedSlice => NativeGoType::EnumeratedSlice,
                    CompoundKind::Map => NativeGoType::Map,
                    CompoundKind::Channel => NativeGoType::Channel,
                    CompoundKind::Sender => NativeGoType::Sender,
                    CompoundKind::Receiver => NativeGoType::Receiver,
                    CompoundKind::Ref | CompoundKind::VarArgs => return None,
                };
                Some(NativeShape {
                    kind: native,
                    params: args,
                })
            }
            Type::Simple(syntax::types::SimpleKind::String) => Some(NativeShape {
                kind: NativeGoType::String,
                params: Vec::new(),
            }),
            _ => None,
        }
    }

    /// True when `ty` resolves to the given native kind after alias peeling.
    pub(crate) fn is_native_shape(&self, ty: &Type, kind: NativeGoType) -> bool {
        self.native_shape(ty).is_some_and(|s| s.kind == kind)
    }

    /// Classify a type as one of the prelude range structs after alias
    /// peeling. Unrelated types named `Range` (Go imports, local types) do
    /// not match.
    pub(crate) fn range_shape(&self, ty: &Type) -> Option<RangeShape> {
        let resolved = self.emit_shape_ty(ty);
        let Type::Nominal { id, .. } = resolved else {
            return None;
        };
        match id.as_str() {
            "prelude.Range" => Some(RangeShape::Range),
            "prelude.RangeInclusive" => Some(RangeShape::RangeInclusive),
            "prelude.RangeFrom" => Some(RangeShape::RangeFrom),
            "prelude.RangeTo" => Some(RangeShape::RangeTo),
            "prelude.RangeToInclusive" => Some(RangeShape::RangeToInclusive),
            _ => None,
        }
    }

    /// Classify a type as a Go-imported nominal after alias peeling.
    /// `type MyFlag = flag.Flag` and `Ref<MyFlag>` both match.
    pub(crate) fn go_imported_shape(&self, ty: &Type) -> Option<GoImportedShape> {
        let resolved = self.emit_shape_ty(ty);
        let Type::Nominal { id, .. } = resolved else {
            return None;
        };
        if go_name::is_go_import(id.as_str()) {
            Some(GoImportedShape { id })
        } else {
            None
        }
    }

    /// Classify a type as a nullable collection (`Slice<Option<...>>` or
    /// `Map<K, Option<...>>`) after alias peeling. Element option types are
    /// also alias-aware via the inner option classifier.
    pub(crate) fn nullable_collection_shape(&self, ty: &Type) -> Option<NullableCollectionShape> {
        let shape = self.native_shape(ty)?;
        match shape.kind {
            NativeGoType::Slice => {
                let element_ty = shape.params.into_iter().next()?;
                let resolved_option = self.pointer_bridged_option_ty(&element_ty)?;
                Some(NullableCollectionShape {
                    kind: CollectionKind::Slice,
                    key_ty: None,
                    element_option_ty: resolved_option,
                })
            }
            NativeGoType::Map => {
                let mut iter = shape.params.into_iter();
                let key_ty = iter.next()?;
                let val_ty = iter.next()?;
                let resolved_option = self.pointer_bridged_option_ty(&val_ty)?;
                Some(NullableCollectionShape {
                    kind: CollectionKind::Map,
                    key_ty: Some(key_ty),
                    element_option_ty: resolved_option,
                })
            }
            _ => None,
        }
    }

    fn pointer_bridged_option_ty(&self, ty: &Type) -> Option<Type> {
        let resolved = self.emit_shape_ty(ty);
        if !resolved.is_option() {
            return None;
        }
        let inner = resolved.ok_type();
        if self.facts.is_nilable_go_type(&inner) {
            return Some(resolved);
        }
        if inner.contains_unknown() || inner.has_name("any") {
            return None;
        }
        Some(resolved)
    }
}
