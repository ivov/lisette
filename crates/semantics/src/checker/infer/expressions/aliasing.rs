use syntax::ast::{Expression, Literal, UnaryOperator};

fn place_root(expression: &Expression) -> Option<&Expression> {
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

/// Identifies which storage a place names, piece by piece. `None` when any
/// piece cannot be pinned down, so unknown places never compare equal.
pub(super) fn place_key(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Identifier { .. } => expression.get_var_name(),
        Expression::DotAccess {
            expression: inner,
            member,
            ..
        } => Some(format!("{}.{member}", place_key(inner.unwrap_parens())?)),
        Expression::IndexedAccess {
            expression: inner,
            index,
            ..
        } => {
            let index = index_key(index.unwrap_parens())?;
            Some(format!("{}[{index}]", place_key(inner.unwrap_parens())?))
        }
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => Some(format!("{}.*", place_key(inner.unwrap_parens())?)),
        _ => None,
    }
}

/// Identifies an index expression, `None` when its value cannot be pinned
/// down, so `rows[i + 1]` twice is one place while `rows[i + 1]` and
/// `rows[j + 1]` are not compared.
fn index_key(expression: &Expression) -> Option<String> {
    let expression = expression.unwrap_parens();
    match expression {
        Expression::Identifier { .. } => expression.get_var_name(),
        Expression::Literal { .. } => render_scalar(expression),
        // A selector, nested index, or deref compares by structural identity.
        Expression::DotAccess { .. }
        | Expression::IndexedAccess { .. }
        | Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => place_key(expression),
        Expression::Range {
            start,
            end,
            inclusive,
            ..
        } => {
            let endpoint = |e: &Option<Box<Expression>>| match e {
                None => Some(String::new()),
                Some(e) => index_key(e),
            };
            Some(format!(
                "({}{}{})",
                endpoint(start)?,
                if *inclusive { "..=" } else { ".." },
                endpoint(end)?
            ))
        }
        Expression::Binary {
            operator,
            left,
            right,
            ..
        } => Some(format!(
            "({}{operator:?}{})",
            index_key(left)?,
            index_key(right)?
        )),
        Expression::Unary {
            operator,
            expression: inner,
            ..
        } => Some(format!("({operator:?}{})", index_key(inner)?)),
        _ => None,
    }
}

/// Reconstructed source text for a place expression. Indexes that are not
/// literals or identifiers render as `..` placeholders.
pub(super) fn render_place(expression: &Expression) -> String {
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
