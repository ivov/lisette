use syntax::ast::{Expression, Literal, UnaryOperator};
use syntax::program::CallKind;

use crate::checker::infer::InferCtx;
use crate::checker::infer::addressability::check_is_non_addressable;
use crate::checker::infer::carry_mut::{
    can_carry_mutation_across_fn_boundary, clone_recipe_is_known, clone_severs_alias,
};

impl InferCtx<'_, '_> {
    /// Reject a mutable binding that would alias a carry-mut place. A
    /// `clone()` of the place is exempt only when the clone actually severs.
    pub(super) fn check_mut_binding_alias(&mut self, binding_name: &str, value: &Expression) {
        let expr = value.unwrap_parens();
        let (place, via_clone) = match clone_call_receiver(expr) {
            Some(receiver) => (receiver.unwrap_parens(), true),
            None => (expr, false),
        };
        if !is_aliasing_place(place) {
            return;
        }
        let rooted_in_binding = place_root_name(place)
            .is_some_and(|root| self.scopes.lookup_binding_id(&root).is_some());
        if !rooted_in_binding {
            return;
        }
        let ty = if via_clone {
            place.get_type()
        } else {
            value.get_type()
        };
        if !can_carry_mutation_across_fn_boundary(&ty, &self.env, self.store) {
            return;
        }
        if via_clone && !clone_recipe_is_known(&ty, &self.env, self.store) {
            return;
        }
        let clone_severs = clone_severs_alias(&ty, &self.env, self.store);
        if via_clone && clone_severs {
            return;
        }
        let source = render_place(place);
        let addressable = check_is_non_addressable(place, &self.env).is_none();
        let diagnostic = if via_clone {
            diagnostics::infer::mut_binding_clone_does_not_sever(
                binding_name,
                &source,
                addressable,
                value.get_span(),
            )
        } else {
            diagnostics::infer::mut_binding_aliases(
                binding_name,
                &source,
                addressable,
                clone_severs,
                value.get_span(),
            )
        };
        self.sink.push(diagnostic);
    }

    /// Source of a `.clone()` of a local carry-mut place that the clone does
    /// not fully sever, or `None` when the clone is fresh, opaque, or severing.
    pub(super) fn non_severing_clone_source(&self, value: &Expression) -> Option<String> {
        let receiver = clone_call_receiver(value.unwrap_parens())?;
        let place = receiver.unwrap_parens();
        if !is_aliasing_place(place) {
            return None;
        }
        let root = place_root_name(place)?;
        self.scopes.lookup_binding_id(&root)?;
        let ty = place.get_type();
        if !can_carry_mutation_across_fn_boundary(&ty, &self.env, self.store) {
            return None;
        }
        if !clone_recipe_is_known(&ty, &self.env, self.store) {
            return None;
        }
        if clone_severs_alias(&ty, &self.env, self.store) {
            return None;
        }
        Some(render_place(place))
    }
}

/// The receiver of a `clone()` call written as `x.clone()`, `Type.clone(x)`,
/// or `Slice.clone(x)`. `None` for a free function named `clone` (UFCS).
pub(super) fn clone_call_receiver(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        call_kind,
        ..
    } = expression
    else {
        return None;
    };
    match callee.unwrap_parens() {
        Expression::DotAccess {
            expression, member, ..
        } if member == "clone"
            && args.is_empty()
            && !matches!(call_kind, Some(CallKind::UfcsMethod)) =>
        {
            Some(expression)
        }
        Expression::Identifier { .. }
            if args.len() == 1
                && matches!(
                    call_kind,
                    Some(CallKind::NativeMethodIdentifier(_) | CallKind::ReceiverMethodUfcs { .. })
                )
                && callee
                    .get_var_name()
                    .is_some_and(|name| name.ends_with(".clone")) =>
        {
            args.first()
        }
        _ => None,
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
