use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct TupleLiteral<'e> {
    pub(in crate::lowering) expression: &'e Expression,
    pub(in crate::lowering) elements: &'e [Expression],
    pub(in crate::lowering) ty: &'e Type,
}

impl<'e> TupleLiteral<'e> {
    pub(in crate::lowering) fn of(expression: &'e Expression) -> Option<Self> {
        let Expression::Tuple { elements, ty, .. } = expression else {
            return None;
        };
        Some(Self {
            expression,
            elements,
            ty,
        })
    }
}
