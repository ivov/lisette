use crate::checker::EnvResolve;
use syntax::ast::{Expression, Span};
use syntax::types::Type;

use crate::checker::TaskState;

impl TaskState<'_> {
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
                0,
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
        let inner = expression.unwrap_parens();

        let Expression::Literal {
            literal: syntax::ast::Literal::Integer { value, .. },
            ..
        } = inner
        else {
            return;
        };

        self.check_negative_magnitude_overflow(*value, target_ty, span);
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
    Some(match type_name? {
        "int8" => IntegerBounds {
            name: "int8",
            min: i8::MIN as i128,
            max: i8::MAX as i128,
        },
        "int16" => IntegerBounds {
            name: "int16",
            min: i16::MIN as i128,
            max: i16::MAX as i128,
        },
        "int32" => IntegerBounds {
            name: "int32",
            min: i32::MIN as i128,
            max: i32::MAX as i128,
        },
        "int64" | "int" => IntegerBounds {
            name: "int64",
            min: i64::MIN as i128,
            max: i64::MAX as i128,
        },
        "rune" => IntegerBounds {
            name: "rune",
            min: i32::MIN as i128,
            max: i32::MAX as i128,
        },
        "uint8" | "byte" => IntegerBounds {
            name: "uint8",
            min: 0,
            max: u8::MAX as i128,
        },
        "uint16" => IntegerBounds {
            name: "uint16",
            min: 0,
            max: u16::MAX as i128,
        },
        "uint32" => IntegerBounds {
            name: "uint32",
            min: 0,
            max: u32::MAX as i128,
        },
        "uint64" | "uint" | "uintptr" => IntegerBounds {
            name: "uint64",
            min: 0,
            max: u64::MAX as i128,
        },
        _ => return None,
    })
}

fn is_unsigned_type(type_name: Option<&str>) -> bool {
    matches!(
        type_name,
        Some("uint8" | "byte" | "uint16" | "uint32" | "uint64" | "uint" | "uintptr")
    )
}
