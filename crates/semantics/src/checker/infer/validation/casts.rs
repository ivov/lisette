use crate::checker::EnvResolve;
use diagnostics::infer::InvalidCastKind;
use syntax::ast::{Expression, Span};
use syntax::types::{SimpleKind, Type};

use crate::checker::infer::InferCtx;
use crate::store::Store;
use syntax::ast::Literal;
use syntax::ast::UnaryOperator;

impl InferCtx<'_> {
    /// Validates that a cast from source_ty to target_ty is allowed.
    /// Pushes a diagnostic if the cast is invalid.
    ///
    /// Allowed conversions:
    /// - Numeric types (int, uint, float families) to any other numeric type,
    ///   including types with numeric underlying types (e.g., `struct Duration(int64)`).
    /// - rune -> string (UTF-8 encodes the codepoint)
    /// - string <-> Slice<byte> / Slice<rune>
    /// - Function type -> function type under the assignment rule: a parameter
    ///   may demand less permission than the target promises, never more.
    ///
    /// Explicitly blocked:
    /// - rune -> byte/uint8 (rune is int32 and may not fit in a byte)
    /// - byte -> string (ambiguous: byte vs codepoint reading;
    ///   force `[b] as string` for raw, or cast through rune for codepoint)
    ///
    /// Complex types (complex64, complex128) are explicitly excluded.
    pub(crate) fn check_valid_cast(
        &mut self,
        raw_source_ty: &Type,
        raw_target_ty: &Type,
        span: Span,
    ) {
        let store = self.store;
        let raw_source_resolved = raw_source_ty.resolve_in(&self.env);
        let raw_target_resolved = raw_target_ty.resolve_in(&self.env);
        let source_is_function = store
            .resolve_to_function_type(&raw_source_resolved)
            .is_some();
        let target_is_function = store
            .resolve_to_function_type(&raw_target_resolved)
            .is_some();
        if source_is_function && target_is_function {
            self.unify(&raw_target_resolved, &raw_source_resolved, &span);
            return;
        }
        if !self.cast_keeps_permission(&raw_source_resolved, &raw_target_resolved) {
            self.sink.push(diagnostics::infer::cast_grants_permission(
                &raw_source_resolved.to_string(),
                &raw_target_resolved.to_string(),
                span,
            ));
            return;
        }
        let source_ty = raw_source_resolved.demoted();
        let target_ty = raw_target_resolved.demoted();

        if source_ty.contains_error() || target_ty.contains_error() {
            return;
        }

        if source_ty.is_complex() || target_ty.is_complex() {
            self.sink.push(diagnostics::infer::invalid_cast(
                raw_source_ty,
                raw_target_ty,
                InvalidCastKind::Complex,
                span,
            ));
            return;
        }

        if store.has_underlying_rune(&source_ty) && store.has_underlying_byte(&target_ty) {
            self.sink.push(diagnostics::infer::invalid_cast(
                raw_source_ty,
                raw_target_ty,
                InvalidCastKind::RuneToByte,
                span,
            ));
            return;
        }

        if store.has_underlying_numeric_type(&source_ty)
            && store.has_underlying_numeric_type(&target_ty)
        {
            return;
        }

        if uintptr_scalar_conversion(store, &source_ty, &target_ty) {
            return;
        }

        if store.peel_alias(&source_ty) == store.peel_alias(&target_ty) {
            return;
        }

        if store.has_underlying_rune(&source_ty) && target_ty.is_string() {
            return;
        }

        if (source_ty.is_string() && store.has_byte_or_rune_slice_underlying(&target_ty))
            || (target_ty.is_string() && store.has_byte_or_rune_slice_underlying(&source_ty))
        {
            return;
        }

        if source_ty.is_byte_slice() && target_ty.is_byte_slice() {
            return;
        }

        if store.peel_alias_deep(&store.peel_underlying(&source_ty))
            == store.peel_alias_deep(&store.peel_underlying(&target_ty))
        {
            return;
        }

        // Concrete type -> interface: allowed if source satisfies the interface.
        // Used for explicit coercion before wrapping in generic containers,
        // e.g. `Some(my_dog as Animal)` to get `Option<Animal>`.
        let peeled_target = store.peel_alias(&target_ty);
        if let Type::Nominal { id, .. } = &peeled_target
            && store.get_interface(id).is_some()
        {
            let _ = self.satisfies_interface(&source_ty, &peeled_target, &span);
            return;
        }

        self.sink.push(diagnostics::infer::invalid_cast(
            raw_source_ty,
            raw_target_ty,
            if store.has_underlying_byte(&source_ty) && target_ty.is_string() {
                InvalidCastKind::ByteToString
            } else {
                InvalidCastKind::Other
            },
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
                literal: Literal::Integer { .. },
                ..
            } if target_resolved.is_numeric() && !target_resolved.is_rune() => {
                self.sink
                    .push(diagnostics::infer::redundant_cast(&target_resolved, span));
            }
            Expression::Literal {
                literal: Literal::Float { .. },
                ..
            } if target_resolved.is_float() => {
                self.sink
                    .push(diagnostics::infer::redundant_cast(&target_resolved, span));
            }
            _ => {}
        }
    }
}

fn uintptr_scalar_conversion(store: &Store, source: &Type, target: &Type) -> bool {
    let castable = |ty: &Type| {
        store.has_underlying_numeric_type(ty)
            || store.underlying_simple_kind(ty) == Some(SimpleKind::Uintptr)
    };
    let either_uintptr = store.underlying_simple_kind(source) == Some(SimpleKind::Uintptr)
        || store.underlying_simple_kind(target) == Some(SimpleKind::Uintptr);
    either_uintptr && castable(source) && castable(target)
}

fn unwrap_parens_and_negation(expression: &Expression) -> &Expression {
    match expression {
        Expression::Paren { expression, .. } => unwrap_parens_and_negation(expression),
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => unwrap_parens_and_negation(expression),
        _ => expression,
    }
}

impl InferCtx<'_> {
    /// Whether the target's write permission is within the source's, with
    /// newtypes normalized so both sides compare at the same layer.
    fn cast_keeps_permission(&self, source: &Type, target: &Type) -> bool {
        let source = self.store.peel_underlying(&source.resolve_in(&self.env));
        let target = self.store.peel_underlying(&target.resolve_in(&self.env));
        match (&source, &target) {
            (
                Type::Compound {
                    writable: source_writable,
                    args: source_args,
                    ..
                },
                Type::Compound {
                    writable: target_writable,
                    args: target_args,
                    ..
                },
            ) => {
                (*source_writable || !*target_writable)
                    && source_args
                        .iter()
                        .zip(target_args)
                        .all(|(s, t)| self.cast_keeps_permission(s, t))
            }
            (
                Type::Nominal {
                    writable: source_writable,
                    params: source_params,
                    ..
                },
                Type::Nominal {
                    writable: target_writable,
                    params: target_params,
                    ..
                },
            ) => {
                (*source_writable || !*target_writable)
                    && source_params
                        .iter()
                        .zip(target_params)
                        .all(|(s, t)| self.cast_keeps_permission(s, t))
            }
            (Type::Tuple(source_elements), Type::Tuple(target_elements)) => source_elements
                .iter()
                .zip(target_elements)
                .all(|(s, t)| self.cast_keeps_permission(s, t)),
            (
                Type::Array {
                    element: source_element,
                    ..
                },
                Type::Array {
                    element: target_element,
                    ..
                },
            ) => self.cast_keeps_permission(source_element, target_element),
            _ => true,
        }
    }
}
