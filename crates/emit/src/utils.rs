use syntax::ast::{Expression, FormatStringPart, Literal, UnaryOperator};
use syntax::program::DotAccessKind;

macro_rules! write_line {
    ($dst:expr, $($arg:tt)*) => {
        { use std::fmt::Write as _; writeln!($dst, $($arg)*).unwrap() }
    };
}
pub(crate) use write_line;

pub(crate) fn wrap_if_struct_literal(condition: String) -> String {
    if condition.contains('{') {
        format!("({})", condition)
    } else {
        condition
    }
}

pub(crate) fn receiver_name(type_name: &str) -> String {
    type_name
        .trim_start_matches('*')
        .split('[')
        .next()
        .unwrap_or(type_name)
        .chars()
        .next()
        .unwrap_or('x')
        .to_lowercase()
        .to_string()
}

fn receiver_generic_names(receiver_generics: &str) -> impl Iterator<Item = &str> {
    receiver_generics
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

pub(crate) fn synthesized_receiver_name(type_name: &str, receiver_generics: &str) -> String {
    let generic_names: Vec<&str> = receiver_generic_names(receiver_generics).collect();
    let mut receiver = receiver_name(type_name);
    if generic_names.contains(&receiver.as_str()) {
        receiver = format!("{receiver}{receiver}");
        let mut counter = 2;
        while generic_names.contains(&receiver.as_str()) {
            receiver = format!("{}{}", receiver_name(type_name), counter);
            counter += 1;
        }
    }
    receiver
}

pub(crate) fn synthesized_local_name(
    base: &str,
    receiver: &str,
    receiver_generics: &str,
) -> String {
    let reserved = |name: &str| {
        name == receiver || receiver_generic_names(receiver_generics).any(|g| g == name)
    };
    if !reserved(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !reserved(candidate))
        .expect("freshening counter is unbounded")
}

/// Group consecutive parameters with the same Go type: `a int, b int` → `a, b int`.
pub(crate) fn group_params(params: &[(String, String)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    if params.len() == 1 {
        return format!("{} {}", params[0].0, params[0].1);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut names: Vec<&str> = vec![&params[0].0];
    let mut current_ty = &params[0].1;

    for param in &params[1..] {
        if param.1 == *current_ty {
            names.push(&param.0);
        } else {
            parts.push(format!("{} {}", names.join(", "), current_ty));
            names.clear();
            names.push(&param.0);
            current_ty = &param.1;
        }
    }
    parts.push(format!("{} {}", names.join(", "), current_ty));
    parts.join(", ")
}

/// Whether an expression contains a function call (i.e. is side-effectful).
/// Temp-lifted forms (if/match/block) return false — after emission they're
/// just variable names.
pub(crate) fn contains_call(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Call { .. } => true,
        Expression::Binary { left, right, .. } => contains_call(left) || contains_call(right),
        Expression::Unary { expression, .. }
        | Expression::DotAccess { expression, .. }
        | Expression::Cast { expression, .. }
        | Expression::Reference { expression, .. } => contains_call(expression),
        Expression::IndexedAccess {
            expression, index, ..
        } => contains_call(expression) || contains_call(index),
        Expression::Tuple { elements, .. } => elements.iter().any(contains_call),
        Expression::StructCall {
            field_assignments,
            spread,
            ..
        } => {
            field_assignments.iter().any(|f| contains_call(&f.value))
                || spread.as_expression().is_some_and(contains_call)
        }
        Expression::Literal {
            literal: Literal::Slice(elements),
            ..
        } => elements.iter().any(contains_call),
        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => parts.iter().any(|part| match part {
            FormatStringPart::Expression(e) => contains_call(e),
            FormatStringPart::Text(_) => false,
        }),
        Expression::Range { start, end, .. } => {
            start.as_deref().is_some_and(contains_call) || end.as_deref().is_some_and(contains_call)
        }
        Expression::If { .. }
        | Expression::IfLet { .. }
        | Expression::Match { .. }
        | Expression::Block { .. }
        | Expression::Loop { .. }
        | Expression::Propagate { .. }
        | Expression::TryBlock { .. }
        | Expression::Select { .. } => false,
        _ => false,
    }
}

pub(crate) fn is_scalar_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_),
            ..
        }
    )
}

pub(crate) fn is_order_sensitive(expression: &Expression) -> bool {
    !(is_scalar_literal(expression)
        || matches!(expression.unwrap_parens(), Expression::Identifier { .. }))
}

/// True for any expression except a scalar constant, i.e. one whose value can
/// be invalidated by a later sibling's setup, or is a call we should not
/// re-evaluate.
pub(crate) fn observable_after_mutation(expression: &Expression) -> bool {
    !is_scalar_literal(expression)
}

pub(crate) fn reads_mutable_operand(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::IndexedAccess { .. } | Expression::Call { .. } => true,
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => true,
        Expression::DotAccess {
            expression,
            dot_access_kind,
            ..
        } => match dot_access_kind {
            Some(
                DotAccessKind::StructField { .. }
                | DotAccessKind::TupleStructField { .. }
                | DotAccessKind::TupleElement,
            ) => true,
            _ => reads_mutable_operand(expression),
        },
        _ => false,
    }
}
