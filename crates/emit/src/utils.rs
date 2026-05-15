use std::borrow::Cow;
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

/// Whether `var` appears as a standalone identifier in emitted Go,
/// ignoring string-literal contents.
pub(crate) fn output_references_var(output: &str, var: &str) -> bool {
    let masked = mask_go_string_literals(output);
    let bytes = masked.as_bytes();
    masked
        .match_indices(var)
        .any(|(abs, _)| is_at_token_boundary(bytes, abs, var.len()))
}

fn is_at_token_boundary(bytes: &[u8], pos: usize, token_len: usize) -> bool {
    let before_ok = pos == 0 || {
        let c = bytes[pos - 1];
        !c.is_ascii_alphanumeric() && c != b'_'
    };
    let after = pos + token_len;
    let after_ok = after >= bytes.len() || {
        let c = bytes[after];
        !c.is_ascii_alphanumeric() && c != b'_'
    };
    before_ok && after_ok
}

/// Replace Go string/rune/raw-string contents with spaces, preserving byte
/// positions. Borrows when no quote is present.
pub(crate) fn mask_go_string_literals(go_text: &str) -> Cow<'_, str> {
    if !go_text.bytes().any(|b| matches!(b, b'"' | b'\'' | b'`')) {
        return Cow::Borrowed(go_text);
    }
    let bytes = go_text.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'"' | b'\'') => i = mask_literal(bytes, i, quote, true, &mut out),
            b'`' => i = mask_literal(bytes, i, b'`', false, &mut out),
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b'"' | b'\'' | b'`') {
                    i += 1;
                }
                out.push_str(&go_text[start..i]);
            }
        }
    }
    Cow::Owned(out)
}

fn mask_literal(bytes: &[u8], start: usize, quote: u8, escapes: bool, out: &mut String) -> usize {
    out.push(quote as char);
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if escapes && b == b'\\' && i + 1 < bytes.len() {
            out.push_str("  ");
            i += 2;
        } else if b == quote {
            out.push(quote as char);
            return i + 1;
        } else {
            out.push(' ');
            i += 1;
        }
    }
    i
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

// -- Eval-order staging ----------------------------------------------------

pub(crate) fn is_order_sensitive(expression: &Expression) -> bool {
    !matches!(
        expression.unwrap_parens(),
        Expression::Literal { .. } | Expression::Identifier { .. }
    )
}

/// True for any expression except a pure literal — i.e. one whose value can
/// be invalidated by a later sibling's setup, or is a call we should not
/// re-evaluate.
pub(crate) fn observable_after_mutation(expression: &Expression) -> bool {
    !matches!(expression.unwrap_parens(), Expression::Literal { .. })
}

/// Guard that snapshots the output length and inserts `_ = var\n` on `finish()`
/// if the variable was never referenced in the output emitted since creation.
pub(crate) struct DiscardGuard {
    pre_len: usize,
    var: String,
}

impl DiscardGuard {
    pub(crate) fn new(output: &str, var: &str) -> Self {
        Self {
            pre_len: output.len(),
            var: var.to_string(),
        }
    }

    pub(crate) fn finish(self, output: &mut String) {
        discard_if_unused(output, self.pre_len, &self.var);
    }
}

fn discard_if_unused(output: &mut String, pre_len: usize, var: &str) {
    if !output_references_var(&output[pre_len..], var) {
        output.insert_str(pre_len, &format!("_ = {}\n", var));
    }
}

/// Check if the output ends with a diverging statement (break/continue/return/panic).
pub(crate) fn output_ends_with_diverge(output: &str) -> bool {
    output
        .trim_end()
        .lines()
        .next_back()
        .is_some_and(is_diverge_line)
}

fn is_diverge_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "break"
        || trimmed.starts_with("break ")
        || trimmed == "continue"
        || trimmed.starts_with("continue ")
        || trimmed == "return"
        || trimmed.starts_with("return ")
        || trimmed.starts_with("panic(")
}
