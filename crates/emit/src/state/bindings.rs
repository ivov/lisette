use crate::plan::local::GoIdentifier;
use crate::plan::values::{GoExpression, Stability};

#[derive(Clone, Debug)]
pub(crate) struct InlineExpr {
    expression: GoExpression,
    /// The binding may be immutable while its subject is not.
    stability: Stability,
}

impl InlineExpr {
    pub(crate) fn new(expression: GoExpression, stability: Stability) -> Self {
        Self {
            expression,
            stability,
        }
    }

    pub(crate) fn expression(&self) -> &GoExpression {
        &self.expression
    }

    pub(crate) fn stability(&self) -> Stability {
        self.stability
    }
}

#[derive(Clone, Debug)]
pub(crate) enum BindingValue {
    GoName(GoIdentifier),
    GoConst(GoIdentifier),
    InlineExpr(InlineExpr),
    Components(ComponentBinding),
    TupleComponents(TupleBinding),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TupleBinding {
    pub(crate) names: Vec<GoIdentifier>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentBinding {
    pub(crate) value: GoIdentifier,
    pub(crate) status: GoIdentifier,
    pub(crate) payload_go_type: String,
    pub(crate) kind: ComponentKind,
    pub(crate) shared_payload: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Option,
    /// A `Result` whose error is the prelude `error`.
    ErrorResult,
    /// A `Result` with another error type, which no prelude constructor rebuilds.
    OtherResult,
}

impl ComponentKind {
    pub(crate) fn is_result(self) -> bool {
        matches!(self, Self::ErrorResult | Self::OtherResult)
    }

    /// The prelude function that rebuilds the whole value from its components.
    pub(crate) fn whole_value_constructor(self) -> Option<&'static str> {
        match self {
            Self::Option => Some("OptionFromCommaOk"),
            Self::ErrorResult => Some("ResultFromPair"),
            Self::OtherResult => None,
        }
    }
}

impl BindingValue {
    pub(crate) fn as_go_name(&self) -> Option<&str> {
        match self {
            BindingValue::GoName(name) | BindingValue::GoConst(name) => Some(name.spelling()),
            BindingValue::InlineExpr(_)
            | BindingValue::Components(_)
            | BindingValue::TupleComponents(_) => None,
        }
    }

    pub(crate) fn as_go_identifier(&self) -> Option<&GoIdentifier> {
        match self {
            BindingValue::GoName(name) | BindingValue::GoConst(name) => Some(name),
            BindingValue::InlineExpr(_)
            | BindingValue::Components(_)
            | BindingValue::TupleComponents(_) => None,
        }
    }

    pub(crate) fn mentions_local(&self, local: &GoIdentifier) -> bool {
        let Some(id) = local.id() else {
            return false;
        };
        match self {
            Self::GoName(name) | Self::GoConst(name) => name.id() == Some(id),
            Self::InlineExpr(inline) => inline.expression().node().mentions_local(local),
            Self::Components(components) => {
                components.value.id() == Some(id) || components.status.id() == Some(id)
            }
            Self::TupleComponents(tuple) => tuple.names.iter().any(|name| name.id() == Some(id)),
        }
    }

    pub(crate) fn mentions(&self, go_name: &str) -> bool {
        match self {
            BindingValue::GoName(name) | BindingValue::GoConst(name) => name.spelling() == go_name,
            BindingValue::InlineExpr(inline) => inline.expression().node().mentions(go_name),
            BindingValue::Components(components) => {
                components.value.spelling() == go_name || components.status.spelling() == go_name
            }
            BindingValue::TupleComponents(tuple) => {
                tuple.names.iter().any(|name| name.spelling() == go_name)
            }
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
