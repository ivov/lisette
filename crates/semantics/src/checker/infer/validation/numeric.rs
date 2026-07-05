use crate::checker::EnvResolve;
use syntax::ast::{Expression, Span};
use syntax::types::{SimpleKind, Type};

use crate::checker::infer::InferCtx;

impl InferCtx<'_, '_> {
    /// Validates that an integer literal fits within the target numeric type.
    /// Note: value is u64 from the parser, so negative literals are handled via unary minus.
    pub(crate) fn check_integer_literal_overflow(
        &mut self,
        value: u64,
        target_ty: &Type,
        span: Span,
    ) {
        let Some(bounds) = integer_bounds(target_ty.get_name()) else {
            return;
        };

        // For positive literals (u64 from parser), only check against max.
        // Negative literals route through check_negative_magnitude_overflow:
        // either via the unary-minus path (operators.rs) or via the
        // pre-negated text detection in literals.rs.
        if value as i128 > bounds.max {
            self.sink.push(diagnostics::infer::integer_literal_overflow(
                bounds.name,
                bounds.min,
                bounds.max,
                span,
            ));
        }
    }

    /// Validates that a float literal fits within the target float type.
    pub(crate) fn check_float_literal_overflow(
        &mut self,
        value: f64,
        target_ty: &Type,
        span: Span,
    ) {
        if target_ty.get_name() == Some("float32") && value.is_finite() {
            let f32_val = value as f32;
            if f32_val.is_infinite() {
                self.sink
                    .push(diagnostics::infer::float_literal_overflow("float32", span));
            }
        }
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
        let type_name = target_ty.get_name();

        // Allow `-0` on unsigned types; any nonzero negation is an error.
        if is_unsigned_type(type_name) {
            if magnitude != 0 {
                self.sink.push(diagnostics::infer::cannot_negate_unsigned(
                    type_name.unwrap_or("uint"),
                    span,
                ));
            }
            return;
        }

        let Some(bounds) = integer_bounds(type_name) else {
            return;
        };

        if magnitude as i128 > -bounds.min {
            self.sink.push(diagnostics::infer::integer_literal_overflow(
                bounds.name,
                bounds.min,
                bounds.max,
                span,
            ));
        }
    }

    pub(crate) fn check_cast_literal_overflow(
        &mut self,
        expression: &Expression,
        target_ty: &Type,
        span: Span,
    ) {
        let resolved = target_ty.resolve_in(&self.env);
        if !resolved.is_numeric() {
            return;
        }

        let (value, is_negative) = match expression.unwrap_parens() {
            Expression::Literal {
                literal: syntax::ast::Literal::Integer { value, .. },
                ..
            } => (*value, false),
            Expression::Unary {
                operator: syntax::ast::UnaryOperator::Negative,
                expression: inner,
                ..
            } => {
                if let Expression::Literal {
                    literal: syntax::ast::Literal::Integer { value, .. },
                    ..
                } = inner.unwrap_parens()
                {
                    (*value, true)
                } else {
                    return;
                }
            }
            _ => return,
        };

        let Some(bounds) = integer_bounds(resolved.get_name()) else {
            return;
        };

        let signed_value: i128 = if is_negative {
            -(value as i128)
        } else {
            value as i128
        };

        if signed_value < bounds.min || signed_value > bounds.max {
            self.sink.push(diagnostics::infer::integer_literal_overflow(
                bounds.name,
                bounds.min,
                bounds.max,
                span,
            ));
        }
    }
}

struct IntegerBounds {
    name: &'static str,
    min: i128,
    max: i128,
}

fn integer_bounds(type_name: Option<&str>) -> Option<IntegerBounds> {
    let kind = SimpleKind::from_name(type_name?)?;
    let (min, max) = kind.integer_range()?;
    let name = match kind {
        SimpleKind::Int | SimpleKind::Int64 => "int64",
        SimpleKind::Int8 => "int8",
        SimpleKind::Int16 => "int16",
        SimpleKind::Int32 => "int32",
        SimpleKind::Rune => "rune",
        SimpleKind::Byte | SimpleKind::Uint8 => "uint8",
        SimpleKind::Uint16 => "uint16",
        SimpleKind::Uint32 => "uint32",
        SimpleKind::Uint | SimpleKind::Uint64 | SimpleKind::Uintptr => "uint64",
        _ => return None,
    };
    Some(IntegerBounds { name, min, max })
}

fn is_unsigned_type(type_name: Option<&str>) -> bool {
    matches!(
        type_name,
        Some("uint8" | "byte" | "uint16" | "uint32" | "uint64" | "uint" | "uintptr")
    )
}
