use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, Span};

use crate::checker::TaskState;

impl TaskState<'_> {
    /// Reject `Err(x)?` and `None?` when used as sub-expressions of a larger
    /// expression (call arg, binary operand, etc.).  These always early-return
    /// and never produce a value, so the surrounding expression is dead code.
    pub(crate) fn check_failure_propagation_in_subexpression(
        &mut self,
        inner: &Expression,
        span: Span,
    ) {
        let is_failure = match inner {
            Expression::Identifier { .. } => {
                // `None?`
                inner.as_option_constructor() == Some(Err(()))
            }
            Expression::Call {
                expression: callee, ..
            } => {
                // `Err(x)?`
                callee.as_result_constructor() == Some(Err(()))
                    || callee.as_option_constructor() == Some(Err(()))
            }
            _ => false,
        };

        if is_failure {
            self.sink
                .push(diagnostics::infer::failure_propagation_in_expression(span));
        }
    }

    /// Check all expressions in a file for `&v` aliasing a sibling read of `v`.
    pub(crate) fn check_reference_sibling_aliasing(&mut self, items: &[Expression]) {
        for item in items {
            self.walk_check_ref_aliasing(item);
        }
    }

    fn walk_check_ref_aliasing(&mut self, expression: &Expression) {
        // At compound expression nodes, check siblings for conflicts.
        match expression {
            Expression::Call {
                args,
                expression,
                spread,
                ..
            } => {
                if let Some(s) = spread.as_ref().as_ref() {
                    let mut siblings: Vec<&Expression> = args.iter().collect();
                    siblings.push(s);
                    self.check_sibling_ref_aliasing_refs(&siblings);
                    self.walk_check_ref_aliasing(s);
                } else {
                    self.check_sibling_ref_aliasing_slice(args);
                }
                self.walk_check_ref_aliasing(expression);
                for arg in args {
                    self.walk_check_ref_aliasing(arg);
                }
            }
            Expression::Binary { left, right, .. } => {
                self.check_sibling_ref_aliasing_refs(&[left, right]);
                self.walk_check_ref_aliasing(left);
                self.walk_check_ref_aliasing(right);
            }
            Expression::Tuple { elements, .. } => {
                self.check_sibling_ref_aliasing_slice(elements);
                for e in elements {
                    self.walk_check_ref_aliasing(e);
                }
            }
            Expression::StructCall {
                field_assignments,
                spread,
                ..
            } => {
                let mut values: Vec<&Expression> =
                    field_assignments.iter().map(|fa| &*fa.value).collect();
                if let Some(s) = spread.as_ref() {
                    values.push(s);
                }
                self.check_sibling_ref_aliasing_refs(&values);
                for v in &values {
                    self.walk_check_ref_aliasing(v);
                }
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                self.check_sibling_ref_aliasing_refs(&[expression.as_ref(), index.as_ref()]);
                self.walk_check_ref_aliasing(expression);
                self.walk_check_ref_aliasing(index);
            }
            Expression::Assignment { target, value, .. } => {
                self.check_sibling_ref_aliasing_refs(&[target.as_ref(), value.as_ref()]);
                self.walk_check_ref_aliasing(target);
                self.walk_check_ref_aliasing(value);
            }
            // For all other expressions, just recurse into children.
            _ => {
                for child in expression.children() {
                    self.walk_check_ref_aliasing(child);
                }
            }
        }
    }

    /// Check sibling aliasing from a slice of owned Expressions.
    fn check_sibling_ref_aliasing_slice(&mut self, siblings: &[Expression]) {
        let refs: Vec<&Expression> = siblings.iter().collect();
        self.check_sibling_ref_aliasing_refs(&refs);
    }

    /// Given a list of sibling expressions, check that no `&v` in one sibling
    /// conflicts with a bare read of `v` in another sibling.
    fn check_sibling_ref_aliasing_refs(&mut self, siblings: &[&Expression]) {
        let mut ref_vars: HashSet<String> = HashSet::default();
        for sib in siblings {
            collect_ref_targets(sib, &mut ref_vars);
        }
        if ref_vars.is_empty() {
            return;
        }

        for (i, sib) in siblings.iter().enumerate() {
            let mut reads: HashSet<String> = HashSet::default();
            collect_read_vars(sib, &mut reads, false);
            for var in reads.intersection(&ref_vars) {
                let mut ref_in_same = HashSet::default();
                collect_ref_targets(sib, &mut ref_in_same);
                if ref_in_same.contains(var.as_str()) {
                    continue; // `&v` and `v` in the same operand is fine
                }
                for (j, other) in siblings.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    if let Some(span) = find_ref_span(other, var) {
                        self.sink
                            .push(diagnostics::infer::reference_aliases_sibling(span, var));
                        return; // One error per compound expression is enough
                    }
                }
            }
        }
    }
}

/// Collect all variable names that appear under `&` anywhere in the expression tree.
fn collect_ref_targets(expression: &Expression, out: &mut HashSet<String>) {
    match expression.unwrap_parens() {
        Expression::Reference { expression, .. } => {
            if let Expression::Identifier { value, .. } = expression.unwrap_parens() {
                out.insert(value.to_string());
            }
            collect_ref_targets(expression, out);
        }
        other => {
            for child in other.children() {
                collect_ref_targets(child, out);
            }
        }
    }
}

/// Collect all variable names that are read (not under `&`) anywhere in the expression tree.
fn collect_read_vars(expression: &Expression, out: &mut HashSet<String>, inside_ref: bool) {
    match expression.unwrap_parens() {
        Expression::Identifier { value, .. } => {
            if !inside_ref {
                out.insert(value.to_string());
            }
        }
        Expression::Reference { expression, .. } => {
            collect_read_vars(expression, out, true);
        }
        other => {
            for child in other.children() {
                collect_read_vars(child, out, false);
            }
        }
    }
}

/// Find the span of `&var_name` in the expression tree.
fn find_ref_span(expression: &Expression, var_name: &str) -> Option<Span> {
    match expression.unwrap_parens() {
        Expression::Reference {
            expression, span, ..
        } => {
            if let Expression::Identifier { value, .. } = expression.unwrap_parens()
                && value.as_str() == var_name
            {
                return Some(*span);
            }
            find_ref_span(expression, var_name)
        }
        other => other
            .children()
            .into_iter()
            .find_map(|child| find_ref_span(child, var_name)),
    }
}
