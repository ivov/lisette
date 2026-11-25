use syntax::ast::{CallTypeArguments, Expression};
use syntax::types::Type;

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct CallSite<'e> {
    pub(in crate::lowering) expression: &'e Expression,
    pub(in crate::lowering) callee: &'e Expression,
    pub(in crate::lowering) args: &'e [Expression],
    pub(in crate::lowering) spread: Option<&'e Expression>,
    pub(in crate::lowering) type_arguments: &'e CallTypeArguments,
    pub(in crate::lowering) ty: &'e Type,
}

impl<'e> CallSite<'e> {
    pub(in crate::lowering) fn of(expression: &'e Expression) -> Option<Self> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            type_arguments,
            ty,
            ..
        } = expression
        else {
            return None;
        };
        Some(Self {
            expression,
            callee,
            args,
            spread: spread.as_deref(),
            type_arguments,
            ty,
        })
    }

    pub(in crate::lowering) fn written_type_arguments(self) -> usize {
        self.type_arguments.annotations().len()
    }
}
