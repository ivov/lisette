use crate::plan::values::GoExpression;

#[derive(Clone, Debug)]
pub(crate) struct InlineExpr {
    expression: GoExpression,
}

impl InlineExpr {
    pub(crate) fn new(expression: GoExpression) -> Self {
        Self { expression }
    }

    pub(crate) fn expression(&self) -> &GoExpression {
        &self.expression
    }
}

#[derive(Clone, Debug)]
pub(crate) enum BindingValue {
    GoName(String),
    GoConst(String),
    InlineExpr(InlineExpr),
}

impl BindingValue {
    pub(crate) fn as_go_name(&self) -> Option<&str> {
        match self {
            BindingValue::GoName(name) | BindingValue::GoConst(name) => Some(name.as_str()),
            BindingValue::InlineExpr(_) => None,
        }
    }

    pub(crate) fn is_go_const(&self) -> bool {
        matches!(self, BindingValue::GoConst(_))
    }
}
