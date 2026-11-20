use crate::EmitFacts;
use syntax::program::NativeTypeKind;
pub(crate) use syntax::types::RangeKind as RangeShape;
use syntax::types::Type;

impl EmitFacts<'_> {
    pub(crate) fn emit_shape_ty(&self, ty: &Type) -> Type {
        let mut current = ty.clone();
        loop {
            let without_refs = current.strip_refs();
            let peeled = self.peel_alias(&without_refs);
            if peeled == current {
                return peeled;
            }
            current = peeled;
        }
    }

    pub(crate) fn native_kind(&self, ty: &Type) -> Option<NativeTypeKind> {
        NativeTypeKind::from_type(&self.emit_shape_ty(ty))
    }

    pub(crate) fn range_shape(&self, ty: &Type) -> Option<RangeShape> {
        RangeShape::of(&self.emit_shape_ty(ty))
    }

    pub(crate) fn native_shape(&self, ty: &Type) -> Option<NativeTypeKind> {
        self.native_kind(ty)
            .filter(|kind| *kind != NativeTypeKind::Array)
    }

    pub(crate) fn is_native_shape(&self, ty: &Type, kind: NativeTypeKind) -> bool {
        self.native_shape(ty).is_some_and(|shape| shape == kind)
    }
}
