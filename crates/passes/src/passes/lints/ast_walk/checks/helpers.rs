use syntax::ast::{Expression, FormatStringPart, Literal, Pattern, Span};
use syntax::types::{SimpleKind, Type, unqualified_name};

use crate::passes::walk::visit_ast;
use semantics::store::Store;

pub(super) use crate::passes::comparison::{
    expressions_equivalent, flip_comparison, is_side_effect_free, signed_integer_literal,
};

pub(super) fn is_float_operand(store: &Store, expression: &Expression) -> bool {
    let resolved = store.deep_resolve_alias(&expression.get_type());
    if let Some(kind) = resolved.underlying_simple_kind() {
        return kind.is_float();
    }
    if let Type::Nominal { id, .. } = &resolved
        && let Some(definition) = store.get_definition(id.as_str())
    {
        return definition
            .ty()
            .underlying_simple_kind()
            .is_some_and(SimpleKind::is_float);
    }
    false
}

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

pub(super) fn is_pure_mapper(expression: &Expression) -> bool {
    matches!(expression.unwrap_parens(), Expression::Lambda { .. }) || is_eager_safe(expression)
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

pub(super) fn wrapped_single_arg<'a>(
    expression: &'a Expression,
    variant: &str,
) -> Option<&'a Expression> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let Expression::Identifier { value, .. } = callee.unwrap_parens() else {
        return None;
    };
    if unqualified_name(value) != variant {
        return None;
    }
    Some(&args[0])
}

pub(super) fn is_bare_identifier(expression: &Expression, name: &str) -> bool {
    matches!(expression.unwrap_parens(), Expression::Identifier { value, .. }
        if value.as_str() == name)
}

pub(super) fn method_call<'a>(
    expression: &'a Expression,
    name: &str,
) -> Option<(&'a Expression, &'a [Expression], &'a Span)> {
    let Expression::Call {
        expression: callee,
        args,
        span,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    (member.as_str() == name).then_some((receiver.as_ref(), args.as_slice(), span))
}

pub(super) fn unary_lambda(expression: &Expression) -> Option<(&str, &Expression)> {
    let Expression::Lambda { params, body, .. } = expression.unwrap_parens() else {
        return None;
    };
    let [param] = params.as_slice() else {
        return None;
    };
    let Pattern::Identifier { identifier, .. } = &param.pattern else {
        return None;
    };
    Some((identifier.as_str(), unwrap_block(body)))
}

fn unwrap_block(expression: &Expression) -> &Expression {
    match expression.unwrap_parens() {
        Expression::Block { items, .. } if items.len() == 1 => unwrap_block(&items[0]),
        other => other,
    }
}

pub(super) fn is_identity_lambda(expression: &Expression) -> bool {
    unary_lambda(expression).is_some_and(|(param, body)| is_bare_identifier(body, param))
}

pub(super) fn wraps_binding(body: &Expression, variant: &str, binding: &str) -> bool {
    wrapped_single_arg(body, variant).is_some_and(|arg| is_bare_identifier(arg, binding))
}

pub(super) fn is_none_pattern(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::EnumVariant { identifier, fields, rest, .. }
        if unqualified_name(identifier) == "None" && fields.is_empty() && !*rest)
}

pub(super) fn mentions_identifier(expression: &Expression, name: &str) -> bool {
    let mut found = false;
    visit_ast(
        std::slice::from_ref(expression),
        &mut |node| {
            if let Expression::Identifier { value, .. } = node {
                found |= value.as_str() == name;
            }
        },
        &mut |_| {},
    );
    found
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
