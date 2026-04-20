use syntax::ast::Expression;

macro_rules! write_line {
    ($dst:expr, $($arg:tt)*) => {
        { use std::fmt::Write as _; writeln!($dst, $($arg)*).unwrap() }
    };
}
pub(crate) use write_line;

pub(crate) fn is_order_sensitive(expression: &Expression) -> bool {
    !matches!(
        expression.unwrap_parens(),
        Expression::Literal { .. } | Expression::Identifier { .. }
    )
}

/// Result of emitting a sub-expression to a separate buffer.
/// `setup` contains any statements the emitter produced (temp vars, etc.).
/// `value` is the final expression string.
pub(crate) struct Staged {
    pub setup: String,
    pub value: String,
    /// Whether the emitted value contains a call (side-effectful).
    /// Detected via `value.contains('(')` on the Go output string.
    pub has_side_effects: bool,
}

impl Staged {
    pub(crate) fn new(setup: String, value: String) -> Self {
        let has_side_effects = value.contains('(');
        Self {
            setup,
            value,
            has_side_effects,
        }
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

fn discard_if_unused(output: &mut String, pre_len: usize, var: &str) {
    if !output_references_var(&output[pre_len..], var) {
        output.insert_str(pre_len, &format!("_ = {}\n", var));
    }
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

fn is_go_identifier(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Run all peephole optimization passes on a region of emitted Go output.
///
/// When `temp_var` is `Some`, uses the given variable name for return inlining.
/// When `None`, auto-discovers `var X T ... return X` patterns.
pub(crate) fn optimize_region(output: &mut String, pre_len: usize, temp_var: Option<&str>) {
    let discovered;
    let resolved = match temp_var {
        Some(v) => Some(v),
        None => {
            discovered = find_inlinable_var(&output[pre_len..]);
            discovered.as_deref()
        }
    };
    if let Some(v) = resolved {
        inline_returns(output, pre_len, v);
    }
    strip_redundant_else(output, pre_len);
    strip_bare_blocks(output, pre_len);
    inline_wrapper_alias(output, pre_len);
    inline_trivial_bindings(output, pre_len);
}

pub(crate) fn optimize_function_body(output: &mut String) {
    optimize_region(output, 0, None);
}

fn find_matching_close(lines: &[impl AsRef<str>], start: usize) -> Option<usize> {
    let mut depth: i32 = 1;
    let mut j = start + 1;
    while j < lines.len() {
        let l = lines[j].as_ref();
        let opens = l.chars().filter(|&c| c == '{').count() as i32;
        let closes = l.chars().filter(|&c| c == '}').count() as i32;
        depth += opens - closes;
        if depth == 0 {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Peephole: rewrite `var TEMP TYPE\n...TEMP = VALUE...\nreturn TEMP\n`
/// into `...return VALUE...\n`, inlining returns into each branch.
///
/// The emitter has no indentation (gofmt handles it), so `TEMP = ` appears
/// at line start even inside switch cases and if/else branches.
fn inline_returns(output: &mut String, pre_len: usize, temp_var: &str) {
    let region = &output[pre_len..];
    let var_line_prefix = format!("var {} ", temp_var);
    let assign_prefix = format!("{} = ", temp_var);
    let return_line = format!("return {}", temp_var);

    let lines: Vec<&str> = region.lines().collect();

    let Some(var_idx) = lines.iter().position(|l| l.starts_with(&var_line_prefix)) else {
        return;
    };

    if lines.last().is_none_or(|l| *l != return_line.as_str()) {
        return;
    }

    let mut result = String::with_capacity(region.len());
    let mut dead_labels: Vec<&str> = Vec::new();

    for line in &lines[..var_idx] {
        result.push_str(line);
        result.push('\n');
    }

    let body = &lines[var_idx + 1..lines.len() - 1];
    let mut i = 0;
    while i < body.len() {
        if body[i].starts_with(&assign_prefix) {
            result.push_str("return ");
            result.push_str(&body[i][assign_prefix.len()..]);
            result.push('\n');
            if i + 1 < body.len() && (body[i + 1] == "break" || body[i + 1].starts_with("break ")) {
                if let Some(label) = body[i + 1].strip_prefix("break ") {
                    dead_labels.push(label);
                }
                i += 1;
            }
        } else {
            result.push_str(body[i]);
            result.push('\n');
        }
        i += 1;
    }

    if !dead_labels.is_empty() {
        let cleaned = result
            .lines()
            .filter(|l| {
                if let Some(label) = l.strip_suffix(':') {
                    !dead_labels.contains(&label)
                        || result.lines().any(|rl| rl == format!("break {}", label))
                } else {
                    true
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        result.clear();
        result.push_str(&cleaned);
        result.push('\n');
    }

    output.truncate(pre_len);
    output.push_str(&result);
}

/// Check whether the region has a `var X T; ... X = expression; ... return X` pattern
/// that can be safely inlined. Returns the variable name if so.
fn find_inlinable_var(region: &str) -> Option<String> {
    let lines: Vec<&str> = region.lines().collect();

    // Last line must be `return VAR` where VAR is a simple identifier.
    let last = *lines.last()?;
    let var_name = last.strip_prefix("return ")?;
    if !is_go_identifier(var_name) {
        return None;
    }

    // Must have a corresponding `var VAR TYPE` declaration.
    let var_line_prefix = format!("var {} ", var_name);
    if !lines.iter().any(|l| l.starts_with(&var_line_prefix)) {
        return None;
    }

    // Must have at least one `VAR = ` assignment (not just initialization in var decl).
    let assign_prefix = format!("{} = ", var_name);
    if !lines.iter().any(|l| l.starts_with(&assign_prefix)) {
        return None;
    }

    // VAR must only appear in `var VAR`, `VAR = `, and `return VAR` lines.
    let var_read_elsewhere = lines.iter().any(|l| {
        !l.starts_with(&var_line_prefix)
            && !l.starts_with(&assign_prefix)
            && *l != last
            && output_references_var(l, var_name)
    });
    if var_read_elsewhere {
        return None;
    }

    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with(&assign_prefix) {
            continue;
        }
        let mut j = i + 1;
        while j < lines.len() && lines[j].starts_with('}') {
            j += 1;
        }
        if j >= lines.len() {
            continue;
        }
        let next = lines[j];
        if next == "break"
            || next.starts_with("break ")
            || next.starts_with(&assign_prefix)
            || next == last
        {
            continue;
        }
        return None;
    }

    Some(var_name.to_owned())
}

/// Collapse single-use bindings: `VAR := EXPR\n{return VAR|TARGET = VAR|_ = VAR}\n`
/// becomes `{return EXPR|TARGET = EXPR|_ = EXPR}\n`.
///
/// Only collapses when VAR appears nowhere else in the region (true single-use).
/// Pattern bindings are always pure field accesses, so reordering is safe.
pub(crate) fn inline_trivial_bindings(output: &mut String, pre_len: usize) {
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

    if changed {
        output.truncate(pre_len);
        output.push_str(&result);
    }
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
    None
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

/// After `inline_returns` converts assignments to returns, `} else {`
/// wrappers become redundant when the if-branch already diverges.
/// This pass strips them: `<diverge>\n} else {\n<body>\n}\n` → `<diverge>\n}\n<body>\n`.
fn strip_redundant_else(output: &mut String, pre_len: usize) {
    loop {
        let lines: Vec<String> = output[pre_len..].lines().map(String::from).collect();
        let mut result: Vec<&str> = Vec::with_capacity(lines.len());
        let mut changed = false;
        let mut i = 0;

        while i < lines.len() {
            if lines[i] == "} else {" && result.last().is_some_and(|l| is_diverge_line(l)) {
                result.push("}");
                changed = true;

                if let Some(close) = find_matching_close(&lines, i) {
                    for line in &lines[i + 1..close] {
                        result.push(line);
                    }
                    i = close + 1;
                } else {
                    i += 1;
                }
            } else {
                result.push(&lines[i]);
                i += 1;
            }
        }

        if !changed {
            break;
        }

        output.truncate(pre_len);
        for line in &result {
            output.push_str(line);
            output.push('\n');
        }
    }
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

fn strip_bare_blocks(output: &mut String, pre_len: usize) {
    let lines: Vec<&str> = output[pre_len..].lines().collect();
    let mut result: Vec<&str> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut i = 0;

    while i < lines.len() {
        if lines[i] == "{"
            && let Some(j) = find_matching_close(&lines, i)
            && j > i + 1
            && lines[j] == "}"
            && is_diverge_line(lines[j - 1])
        {
            // Skip when an inner `var :=` would collide with an outer reference
            // ("no new variables on left side of :=" error).
            let has_conflict = lines[i + 1..j].iter().any(|l| {
                let Some(idx) = l.find(" := ") else {
                    return false;
                };
                let var = &l[..idx];
                is_go_identifier(var)
                    && lines
                        .iter()
                        .enumerate()
                        .any(|(k, ol)| (k < i || k > j) && output_references_var(ol, var))
            });

            if !has_conflict {
                for line in &lines[i + 1..j] {
                    result.push(line);
                }
                changed = true;
                i = j + 1;
                continue;
            }
        }
        result.push(lines[i]);
        i += 1;
    }

    if changed {
        let mut new = String::with_capacity(output.len());
        new.push_str(&output[..pre_len]);
        for line in &result {
            new.push_str(line);
            new.push('\n');
        }
        *output = new;
    }
}

/// Peephole: when a Go interop wrapper temp is immediately aliased
/// (`var tmp T; ... tmp = V ...; alias := tmp`), rename tmp → alias
/// throughout and remove the alias line.
fn inline_wrapper_alias(output: &mut String, pre_len: usize) {
    let lines: Vec<String> = output[pre_len..].lines().map(String::from).collect();

    for (alias_idx, line) in lines.iter().enumerate() {
        let Some((alias, temp)) = parse_binding(line) else {
            continue;
        };
        let var_prefix = format!("var {} ", temp);
        let Some(var_idx) = lines[..alias_idx]
            .iter()
            .position(|l| l.starts_with(&var_prefix))
        else {
            continue;
        };

        let assign_prefix = format!("{} = ", temp);
        let temp_used_elsewhere = lines.iter().enumerate().any(|(j, l)| {
            j != var_idx
                && j != alias_idx
                && !l.starts_with(&assign_prefix)
                && output_references_var(l, temp)
        });
        if temp_used_elsewhere {
            continue;
        }

        let mut result = String::with_capacity(output.len() - pre_len);
        for (j, l) in lines.iter().enumerate() {
            if j == var_idx {
                result.push_str(&format!("var {} {}", alias, &l[var_prefix.len()..]));
                result.push('\n');
            } else if j == alias_idx {
                // Remove the alias line.
            } else if l.starts_with(&assign_prefix) {
                result.push_str(&format!("{} = {}", alias, &l[assign_prefix.len()..]));
                result.push('\n');
            } else {
                result.push_str(l);
                result.push('\n');
            }
        }

        output.truncate(pre_len);
        output.push_str(&result);
        return;
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
