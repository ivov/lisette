use super::output_references_var;
use syntax::ast::Expression;

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
