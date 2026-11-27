use syntax::EcoString;
use syntax::ast::{
    BinaryOperator, BindingId, Expression, IdentifierResolution, Literal, UnaryOperator,
};

pub(super) fn place_root(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Identifier { .. } => Some(expression),
        Expression::DotAccess {
            expression: inner, ..
        }
        | Expression::IndexedAccess {
            expression: inner, ..
        }
        | Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => place_root(inner.unwrap_parens()),
        _ => None,
    }
}

/// Name of the identifier a place expression is rooted at.
pub(super) fn place_root_name(expression: &Expression) -> Option<String> {
    place_root(expression)?.get_var_name()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PlaceKey {
    Binding(BindingId),
    Definition(EcoString),
    Field(Box<PlaceKey>, EcoString),
    Index(Box<PlaceKey>, Box<IndexKey>),
    Dereference(Box<PlaceKey>),
}

impl PlaceKey {
    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.is_prefix_of(other) || other.is_prefix_of(self)
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        if self == other {
            return true;
        }
        match other {
            Self::Field(parent, _) | Self::Index(parent, _) | Self::Dereference(parent) => {
                self.is_prefix_of(parent)
            }
            Self::Binding(_) | Self::Definition(_) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IndexKey {
    Place(PlaceKey),
    Integer(u64),
    String(String),
    Boolean(bool),
    Range(Option<Box<IndexKey>>, Option<Box<IndexKey>>, bool),
    Binary(BinaryOperator, Box<IndexKey>, Box<IndexKey>),
    Unary(UnaryOperator, Box<IndexKey>),
}

pub(crate) fn place_key(expression: &Expression) -> Option<PlaceKey> {
    Some(match expression.unwrap_parens() {
        Expression::Identifier { resolution, .. } => match resolution {
            IdentifierResolution::Binding(id) => PlaceKey::Binding(*id),
            IdentifierResolution::Definition { name, .. } => {
                PlaceKey::Definition(name.as_eco().clone())
            }
            IdentifierResolution::Method { method, .. } => {
                PlaceKey::Definition(method.to_string().into())
            }
            IdentifierResolution::Unresolved => return None,
        },
        Expression::DotAccess {
            expression, member, ..
        } => PlaceKey::Field(Box::new(place_key(expression)?), member.clone()),
        Expression::IndexedAccess {
            expression, index, ..
        } => PlaceKey::Index(
            Box::new(place_key(expression)?),
            Box::new(index_key(index)?),
        ),
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression,
            ..
        } => PlaceKey::Dereference(Box::new(place_key(expression)?)),
        _ => return None,
    })
}

fn index_key(expression: &Expression) -> Option<IndexKey> {
    Some(match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Integer { value, .. },
            ..
        } => IndexKey::Integer(*value),
        Expression::Literal {
            literal: Literal::String { value, .. },
            ..
        } => IndexKey::String(value.clone()),
        Expression::Literal {
            literal: Literal::Boolean(value),
            ..
        } => IndexKey::Boolean(*value),
        Expression::Range {
            start,
            end,
            inclusive,
            ..
        } => {
            let endpoint = |value: &Option<Box<Expression>>| match value {
                Some(value) => Some(Some(Box::new(index_key(value)?))),
                None => Some(None),
            };
            IndexKey::Range(endpoint(start)?, endpoint(end)?, *inclusive)
        }
        Expression::Binary {
            operator,
            left,
            right,
            ..
        } => IndexKey::Binary(
            *operator,
            Box::new(index_key(left)?),
            Box::new(index_key(right)?),
        ),
        Expression::Unary {
            operator,
            expression,
            ..
        } if *operator != UnaryOperator::Deref => {
            IndexKey::Unary(operator.clone(), Box::new(index_key(expression)?))
        }
        other => IndexKey::Place(place_key(other)?),
    })
}

/// Reconstructed source text for a place expression. Indexes that are not
/// literals or identifiers render as `..` placeholders.
pub(crate) fn render_place(expression: &Expression) -> String {
    let expression = expression.unwrap_parens();
    match expression {
        Expression::Identifier { .. } => expression.get_var_name().unwrap_or_default(),
        Expression::DotAccess {
            expression: inner,
            member,
            ..
        } => format!("{}.{member}", render_place(inner.unwrap_parens())),
        Expression::IndexedAccess {
            expression: inner,
            index,
            ..
        } => format!(
            "{}[{}]",
            render_place(inner.unwrap_parens()),
            render_index(index.unwrap_parens())
        ),
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => format!("{}.*", render_place(inner.unwrap_parens())),
        Expression::Call { expression, .. } => {
            format!("{}()", render_place(expression.unwrap_parens()))
        }
        _ => String::new(),
    }
}

fn render_index(index: &Expression) -> String {
    match index {
        Expression::Range {
            start,
            end,
            inclusive,
            ..
        } => {
            let endpoint = |e: &Option<Box<Expression>>| match e {
                None => Some(String::new()),
                Some(e) => render_scalar(e.unwrap_parens()),
            };
            match (endpoint(start), endpoint(end)) {
                (Some(s), Some(e)) => {
                    format!("{s}{}{e}", if *inclusive { "..=" } else { ".." })
                }
                _ => "..".to_string(),
            }
        }
        other => render_scalar(other).unwrap_or_else(|| "..".to_string()),
    }
}

fn render_scalar(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Identifier { .. } => expression.get_var_name(),
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { value, text } => {
                Some(text.clone().unwrap_or_else(|| value.to_string()))
            }
            Literal::String { value, .. } => Some(format!("{value:?}")),
            Literal::Boolean(value) => Some(value.to_string()),
            _ => None,
        },
        _ => None,
    }
}
