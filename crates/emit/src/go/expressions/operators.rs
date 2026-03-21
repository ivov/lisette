use crate::Emitter;
use crate::go::utils::try_flip_comparison;
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::types::Type;

struct NumericBinaryEmitInfo {
    cast_left_to: Option<Type>,
    cast_right_to: Option<Type>,
    cast_result_to: Option<Type>,
}

impl Emitter<'_> {
    pub(crate) fn emit_binary_expression(
        &mut self,
        output: &mut String,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
    ) -> String {
        if matches!(operator, BinaryOperator::Pipeline) {
            unreachable!("Pipeline operator should have been desugared by now")
        }

        let left_ty = left_expression.get_type().resolve();
        let right_ty = right_expression.get_type().resolve();

        if let Some(emit_info) = self.is_casting_needed(
            operator,
            &left_ty,
            &right_ty,
            left_expression,
            right_expression,
        ) {
            return self.emit_numeric_binary_with_casts(
                output,
                operator,
                left_expression,
                right_expression,
                emit_info,
            );
        }

        if matches!(operator, BinaryOperator::Multiplication) {
            if let Expression::Literal {
                literal: Literal::Imaginary(imag_coef),
                ..
            } = right_expression
                && left_ty.is_float()
                && !left_ty.is_complex()
            {
                let float_expression = self.emit_operand(output, left_expression);
                return format!("complex(0, {}*{})", float_expression, imag_coef);
            }
            if let Expression::Literal {
                literal: Literal::Imaginary(imag_coef),
                ..
            } = left_expression
                && right_ty.is_float()
                && !right_ty.is_complex()
            {
                let float_expression = self.emit_operand(output, right_expression);
                return format!("complex(0, {}*{})", float_expression, imag_coef);
            }
        }

        let stages = vec![
            self.stage_composite(left_expression),
            self.stage_composite(right_expression),
        ];
        let values = self.sequence(output, stages, "_left");
        let left_string = values[0].clone();
        let right_string = values[1].clone();

        format!("{} {} {}", left_string, operator, right_string)
    }

    pub(crate) fn emit_unary_expression(
        &mut self,
        output: &mut String,
        operator: &UnaryOperator,
        expression: &Expression,
    ) -> String {
        // Special case: -9223372036854775808 cannot be written as a positive literal
        // because 9223372036854775808 overflows i64. Go handles this correctly
        // when written directly as -9223372036854775808.
        if matches!(operator, UnaryOperator::Negative)
            && let Expression::Literal {
                literal:
                    Literal::Integer {
                        value: 9223372036854775808,
                        ..
                    },
                ..
            } = expression
        {
            return "-9223372036854775808".to_string();
        }

        let expression = self.emit_operand(output, expression);

        // Negate comparisons by flipping the operator instead of prepending `!`.
        // Without this, `!len(s) == 0` would be `(!len(s)) == 0` in Go
        // because `!` binds tighter than `==`.
        if matches!(operator, UnaryOperator::Not)
            && let Some(flipped) = try_flip_comparison(&expression)
        {
            return flipped;
        }

        let op_str = match operator {
            UnaryOperator::Negative => "-",
            UnaryOperator::Not => "!",
            UnaryOperator::Deref => "*",
        };
        format!("{}{}", op_str, expression)
    }

    /// Determines if casting is needed for a binary operation.
    ///
    /// Go requires explicit casts when mixing aliased numeric types with
    /// their underlying types. This function analyzes the operand types
    /// and determines what casts are needed.
    fn is_casting_needed(
        &self,
        operator: &BinaryOperator,
        left_ty: &Type,
        right_ty: &Type,
        left_expression: &Expression,
        right_expression: &Expression,
    ) -> Option<NumericBinaryEmitInfo> {
        use BinaryOperator::*;

        if !matches!(
            operator,
            Addition
                | Subtraction
                | Multiplication
                | Division
                | Remainder
                | LessThan
                | LessThanOrEqual
                | GreaterThan
                | GreaterThanOrEqual
                | Equal
                | NotEqual
        ) {
            return None;
        }

        let left_underlying_ty = left_ty.underlying_numeric_type();
        let right_underlying_ty = right_ty.underlying_numeric_type();

        let (left_underlying_ty, right_underlying_ty) =
            match (&left_underlying_ty, &right_underlying_ty) {
                (Some(l), Some(r)) => (l, r),
                _ => return None,
            };

        let left_family = left_underlying_ty.numeric_family()?;
        let right_family = right_underlying_ty.numeric_family()?;

        if left_family != right_family {
            return None;
        }

        let left_is_aliased = left_ty.is_aliased_numeric_type();
        let right_is_aliased = right_ty.is_aliased_numeric_type();

        if left_ty == right_ty {
            if left_is_aliased && matches!(operator, Division) {
                return Some(NumericBinaryEmitInfo {
                    cast_left_to: None,
                    cast_right_to: None,
                    cast_result_to: Some(left_underlying_ty.clone()),
                });
            }
            return None;
        }

        let left_is_literal = is_literal_expression(left_expression);
        let right_is_literal = is_literal_expression(right_expression);

        match (left_is_aliased, right_is_aliased) {
            (true, false) => Some(NumericBinaryEmitInfo {
                cast_left_to: None,
                cast_right_to: if right_is_literal {
                    None
                } else {
                    Some(left_ty.clone())
                },
                cast_result_to: None,
            }),

            (false, true) => Some(NumericBinaryEmitInfo {
                cast_left_to: if left_is_literal {
                    None
                } else {
                    Some(right_ty.clone())
                },
                cast_right_to: None,
                cast_result_to: None,
            }),

            _ => None,
        }
    }

    fn emit_numeric_binary_with_casts(
        &mut self,
        output: &mut String,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        info: NumericBinaryEmitInfo,
    ) -> String {
        let stages = vec![
            self.stage_operand(left_expression),
            self.stage_operand(right_expression),
        ];
        let values = self.sequence(output, stages, "_left");
        let left_string = values[0].clone();
        let right_string = values[1].clone();

        let left_string = match &info.cast_left_to {
            Some(ty) => format!("{}({})", self.go_type_as_string(ty), left_string),
            None => left_string,
        };

        let right_string = match &info.cast_right_to {
            Some(ty) => format!("{}({})", self.go_type_as_string(ty), right_string),
            None => right_string,
        };

        let result = format!("{} {} {}", left_string, operator, right_string);

        match &info.cast_result_to {
            Some(ty) => format!("{}({})", self.go_type_as_string(ty), result),
            None => result,
        }
    }
}

fn is_literal_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Literal { .. } => true,
        Expression::Paren { expression, .. } => is_literal_expression(expression),
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => is_literal_expression(expression),
        _ => false,
    }
}
