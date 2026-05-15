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

/// Try to negate a simple comparison by flipping its operator.
/// Returns `None` for compound expressions (`&&`/`||`) or non-comparisons.
/// Only considers operators at the outermost paren depth that contains any
/// token character, so a comparison inside a call argument or other inner
/// parenthesized context is not mistaken for the expression being negated.
/// Used by unary-not emission and let-else condition negation.
pub(crate) fn try_flip_comparison(expression: &str) -> Option<String> {
    const FLIPS: &[(&str, &str)] = &[
        (" == ", " != "),
        (" != ", " == "),
        (" <= ", " > "),
        (" >= ", " < "),
        (" < ", " >= "),
        (" > ", " <= "),
    ];

    let masked = mask_go_string_literals(expression);
    let bytes = masked.as_bytes();

    // The main operator may sit inside outer parens (e.g. `("x" == s)`), so
    // pick the smallest depth where a non-paren, non-whitespace byte appears
    // and look for operators at exactly that depth.
    let mut depth: i32 = 0;
    let mut target: Option<i32> = None;
    for &b in bytes {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b' ' | b'\t' | b'\n' | b'\r' => {}
            _ => target = Some(target.map_or(depth, |d| d.min(depth))),
        }
    }
    let target = target.unwrap_or(0);

    let mut depth: i32 = 0;
    let mut found: Option<(usize, usize, &str)> = None;
    for i in 0..bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => {
                depth += 1;
                continue;
            }
            b')' | b']' | b'}' => {
                depth -= 1;
                continue;
            }
            _ => {}
        }
        if depth != target {
            continue;
        }
        let rest = &bytes[i..];
        if rest.starts_with(b" && ") || rest.starts_with(b" || ") {
            return None;
        }
        if found.is_none() {
            for (op, flipped) in FLIPS {
                if rest.starts_with(op.as_bytes()) {
                    found = Some((i, op.len(), flipped));
                    break;
                }
            }
        }
    }

    let (position, op_len, flipped) = found?;
    let lhs = &expression[..position];
    let rhs = &expression[position + op_len..];
    Some(format!("{}{}{}", lhs, flipped, rhs))
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

/// Collapse single-use bindings: `VAR := EXPR\n{return VAR|TARGET = VAR|_ = VAR}\n`
/// becomes `{return EXPR|TARGET = EXPR|_ = EXPR}\n`.
///
/// Only collapses when VAR appears nowhere else in the region (true single-use).
/// Pattern bindings are always pure field accesses, so reordering is safe.
pub(crate) fn inline_trivial_bindings(output: &mut String, pre_len: usize) {
    // Loop to fixpoint: collapsing one binding can expose another (e.g.
    // `y := pair.Second` collapsing into the return line then makes
    // `x := pair.First; return x + …` collapsible too).
    loop {
        let region = &output[pre_len..];
        let lines: Vec<&str> = region.lines().collect();

        let mut result = String::with_capacity(region.len());
        let mut i = 0;
        let mut changed = false;

        while i < lines.len() {
            if i + 1 < lines.len()
                && let Some((var, expression)) = parse_binding(lines[i])
                && let Some(collapsed) = try_inline_binding(lines[i + 1], var, expression)
            {
                let used_elsewhere = lines
                    .iter()
                    .enumerate()
                    .any(|(j, line)| j != i && j != i + 1 && output_references_var(line, var));

                if !used_elsewhere {
                    result.push_str(&collapsed);
                    result.push('\n');
                    i += 2;
                    changed = true;
                    continue;
                }
            }
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
        }

        if !changed {
            break;
        }
        output.truncate(pre_len);
        output.push_str(&result);
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

fn is_go_identifier(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Parse a short variable declaration: `VAR := EXPR` → `(VAR, EXPR)`.
fn parse_binding(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(" := ")?;
    let var = &line[..idx];
    if !is_go_identifier(var) {
        return None;
    }
    let expression = &line[idx + 4..];
    Some((var, expression))
}

/// Try to collapse the next line with a single-use binding variable.
fn try_inline_binding(next_line: &str, var: &str, expression: &str) -> Option<String> {
    if let Some(rest) = next_line.strip_prefix("return ")
        && rest == var
    {
        return Some(format!("return {}", expression));
    }
    if let Some(rest) = next_line.strip_prefix("_ = ")
        && rest == var
    {
        return Some(format!("_ = {}", expression));
    }
    if let Some(eq_position) = next_line.find(" = ")
        && !next_line.contains(":=")
    {
        let target = &next_line[..eq_position];
        let value = &next_line[eq_position + 3..];
        if value == var && target != var {
            return Some(format!("{} = {}", target, expression));
        }
    }
    // Skip `for` headers: Lisette emits capture temps (`_bound_N`) precisely
    // to evaluate the bound once; inlining would re-evaluate per iteration.
    // Skip `&VAR` use sites: substituting a function reference (`&add1`,
    // `&pkg.Fn`, `&s.method`) is invalid Go — VAR must be an addressable local.
    if is_pure_dot_path(expression)
        && !next_line.trim_start().starts_with("for ")
        && let Some(pos) = single_token_position(next_line, var)
        && !(pos > 0 && next_line.as_bytes()[pos - 1] == b'&')
    {
        return Some(format!(
            "{}{}{}",
            &next_line[..pos],
            expression,
            &next_line[pos + var.len()..]
        ));
    }
    None
}

/// True for `IDENT` or `IDENT(.IDENT)*` — no parens, brackets, or operators.
fn is_pure_dot_path(s: &str) -> bool {
    let mut want_ident_start = true;
    for c in s.chars() {
        if want_ident_start {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return false;
            }
            want_ident_start = false;
        } else if c == '.' {
            want_ident_start = true;
        } else if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    !want_ident_start
}

/// Byte offset of `token` if it appears exactly once as a complete identifier
/// in `line`, ignoring string-literal contents.
fn single_token_position(line: &str, token: &str) -> Option<usize> {
    let masked = mask_go_string_literals(line);
    let bytes = masked.as_bytes();
    let mut iter = masked
        .match_indices(token)
        .filter(|(abs, _)| is_at_token_boundary(bytes, *abs, token.len()))
        .map(|(abs, _)| abs);
    let first = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    Some(first)
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
