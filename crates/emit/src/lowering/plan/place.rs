use crate::target::ir::values::GoExpression;
use syntax::types::Type;

pub(in crate::lowering) enum PlacePlan<'a> {
    Statement,
    Return,
    Assign {
        local: &'a GoExpression,
        target_ty: Option<&'a Type>,
    },
}

impl PlacePlan<'_> {
    pub(in crate::lowering) fn is_return(&self) -> bool {
        matches!(self, PlacePlan::Return)
    }
}
