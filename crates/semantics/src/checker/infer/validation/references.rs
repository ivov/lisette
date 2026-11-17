use syntax::EcoString;
use syntax::ast::{Expression, Span};
use syntax::program::prelude_constructor;
use syntax::types::{CompoundKind, Type};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::checker::infer::expressions::aliasing::{PlaceKey, place_key, render_place};

impl InferCtx<'_> {
    /// Reject `Err(x)?` and `None?` when used as sub-expressions of a larger
    /// expression (call arg, binary operand, etc.).  These always early-return
    /// and never produce a value, so the surrounding expression is dead code.
    pub(crate) fn check_failure_propagation_in_subexpression(
        &mut self,
        inner: &Expression,
        span: Span,
    ) {
        if always_fails(inner) {
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
            Expression::Binary { .. }
            | Expression::Tuple { .. }
            | Expression::StructCall { .. }
            | Expression::IndexedAccess { .. }
            | Expression::Assignment { .. } => {
                let children = expression.children();
                self.check_sibling_ref_aliasing_refs(&children);
                for child in children {
                    self.walk_check_ref_aliasing(child);
                }
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

    fn check_sibling_ref_aliasing_refs(&mut self, siblings: &[&Expression]) {
        let accesses: Vec<_> = siblings
            .iter()
            .map(|sibling| {
                let mut accesses = Accesses::default();
                self.collect_accesses(sibling, false, false, &mut accesses);
                accesses
            })
            .collect();
        let mut previous: Vec<&Access> = Vec::new();
        for access in &accesses {
            for read in &access.reads {
                if !access
                    .references
                    .iter()
                    .any(|reference| reference.granted && reference.place.overlaps(&read.place))
                    && let Some(reference) = previous
                        .iter()
                        .find(|reference| reference.place.overlaps(&read.place))
                {
                    self.sink
                        .push(diagnostics::infer::reference_aliases_sibling(
                            reference.span,
                            read.span,
                            &reference.name,
                            &read.name,
                        ));
                    return;
                }
            }

            previous.extend(
                access
                    .references
                    .iter()
                    .filter(|reference| reference.granted),
            );
        }
    }

    fn collect_accesses(
        &self,
        expression: &Expression,
        granted: bool,
        inside_ref: bool,
        accesses: &mut Accesses,
    ) {
        let expression = expression.unwrap_parens();
        if let Some(place) = place_key(expression) {
            if !inside_ref {
                accesses.reads.push(Access {
                    place,
                    name: render_place(expression).into(),
                    span: expression.get_span(),
                    granted: false,
                });
            }

            self.collect_place_indices(expression, accesses);
            return;
        }
        match expression {
            Expression::Reference {
                expression, span, ..
            } => {
                if let Some(place) = place_key(expression) {
                    accesses.references.push(Access {
                        place,
                        name: render_place(expression).into(),
                        span: *span,
                        granted,
                    });
                }
                self.collect_accesses(expression, granted, true, accesses);
            }
            Expression::Call {
                expression: callee,
                args: arguments,
                spread,
                ..
            } => {
                self.collect_accesses(callee, false, false, accesses);
                let resolved = callee.get_type().resolve_in(&self.env);
                let function = self
                    .store
                    .resolve_to_function_type(&resolved)
                    .unwrap_or(resolved);
                for (index, argument) in arguments.iter().enumerate() {
                    self.collect_accesses(
                        argument,
                        self.argument_position_grants(&function, index),
                        false,
                        accesses,
                    );
                }
                if let Some(spread) = spread {
                    self.collect_accesses(
                        spread,
                        self.argument_position_grants(&function, arguments.len()),
                        false,
                        accesses,
                    );
                }
            }
            other => {
                for child in other.evaluation_children() {
                    self.collect_accesses(child, granted, false, accesses);
                }
            }
        }
    }

    fn collect_place_indices(&self, expression: &Expression, accesses: &mut Accesses) {
        match expression.unwrap_parens() {
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                self.collect_place_indices(expression, accesses);
                self.collect_accesses(index, false, false, accesses);
            }
            Expression::DotAccess { expression, .. } | Expression::Unary { expression, .. } => {
                self.collect_place_indices(expression, accesses);
            }
            _ => {}
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

struct Access {
    granted: bool,
    place: PlaceKey,
    name: EcoString,
    span: Span,
}

#[derive(Default)]
struct Accesses {
    references: Vec<Access>,
    reads: Vec<Access>,
}

/// Whether `?` on this inferred expression always propagates: `None` or `Err(..)`.
pub(crate) fn always_fails(expression: &Expression) -> bool {
    let constructor = match expression {
        Expression::Call { expression, .. } => expression.as_ref(),
        expression => expression,
    };
    matches!(
        prelude_constructor(constructor),
        Some(("prelude.Option", "None") | ("prelude.Result", "Err"))
    )
}
