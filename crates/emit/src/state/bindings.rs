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
    Components(ComponentBinding),
    TupleComponents(TupleBinding),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TupleBinding {
    pub(crate) names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentBinding {
    pub(crate) value: String,
    pub(crate) status: String,
    pub(crate) payload_go_type: String,
    pub(crate) kind: ComponentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Option,
    Result,
}

impl BindingValue {
    pub(crate) fn as_go_name(&self) -> Option<&str> {
        match self {
            BindingValue::GoName(name) | BindingValue::GoConst(name) => Some(name.as_str()),
            BindingValue::InlineExpr(_)
            | BindingValue::Components(_)
            | BindingValue::TupleComponents(_) => None,
        }
    }

    pub(crate) fn is_discard(&self) -> bool {
        self.as_go_name() == Some("_")
    }

    pub(crate) fn can_reuse_pattern_subject(&self) -> bool {
        match self {
            Self::GoName(_) | Self::GoConst(_) | Self::Components(_) => true,
            Self::InlineExpr(_) | Self::TupleComponents(_) => false,
        }
    }

    pub(crate) fn is_go_const(&self) -> bool {
        matches!(self, BindingValue::GoConst(_))
    }
}
