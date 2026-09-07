use crate::plan::values::GoExpression;

#[derive(Clone, Debug)]
pub(crate) struct InlineExpr {
    expression: GoExpression,
    /// Emitter vars the expression references, recorded as uses on substitution.
    refs: Vec<String>,
}

impl InlineExpr {
    pub(crate) fn new(
        expression: GoExpression,
        refs: Vec<String>,
        contains_deferred_evaluation: bool,
    ) -> Self {
        Self {
            expression: expression.with_deferred_evaluation(contains_deferred_evaluation),
            refs,
        }
    }

    pub(crate) fn expression(&self) -> &GoExpression {
        &self.expression
    }

    pub(crate) fn refs(&self) -> &[String] {
        &self.refs
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
