use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, Span};
use syntax::types::{CompoundKind, Type};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
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
                if let Some(s) = spread.as_deref() {
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
                if let Some(s) = spread.as_expression() {
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

    /// In evaluation order, flag a read of `v` when an *earlier* sibling takes
    /// `&v`, so the read may see the mutation. A `&v` after the read is fine.
    fn check_sibling_ref_aliasing_refs(&mut self, siblings: &[&Expression]) {
        let mut ref_vars: HashSet<String> = HashSet::default();
        for sib in siblings {
            self.collect_ref_targets(sib, &mut ref_vars, false);
        }
        if ref_vars.is_empty() {
            return;
        }

        for (i, sib) in siblings.iter().enumerate() {
            let mut reads: HashSet<String> = HashSet::default();
            collect_read_vars(sib, &mut reads, false);
            for var in reads.intersection(&ref_vars) {
                let mut ref_in_same = HashSet::default();
                self.collect_ref_targets(sib, &mut ref_in_same, false);
                if ref_in_same.contains(var.as_str()) {
                    continue; // `&v` and `v` in the same operand is fine
                }
                let Some(read_span) = find_read_span(sib, var, false) else {
                    continue;
                };
                for (j, other) in siblings.iter().enumerate() {
                    if j >= i {
                        continue; // only a `&v` before the read can affect it
                    }
                    if let Some(ref_span) = find_ref_span(other, var) {
                        self.sink
                            .push(diagnostics::infer::reference_aliases_sibling(
                                ref_span, read_span, var,
                            ));
                        return; // One error per compound expression is enough
                    }
                }
            }
        }
    }

    /// Collect `v` for each `&v` handed to a parameter that permits writing.
    fn collect_ref_targets(
        &self,
        expression: &Expression,
        out: &mut HashSet<String>,
        granted: bool,
    ) {
        match expression.unwrap_parens() {
            Expression::Reference { expression, .. } => {
                if granted && let Expression::Identifier { value, .. } = expression.unwrap_parens()
                {
                    out.insert(value.to_string());
                }
                self.collect_ref_targets(expression, out, granted);
            }
            Expression::Call {
                expression: callee,
                args,
                spread,
                ..
            } => {
                self.collect_ref_targets(callee, out, false);
                let resolved = callee.get_type().resolve_in(&self.env);
                let function = self
                    .store
                    .resolve_to_function_type(&resolved)
                    .unwrap_or(resolved);
                for (index, arg) in args.iter().enumerate() {
                    let grants = self.argument_position_grants(&function, index);
                    self.collect_ref_targets(arg, out, grants);
                }
                if let Some(spread) = spread.as_deref() {
                    let grants = self.argument_position_grants(&function, args.len());
                    self.collect_ref_targets(spread, out, grants);
                }
            }
            other => {
                for child in other.children() {
                    self.collect_ref_targets(child, out, granted);
                }
            }
        }
    }

    fn argument_position_grants(&self, function: &Type, index: usize) -> bool {
        let Some(parameters) = function.get_function_params() else {
            return true;
        };
        let parameter = if index < parameters.len() {
            &parameters[index]
        } else {
            match parameters.last() {
                Some(last) if matches!(last.ty.as_compound(), Some((CompoundKind::VarArgs, _))) => {
                    last
                }
                _ => return true,
            }
        };
        self.store
            .parameter_grants_write(&parameter.ty.resolve_in(&self.env))
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

fn find_read_span(expression: &Expression, var_name: &str, inside_ref: bool) -> Option<Span> {
    match expression.unwrap_parens() {
        Expression::Identifier { value, span, .. } => {
            (!inside_ref && value.as_str() == var_name).then_some(*span)
        }
        Expression::Reference { expression, .. } => find_read_span(expression, var_name, true),
        other => other
            .children()
            .into_iter()
            .find_map(|child| find_read_span(child, var_name, false)),
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
