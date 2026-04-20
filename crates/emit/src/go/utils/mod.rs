mod optimize;
mod staging;

pub(crate) use optimize::{
    inline_trivial_bindings, optimize_function_body, optimize_region, output_ends_with_diverge,
};
pub(crate) use staging::{DiscardGuard, Staged, is_order_sensitive};

use syntax::ast::Expression;

macro_rules! write_line {
    ($dst:expr, $($arg:tt)*) => {
        { use std::fmt::Write as _; writeln!($dst, $($arg)*).unwrap() }
    };
}
pub(crate) use write_line;

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

/// Check if emitted Go output references `var` as a standalone identifier
/// (not as a substring of another identifier like `p` in `tmp_1`).
pub(crate) fn output_references_var(output: &str, var: &str) -> bool {
    let var_bytes = var.as_bytes();
    let out_bytes = output.as_bytes();
    let mut start = 0;
    while start + var.len() <= output.len() {
        if let Some(position) = output[start..].find(var) {
            let abs = start + position;
            let before_ok = abs == 0 || {
                let c = out_bytes[abs - 1];
                !c.is_ascii_alphanumeric() && c != b'_'
            };
            let after = abs + var_bytes.len();
            let after_ok = after >= out_bytes.len() || {
                let c = out_bytes[after];
                !c.is_ascii_alphanumeric() && c != b'_'
            };
            if before_ok && after_ok {
                return true;
            }
            start = abs + 1;
        } else {
            break;
        }
    }
    false
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

/// Try to negate a simple comparison by flipping its operator.
/// Returns `None` for compound expressions (`&&`/`||`) or non-comparisons.
/// Used by unary-not emission and let-else condition negation.
pub(crate) fn try_flip_comparison(expression: &str) -> Option<String> {
    if expression.contains(" && ") || expression.contains(" || ") {
        return None;
    }
    for (op, flipped) in [
        (" == ", " != "),
        (" != ", " == "),
        (" <= ", " > "),
        (" >= ", " < "),
        (" < ", " >= "),
        (" > ", " <= "),
    ] {
        if let Some(position) = expression.find(op) {
            let lhs = &expression[..position];
            let rhs = &expression[position + op.len()..];
            return Some(format!("{}{}{}", lhs, flipped, rhs));
        }
    }
    None
}

pub(crate) fn requires_temp_var(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Block { .. }
            | Expression::Loop { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::Select { .. }
    )
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
        e if requires_temp_var(e) => false,
        _ => false,
    }
}
