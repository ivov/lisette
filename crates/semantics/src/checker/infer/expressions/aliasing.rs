use syntax::ast::{Expression, Literal, UnaryOperator};
use syntax::types::{CompoundKind, Type};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::checker::infer::addressability::check_is_non_addressable;

impl InferCtx<'_, '_> {
    /// A mutable binding initialized or reassigned from a place expression of
    /// a directly aliasable type would share the source's backing storage, so
    /// writes through the binding would also be visible through the source.
    pub(super) fn check_mut_binding_alias(&mut self, binding_name: &str, value: &Expression) {
        let place = value.unwrap_parens();
        if !is_aliasing_place(place) {
            return;
        }
        if !self.is_directly_aliasable(&value.get_type()) {
            return;
        }
        let source = render_place(place);
        let addressable = check_is_non_addressable(place, &self.env).is_none();
        self.sink.push(diagnostics::infer::mut_binding_aliases(
            binding_name,
            &source,
            addressable,
            value.get_span(),
        ));
    }

    fn is_directly_aliasable(&self, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env);
        let peeled = self.store.peel_alias(&resolved);
        matches!(
            peeled,
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::Map | CompoundKind::EnumeratedSlice,
                ..
            }
        )
    }
}

/// Name of the identifier a place expression is rooted at.
pub(super) fn place_root_name(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Identifier { .. } => expression.get_var_name(),
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
        } => place_root_name(inner.unwrap_parens()),
        _ => None,
    }
}

/// Identifiers, field accesses, indexed accesses, and derefs rooted at one of
/// these.
fn is_aliasing_place(expression: &Expression) -> bool {
    match expression {
        Expression::Identifier { .. } => true,
        Expression::DotAccess {
            expression: inner, ..
        } => is_aliasing_place(inner.unwrap_parens()),
        Expression::IndexedAccess {
            expression: inner, ..
        } => is_aliasing_place(inner.unwrap_parens()),
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => is_aliasing_place(inner.unwrap_parens()),
        _ => false,
    }
}

/// Reconstructed source text for a place expression. Indexes that are not
/// literals or identifiers render as `..` placeholders.
fn render_place(expression: &Expression) -> String {
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
