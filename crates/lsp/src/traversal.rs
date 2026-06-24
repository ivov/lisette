use syntax::ast::Expression;

use crate::offset_in_span;

pub(crate) fn find_expression_at(items: &[Expression], offset: u32) -> Option<&Expression> {
    items
        .iter()
        .find_map(|item| find_in_expression(item, offset))
}

fn find_in_expression(expression: &Expression, offset: u32) -> Option<&Expression> {
    if !offset_in_span(offset, &expression.get_span()) {
        return None;
    }

    let mut current = expression;
    loop {
        match child_containing_offset(current, offset) {
            Some(child) => current = child,
            None => return Some(current),
        }
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
