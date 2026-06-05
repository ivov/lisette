use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{ValuePlan, setup_from_string, value_plan_from_statements};
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::types::Type;

struct NumericBinaryEmitInfo {
    cast_left_to: Option<Type>,
    cast_right_to: Option<Type>,
}

struct BinaryOperand<'a> {
    expression: &'a Expression,
    ty: Type,
}

impl Planner<'_> {
    /// Plan a binary expression. Numeric-cast, imaginary-multiply, and
    /// short-circuit `&&`/`||` bridge through their string emitters.
    pub(crate) fn plan_binary(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        if matches!(operator, BinaryOperator::Pipeline) {
            unreachable!("Pipeline operator should have been desugared by now")
        }

        let left = BinaryOperand {
            expression: left_expression,
            ty: left_expression.get_type(),
        };
        let right = BinaryOperand {
            expression: right_expression,
            ty: right_expression.get_type(),
        };

        if let Some(emit_info) = is_casting_needed(operator, &left, &right) {
            let (setup, value) = self.plan_numeric_binary_with_casts(
                operator,
                left_expression,
                right_expression,
                emit_info,
                ctx,
                fx,
            );
            return value_plan_from_statements(setup, value);
        }

        let left_ty = &left.ty;
        let right_ty = &right.ty;

        if matches!(operator, BinaryOperator::Multiplication) {
            if let Expression::Literal {
                literal: Literal::Imaginary(imag_coef),
                ..
            } = right_expression
                && left_ty.is_float()
                && !left_ty.is_complex()
            {
                let staged = self.stage_operand(left_expression, ctx, fx);
                return value_plan_from_statements(
                    staged.setup,
                    format!("complex(0, {}*{})", staged.value, imag_coef),
                );
            }
            if let Expression::Literal {
                literal: Literal::Imaginary(imag_coef),
                ..
            } = left_expression
                && right_ty.is_float()
                && !right_ty.is_complex()
            {
                let staged = self.stage_operand(right_expression, ctx, fx);
                return value_plan_from_statements(
                    staged.setup,
                    format!("complex(0, {}*{})", staged.value, imag_coef),
                );
            }
        }

        if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
            let (setup, value) = self.plan_short_circuit_binary(
                operator,
                left_expression,
                right_expression,
                ctx,
                fx,
            );
            return value_plan_from_statements(setup, value);
        }

        let stages = vec![
            self.stage_composite(left_expression, ctx, fx),
            self.stage_composite(right_expression, ctx, fx),
        ];
        let (setup, values) = self.sequence_structured(stages, "_left");
        value_plan_from_statements(setup, format!("{} {} {}", values[0], operator, values[1]))
    }

    fn plan_short_circuit_binary(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let left_staged = self.stage_composite(left_expression, ctx, fx);

        // Wrap RHS setup in an IIFE so it runs only when control reaches the
        // RHS. Hoisting it before the operator would defeat short-circuit.
        let right_staged = self.stage_composite(right_expression, ctx, fx);
        let right_string = if right_staged.setup.is_empty() {
            right_staged.value
        } else {
            format!(
                "func() bool {{\n{}return {}\n}}()",
                Renderer.render_setup(&right_staged.setup),
                right_staged.value
            )
        };

        (
            left_staged.setup,
            format!("{} {} {}", left_staged.value, operator, right_string),
        )
    }

    /// Plan a prefix unary; `!` bridges through `emit_unary_not`.
    pub(crate) fn plan_unary(
        &mut self,
        operator: &UnaryOperator,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        // Special case: -9223372036854775808 cannot be written as a positive
        // literal because 9223372036854775808 overflows i64. Go handles this
        // correctly when written directly as -9223372036854775808.
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
            return ValuePlan::Operand("-9223372036854775808".to_string());
        }

        if matches!(operator, UnaryOperator::Not) {
            let (setup, value) = self.plan_unary_not(expression, ctx, fx);
            return value_plan_from_statements(setup, value);
        }

        let op = match operator {
            UnaryOperator::Negative => "-",
            UnaryOperator::BitwiseNot => "^",
            UnaryOperator::Deref => "*",
            UnaryOperator::Not => unreachable!("Not handled above"),
        };
        ValuePlan::Unary {
            op,
            inner: Box::new(self.plan_operand(expression, ctx, fx)),
        }
    }

    /// Plan `!` (logical-not). Comparisons flip operator because `!` binds
    /// tighter than `==` in Go (`!(a == b)` must not emit as `!a == b`).
    fn plan_unary_not(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let target = expression.unwrap_parens();
        let preserve_parens = matches!(expression, Expression::Paren { .. });
        let wrap = |s: String| {
            if preserve_parens {
                format!("({})", s)
            } else {
                s
            }
        };
        if let Expression::Binary {
            operator: cmp,
            left,
            right,
            ..
        } = target
            && let Some(flipped) = flip_comparison(cmp)
        {
            let plan = self.plan_binary(&flipped, left, right, ctx, fx);
            let staged = StagedExpression::from_plan(plan, target);
            return (staged.setup, wrap(staged.value));
        }
        if matches!(target, Expression::Call { .. }) {
            let mut buffer = String::new();
            if let Some(negated) = self.try_emit_negated_call(&mut buffer, target, fx) {
                return (setup_from_string(buffer), wrap(negated));
            }
        }

        let staged = self.stage_operand(expression, ctx, fx);
        (staged.setup, format!("!{}", staged.value))
    }

    fn plan_numeric_binary_with_casts(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        info: NumericBinaryEmitInfo,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let stages = vec![
            self.stage_operand(left_expression, ctx, fx),
            self.stage_operand(right_expression, ctx, fx),
        ];
        let (setup, values) = self.sequence_structured(stages, "_left");
        let left_string = values[0].clone();
        let right_string = values[1].clone();

        let left_string = match &info.cast_left_to {
            Some(ty) => format!("{}({})", self.go_type_string(ty, fx), left_string),
            None => left_string,
        };

        let right_string = match &info.cast_right_to {
            Some(ty) => format!("{}({})", self.go_type_string(ty, fx), right_string),
            None => right_string,
        };

        let result = format!("{} {} {}", left_string, operator, right_string);
        (setup, result)
    }
}

fn flip_comparison(operator: &BinaryOperator) -> Option<BinaryOperator> {
    match operator {
        BinaryOperator::Equal => Some(BinaryOperator::NotEqual),
        BinaryOperator::NotEqual => Some(BinaryOperator::Equal),
        BinaryOperator::LessThan => Some(BinaryOperator::GreaterThanOrEqual),
        BinaryOperator::LessThanOrEqual => Some(BinaryOperator::GreaterThan),
        BinaryOperator::GreaterThan => Some(BinaryOperator::LessThanOrEqual),
        BinaryOperator::GreaterThanOrEqual => Some(BinaryOperator::LessThan),
        _ => None,
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

fn is_numeric_binary_op(operator: &BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(
        operator,
        Addition
            | Subtraction
            | Multiplication
            | Division
            | Remainder
            | BitwiseAnd
            | BitwiseOr
            | BitwiseXor
            | BitwiseAndNot
            | ShiftLeft
            | ShiftRight
            | LessThan
            | LessThanOrEqual
            | GreaterThan
            | GreaterThanOrEqual
            | Equal
            | NotEqual
    )
}

/// Common underlying numeric type when both operands lower to the same
/// numeric family; `None` if either operand is non-numeric or the two
/// numeric families differ.
fn matching_underlying_numeric(left: &Type, right: &Type) -> Option<Type> {
    let left_underlying = left.underlying_numeric_type()?;
    let right_underlying = right.underlying_numeric_type()?;
    if left_underlying.numeric_family()? != right_underlying.numeric_family()? {
        return None;
    }
    Some(left_underlying)
}

fn cast_unless_literal(is_literal: bool, target: &Type) -> Option<Type> {
    if is_literal {
        None
    } else {
        Some(target.clone())
    }
}

/// Go requires explicit casts when mixing aliased numeric types with
/// their underlying types.
fn is_casting_needed(
    operator: &BinaryOperator,
    left: &BinaryOperand<'_>,
    right: &BinaryOperand<'_>,
) -> Option<NumericBinaryEmitInfo> {
    if !is_numeric_binary_op(operator) {
        return None;
    }

    matching_underlying_numeric(&left.ty, &right.ty)?;

    let left_is_aliased = left.ty.is_aliased_numeric_type();
    let right_is_aliased = right.ty.is_aliased_numeric_type();

    if left.ty == right.ty {
        return None;
    }

    let left_is_literal = is_literal_expression(left.expression);
    let right_is_literal = is_literal_expression(right.expression);

    match (left_is_aliased, right_is_aliased) {
        (true, false) => Some(NumericBinaryEmitInfo {
            cast_left_to: None,
            cast_right_to: cast_unless_literal(right_is_literal, &left.ty),
        }),
        (false, true) => Some(NumericBinaryEmitInfo {
            cast_left_to: cast_unless_literal(left_is_literal, &right.ty),
            cast_right_to: None,
        }),
        _ => None,
    }
}
