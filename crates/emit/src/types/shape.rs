use syntax::types::{CompoundKind, Type};

use crate::Planner;
use crate::types::native::NativeGoType;

#[derive(Debug, Clone)]
pub(crate) struct NativeShape {
    pub(crate) kind: NativeGoType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeShape {
    Range,
    RangeInclusive,
    RangeFrom,
    RangeTo,
    RangeToInclusive,
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
            Type::Compound { kind, .. } => {
                let native = match kind {
                    CompoundKind::Slice => NativeGoType::Slice,
                    CompoundKind::EnumeratedSlice => NativeGoType::EnumeratedSlice,
                    CompoundKind::Map => NativeGoType::Map,
                    CompoundKind::Channel => NativeGoType::Channel,
                    CompoundKind::Sender => NativeGoType::Sender,
                    CompoundKind::Receiver => NativeGoType::Receiver,
                    CompoundKind::Ref | CompoundKind::VarArgs => return None,
                };
                Some(NativeShape { kind: native })
            }
            Type::Simple(syntax::types::SimpleKind::String) => Some(NativeShape {
                kind: NativeGoType::String,
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
}
