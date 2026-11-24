use syntax::ast::{Expression, Literal, Span};
use syntax::constant::{self, ConstantError, Value};
use syntax::types::{SimpleKind, Type};

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(crate) fn check_float_literal_overflow(
        &mut self,
        value: f64,
        target_ty: &Type,
        span: Span,
    ) {
        let Some(kind @ (SimpleKind::Float32 | SimpleKind::Complex64)) =
            self.store.underlying_simple_kind(target_ty)
        else {
            return;
        };
        let Some(value) = Value::from_f64(value) else {
            return;
        };
        if constant::represent(value, SimpleKind::Float32, span).is_err() {
            self.sink.push(diagnostics::infer::float_literal_overflow(
                kind.leaf_name(),
                span,
            ));
        }
    }

    pub(crate) fn check_pattern_literal_range(&mut self, literal: &Literal, ty: &Type, span: Span) {
        if matches!(literal, Literal::Integer { text: Some(text), .. } if text.starts_with('-')) {
            return;
        }
        let Some(kind) = self.store.underlying_simple_kind(ty) else {
            return;
        };
        let Some(value) = constant::literal_value(literal) else {
            return;
        };
        if !matches!(
            constant::represent(value, kind, span),
            Err(ConstantError::Overflow { .. })
        ) {
            return;
        }
        let diagnostic = match kind.integer_range() {
            Some((min, max)) => {
                diagnostics::infer::integer_literal_overflow(kind.sized_name(), min, max, span)
            }
            None => diagnostics::infer::float_literal_overflow(kind.leaf_name(), span),
        };
        self.sink.push(diagnostic);
    }

    pub(crate) fn check_negative_literal_overflow(
        &mut self,
        expression: &Expression,
        target_ty: &Type,
        span: Span,
    ) {
        let Some(value) = expression.as_integer() else {
            return;
        };

        self.check_negative_magnitude_overflow(value, target_ty, span);
    }

    pub(crate) fn check_negative_magnitude_overflow(
        &mut self,
        magnitude: u64,
        target_ty: &Type,
        span: Span,
    ) {
        let Some(kind) = self.store.underlying_simple_kind(target_ty) else {
            return;
        };

        // Allow `-0` on unsigned types; any nonzero negation is an error.
        if kind.is_unsigned() {
            if magnitude != 0 {
                let type_name = target_ty
                    .display_name()
                    .map_or_else(|| kind.leaf_name().to_string(), |name| name.to_string());
                self.sink
                    .push(diagnostics::infer::cannot_negate_unsigned(&type_name, span));
            }
            return;
        }

        let Some((min, max)) = kind.integer_range() else {
            return;
        };
        let value = Value::from_i128(-i128::from(magnitude));
        if constant::represent(value, kind, span).is_err() {
            self.sink.push(diagnostics::infer::integer_literal_overflow(
                kind.sized_name(),
                min,
                max,
                span,
            ));
        }
    }
}
