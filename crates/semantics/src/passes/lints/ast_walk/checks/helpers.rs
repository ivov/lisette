use syntax::ast::{Expression, FormatStringPart, Literal, Pattern};
use syntax::types::unqualified_name;

use crate::passes::walk::visit_ast;

pub(super) use crate::passes::comparison::{
    expressions_equivalent, flip_comparison, is_side_effect_free, signed_integer_literal,
};

pub(super) fn is_zero_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 0, .. },
            ..
        }
    )
}

pub(super) fn is_one_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 1, .. },
            ..
        }
    )
}

pub(super) fn bool_literal(expression: &Expression) -> Option<bool> {
    if let Expression::Literal {
        literal: Literal::Boolean(b),
        ..
    } = expression
    {
        Some(*b)
    } else {
        None
    }
}

pub(super) fn is_empty_block(expression: &Expression) -> bool {
    matches!(expression, Expression::Block { items, .. } if items.is_empty())
}

pub(super) fn is_eager_safe(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Identifier { .. } => true,
        Expression::Literal { literal, .. } => literal_is_eager_safe(literal),
        Expression::DotAccess {
            expression: inner, ..
        } => is_eager_safe(inner),
        _ => false,
    }
}

fn literal_is_eager_safe(literal: &Literal) -> bool {
    match literal {
        Literal::Slice(elements) => elements.iter().all(is_eager_safe),
        Literal::FormatString(parts) => parts
            .iter()
            .all(|part| matches!(part, FormatStringPart::Text(_))),
        _ => true,
    }
}

pub(super) fn span_text<'a>(source: &'a str, expression: &Expression) -> Option<&'a str> {
    let span = expression.get_span();
    source.get(span.byte_offset as usize..span.end() as usize)
}

// The single identifier bound by a one-field enum-variant pattern such as
// `Some(v)` or `Err(e)`, if the variant name matches.
pub(super) fn enum_variant_binding<'a>(pattern: &'a Pattern, variant: &str) -> Option<&'a str> {
    let Pattern::EnumVariant {
        identifier,
        fields,
        rest,
        ..
    } = pattern
    else {
        return None;
    };
    if unqualified_name(identifier) != variant || *rest || fields.len() != 1 {
        return None;
    }
    let Pattern::Identifier {
        identifier: name, ..
    } = &fields[0]
    else {
        return None;
    };
    Some(name.as_str())
}

// `?`, `return`, `break`, and `continue` target a scope outside a synthesized
// closure, so a body containing them cannot be moved into one.
pub(super) fn has_escaping_control_flow(body: &Expression) -> bool {
    let mut found = false;
    visit_ast(
        std::slice::from_ref(body),
        &mut |node| {
            found |= matches!(
                node,
                Expression::Return { .. }
                    | Expression::Propagate { .. }
                    | Expression::Break { .. }
                    | Expression::Continue { .. }
            );
        },
        &mut |_| {},
    );
    found
}
