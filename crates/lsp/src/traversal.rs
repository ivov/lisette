use std::iter::successors;

use syntax::ast::{Expression, Pattern, SelectArm, Span};

use crate::offset_in_span;

pub(crate) fn find_expression_at(items: &[Expression], offset: u32) -> Option<&Expression> {
    expression_ancestors(items, offset).last()
}

/// Walk from the outermost expression to the node at `offset`.
pub(crate) fn expression_ancestors(
    items: &[Expression],
    offset: u32,
) -> impl Iterator<Item = &Expression> {
    let root = items
        .iter()
        .find(|item| offset_in_span(offset, &item.get_span()));
    successors(root, move |expression| {
        child_containing_offset(expression, offset)
    })
}

pub(crate) fn shorthand_field_at(items: &[Expression], span: Span) -> Option<&str> {
    expression_ancestors(items, span.byte_offset).find_map(|expression| match expression {
        Expression::StructCall {
            field_assignments, ..
        } => field_assignments
            .iter()
            .find(|field| field.name_span == span && field.value.get_span() == span)
            .map(|field| field.name.as_str()),
        _ => expression_patterns(expression)
            .into_iter()
            .find_map(|pattern| pattern_shorthand_field_at(pattern, span)),
    })
}

fn expression_patterns(expression: &Expression) -> Vec<&Pattern> {
    match expression {
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            vec![&binding.pattern]
        }
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            params.iter().map(|binding| &binding.pattern).collect()
        }
        Expression::Match { arms, .. } => arms.iter().map(|arm| &arm.pattern).collect(),
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            vec![pattern]
        }
        Expression::Select { arms, .. } => arms
            .iter()
            .flat_map(|arm| match arm {
                SelectArm::Receive { binding, .. } => vec![binding.as_ref()],
                SelectArm::MatchReceive { arms, .. } => {
                    arms.iter().map(|arm| &arm.pattern).collect()
                }
                SelectArm::Send { .. } | SelectArm::WildCard { .. } => vec![],
            })
            .collect(),
        _ => vec![],
    }
}

fn pattern_shorthand_field_at(pattern: &Pattern, span: Span) -> Option<&str> {
    match pattern {
        Pattern::Struct { fields, .. } => fields.iter().find_map(|field| {
            if field.name_span == span && field.value.get_span() == span {
                Some(field.name.as_str())
            } else {
                pattern_shorthand_field_at(&field.value, span)
            }
        }),
        Pattern::Tuple { elements, .. }
        | Pattern::Or {
            patterns: elements, ..
        }
        | Pattern::EnumVariant {
            fields: elements, ..
        }
        | Pattern::Slice {
            prefix: elements, ..
        } => elements
            .iter()
            .find_map(|pattern| pattern_shorthand_field_at(pattern, span)),
        Pattern::AsBinding { pattern, .. } => pattern_shorthand_field_at(pattern, span),
        Pattern::Identifier { .. }
        | Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::WildCard { .. } => None,
    }
}

/// Find which immediate child of `expression` contains `offset`, without recursing.
fn child_containing_offset(expression: &Expression, offset: u32) -> Option<&Expression> {
    expression
        .children()
        .into_iter()
        .find(|child| offset_in_span(offset, &child.get_span()))
}

/// Find the deepest `Call` expression where `offset` falls in the arg region
/// (i.e. past the callee, inside the parentheses).
pub(crate) fn find_enclosing_call(items: &[Expression], offset: u32) -> Option<&Expression> {
    items
        .iter()
        .find_map(|item| find_call_in_expression(item, offset))
}

fn find_call_in_expression(expression: &Expression, offset: u32) -> Option<&Expression> {
    if !offset_in_span(offset, &expression.get_span()) {
        return None;
    }

    let mut current = expression;
    let mut deepest_call = None;

    loop {
        if let Expression::Call { expression, .. } = current {
            let s = expression.get_span();
            if offset >= s.byte_offset + s.byte_length {
                deepest_call = Some(current);
            }
        }

        match child_containing_offset(current, offset) {
            Some(child) => current = child,
            None => break,
        }
    }

    deepest_call
}

/// Find the `receiver_name` of the enclosing `impl` block for a given offset.
pub(crate) fn find_enclosing_impl_type(items: &[Expression], offset: u32) -> Option<&str> {
    items.iter().find_map(|item| {
        if let Expression::ImplBlock {
            receiver_name,
            methods,
            span,
            ..
        } = item
            && offset_in_span(offset, span)
            && methods
                .iter()
                .any(|m| offset_in_span(offset, &m.get_span()))
        {
            Some(receiver_name.as_str())
        } else {
            None
        }
    })
}
