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
fn child_containing_offset<'a>(expression: &'a Expression, offset: u32) -> Option<&'a Expression> {
    let c = |e: &'a Expression| -> Option<&'a Expression> {
        offset_in_span(offset, &e.get_span()).then_some(e)
    };

    match expression {
        Expression::Function { body, .. } | Expression::Lambda { body, .. } => c(body),

        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => items.iter().find_map(c),

        Expression::Let {
            value, else_block, ..
        } => c(value).or_else(|| else_block.as_deref().and_then(c)),

        Expression::Call {
            expression,
            args,
            spread,
            ..
        } => c(expression)
            .or_else(|| args.iter().find_map(c))
            .or_else(|| spread.as_ref().as_ref().and_then(c)),

        Expression::If {
            condition,
            consequence,
            alternative,
            ..
        } => c(condition)
            .or_else(|| c(consequence))
            .or_else(|| c(alternative)),

        Expression::IfLet {
            scrutinee,
            consequence,
            alternative,
            ..
        } => c(scrutinee)
            .or_else(|| c(consequence))
            .or_else(|| c(alternative)),

        Expression::Match { subject, arms, .. } => c(subject).or_else(|| {
            arms.iter().find_map(|arm| {
                arm.guard
                    .as_deref()
                    .and_then(c)
                    .or_else(|| c(&arm.expression))
            })
        }),

        Expression::Tuple { elements, .. } => elements.iter().find_map(c),

        Expression::StructCall {
            field_assignments,
            spread,
            ..
        } => field_assignments
            .iter()
            .find_map(|fa| c(&fa.value))
            .or_else(|| spread.as_expression().and_then(c)),

        Expression::DotAccess { expression, .. }
        | Expression::Return { expression, .. }
        | Expression::Propagate { expression, .. }
        | Expression::Unary { expression, .. }
        | Expression::Paren { expression, .. }
        | Expression::Const { expression, .. }
        | Expression::Reference { expression, .. }
        | Expression::Task { expression, .. }
        | Expression::Defer { expression, .. }
        | Expression::Cast { expression, .. } => c(expression),

        Expression::Assignment { target, value, .. } => c(target).or_else(|| c(value)),

        Expression::ImplBlock { methods, .. } => methods.iter().find_map(c),

        Expression::Binary { left, right, .. } => c(left).or_else(|| c(right)),

        Expression::Loop { body, .. } => c(body),

        Expression::While {
            condition, body, ..
        } => c(condition).or_else(|| c(body)),

        Expression::WhileLet {
            scrutinee, body, ..
        } => c(scrutinee).or_else(|| c(body)),

        Expression::For { iterable, body, .. } => c(iterable).or_else(|| c(body)),

        Expression::Break { value, .. } => value.as_deref().and_then(c),

        Expression::IndexedAccess {
            expression, index, ..
        } => c(expression).or_else(|| c(index)),

        Expression::Select { arms, .. } => arms.iter().find_map(|arm| {
            use syntax::ast::SelectArmPattern;
            match &arm.pattern {
                SelectArmPattern::Receive {
                    receive_expression,
                    body,
                    ..
                } => c(receive_expression).or_else(|| c(body)),
                SelectArmPattern::Send {
                    send_expression,
                    body,
                } => c(send_expression).or_else(|| c(body)),
                SelectArmPattern::MatchReceive {
                    receive_expression,
                    arms: match_arms,
                } => c(receive_expression).or_else(|| {
                    match_arms.iter().find_map(|arm| {
                        arm.guard
                            .as_deref()
                            .and_then(c)
                            .or_else(|| c(&arm.expression))
                    })
                }),
                SelectArmPattern::WildCard { body } => c(body),
            }
        }),

        Expression::Range { start, end, .. } => start
            .as_deref()
            .and_then(c)
            .or_else(|| end.as_deref().and_then(c)),

        Expression::Literal { literal, .. } => {
            use syntax::ast::{FormatStringPart, Literal};
            match literal {
                Literal::Slice(elements) => elements.iter().find_map(c),
                Literal::FormatString(parts) => parts.iter().find_map(|p| match p {
                    FormatStringPart::Expression(e) => c(e),
                    FormatStringPart::Text(_) => None,
                }),
                _ => None,
            }
        }

        Expression::Identifier { .. }
        | Expression::VariableDeclaration { .. }
        | Expression::RawGo { .. }
        | Expression::Enum { .. }
        | Expression::ValueEnum { .. }
        | Expression::Struct { .. }
        | Expression::TypeAlias { .. }
        | Expression::ModuleImport { .. }
        | Expression::Interface { .. }
        | Expression::Unit { .. }
        | Expression::Continue { .. }
        | Expression::NoOp => None,
    }
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
