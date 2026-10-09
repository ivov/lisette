use syntax::program::NativeTypeKind;
use syntax::types::Type;

use crate::Planner;

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

    /// Unlike `NativeTypeKind::from_type`, arrays are excluded.
    pub(crate) fn native_shape(&self, ty: &Type) -> Option<NativeTypeKind> {
        NativeTypeKind::from_type(&self.emit_shape_ty(ty))
            .filter(|kind| *kind != NativeTypeKind::Array)
    }

    /// True when `ty` resolves to the given native kind after alias peeling.
    pub(crate) fn is_native_shape(&self, ty: &Type, kind: NativeTypeKind) -> bool {
        self.native_shape(ty).is_some_and(|shape| shape == kind)
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
