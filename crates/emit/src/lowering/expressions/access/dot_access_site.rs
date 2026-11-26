use ecow::EcoString;
use syntax::ast::Expression;
use syntax::program::DotAccessResolution;
use syntax::types::Type;

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct DotAccessSite<'e> {
    pub(in crate::lowering) expression: &'e Expression,
    pub(in crate::lowering) receiver: &'e Expression,
    pub(in crate::lowering) member: &'e EcoString,
    pub(in crate::lowering) ty: &'e Type,
    pub(in crate::lowering) resolution: &'e DotAccessResolution,
}

impl<'e> DotAccessSite<'e> {
    pub(in crate::lowering) fn of(expression: &'e Expression) -> Option<Self> {
        let Expression::DotAccess {
            expression: receiver,
            member,
            ty,
            resolution,
            ..
        } = expression
        else {
            return None;
        };
        Some(Self {
            expression,
            receiver,
            member,
            ty,
            resolution,
        })
    }

    pub(in crate::lowering) fn reads_newtype(&self) -> bool {
        matches!(
            self.resolution,
            DotAccessResolution::TupleStructField { is_newtype: true }
        )
    }
}
