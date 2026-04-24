use crate::checker::EnvResolve;
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::types::Type;

use crate::checker::TaskState;

impl TaskState<'_> {
    /// Validates that a cast from source_ty to target_ty is allowed.
    /// Pushes a diagnostic if the cast is invalid.
    ///
    /// Allowed conversions:
    /// - Numeric types (int, uint, float families) to any other numeric type,
    ///   including types with numeric underlying types (e.g., `enum Duration: int64`)
    /// - Integer <-> rune
    /// - string <-> Slice<byte> / Slice<rune>, including types with byte/rune slice
    ///   underlying types (e.g., `type Bytes = Slice<byte>`)
    ///
    /// Complex types (complex64, complex128) are explicitly excluded.
    pub(crate) fn check_valid_cast(
        &mut self,
        store: &Store,
        raw_source_ty: &Type,
        raw_target_ty: &Type,
        span: Span,
    ) {
        let source_ty = raw_source_ty.resolve_in(&self.env);
        let target_ty = raw_target_ty.resolve_in(&self.env);

        if source_ty.is_complex() || target_ty.is_complex() {
            self.sink.push(diagnostics::infer::invalid_cast(
                raw_source_ty,
                raw_target_ty,
                span,
            ));
            return;
        }

        if source_ty.has_underlying_numeric_type() && target_ty.has_underlying_numeric_type() {
            return;
        }

        if (source_ty.is_string() && target_ty.has_byte_or_rune_slice_underlying())
            || (target_ty.is_string() && source_ty.has_byte_or_rune_slice_underlying())
        {
            return;
        }

        if source_ty.is_byte_slice() && target_ty.is_byte_slice() {
            return;
        }

        // Concrete type -> interface: allowed if source satisfies the interface.
        // Used for explicit coercion before wrapping in generic containers,
        // e.g. `Some(my_dog as Animal)` to get `Option<Animal>`.
        let peeled_target = store.peel_alias(&target_ty);
        if let Type::Nominal { id, params, .. } = &peeled_target
            && let Some(interface) = store.get_interface(id).cloned()
            && self
                .satisfies_interface(store, &source_ty, &interface, params, &span)
                .is_ok()
        {
            return;
        }

        // Type alias <-> underlying type (e.g., fn as HandlerFunc, HandlerFunc as fn)
        if let Some(underlying) = target_ty.get_underlying()
            && source_ty == *underlying
        {
            return;
        }
        if let Some(underlying) = source_ty.get_underlying()
            && target_ty == *underlying
        {
            return;
        }

        self.sink.push(diagnostics::infer::invalid_cast(
            raw_source_ty,
            raw_target_ty,
            span,
        ));
    }

    pub(crate) fn check_redundant_cast(
        &mut self,
        raw_source_ty: &Type,
        raw_target_ty: &Type,
        span: Span,
    ) -> bool {
        let source_ty = raw_source_ty.resolve_in(&self.env);

        if source_ty == raw_target_ty.resolve_in(&self.env) {
            self.sink
                .push(diagnostics::infer::redundant_cast(&source_ty, span));
            return true;
        }
        false
    }

    /// Checks for redundant casts on literals that would adapt to the target type anyway.
    /// For example, `let x: int64 = 100 as int64` is redundant because the literal would adapt.
    /// But `let x = 100 as int64` is NOT redundant - without the cast, x would be int.
    /// Note: `65 as rune` is NOT redundant - it's a semantic conversion from number to character.
    pub(crate) fn check_redundant_literal_cast(
        &mut self,
        expression: &Expression,
        target_ty: &Type,
        expected_ty: &Type,
        span: Span,
    ) {
        let target_resolved = target_ty.resolve_in(&self.env);
        let expected_resolved = expected_ty.resolve_in(&self.env);

        if expected_resolved.is_variable() {
            return;
        }

        if expected_resolved != target_resolved {
            return;
        }

        let inner_expression = unwrap_parens_and_negation(expression);

        match inner_expression {
            Expression::Literal {
                literal: syntax::ast::Literal::Integer { .. },
                ..
            } if target_resolved.is_numeric() && !target_resolved.is_rune() => {
                self.sink
                    .push(diagnostics::infer::redundant_cast(&target_resolved, span));
            }
            Expression::Literal {
                literal: syntax::ast::Literal::Float { .. },
                ..
            } if target_resolved.is_float() => {
                self.sink
                    .push(diagnostics::infer::redundant_cast(&target_resolved, span));
            }
            _ => {}
        }
    }
}

fn unwrap_parens_and_negation(expression: &Expression) -> &Expression {
    match expression {
        Expression::Paren { expression, .. } => unwrap_parens_and_negation(expression),
        Expression::Unary {
            operator: syntax::ast::UnaryOperator::Negative,
            expression,
            ..
        } => unwrap_parens_and_negation(expression),
        _ => expression,
    }
}
