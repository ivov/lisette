use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::program::DefinitionBody;
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
            return self.plan_numeric_binary_with_casts(
                operator,
                left_expression,
                right_expression,
                emit_info,
                ctx,
            );
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
                let staged = self.stage_operand(left_expression, ctx);
                return staged.map_rendered_as_computed(|_, value, _| {
                    GoExpression::call(
                        GoExpression::name("complex".to_string()),
                        vec![
                            GoExpression::literal("0".to_string()),
                            GoExpression::compact_binary(
                                GoExpression::opaque_with_deferred_evaluation(value, true),
                                "*",
                                GoExpression::literal(imag_coef.to_string()),
                            ),
                        ],
                    )
                });
            }
            if let Expression::Literal {
                literal: Literal::Imaginary(imag_coef),
                ..
            } = left_expression
                && right_ty.is_float()
                && !right_ty.is_complex()
            {
                let staged = self.stage_operand(right_expression, ctx);
                return staged.map_rendered_as_computed(|_, value, _| {
                    GoExpression::call(
                        GoExpression::name("complex".to_string()),
                        vec![
                            GoExpression::literal("0".to_string()),
                            GoExpression::compact_binary(
                                GoExpression::opaque_with_deferred_evaluation(value, true),
                                "*",
                                GoExpression::literal(imag_coef.to_string()),
                            ),
                        ],
                    )
                });
            }
        }

        if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
            return self.plan_short_circuit_binary(
                operator,
                left_expression,
                right_expression,
                ctx,
            );
        }

        let stages = vec![
            self.stage_composite(left_expression, ctx),
            self.stage_composite(right_expression, ctx),
        ];
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "_left");
        let effect = sequenced.effect;
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        ValuePlan::computed(
            setup,
            GoExpression::binary(
                values.next().expect("binary expression has a left operand"),
                operator.to_string(),
                values
                    .next()
                    .expect("binary expression has a right operand"),
            ),
            effect,
        )
    }

    fn plan_short_circuit_binary(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let left_staged = self.stage_composite(left_expression, ctx);
        let left_effect = left_staged.evaluation.effect;
        let left_contains_deferred_evaluation =
            left_staged.expression.contains_deferred_evaluation();
        let (left_setup, left_value) = left_staged.into_parts();

        // Wrap RHS setup in an IIFE so it runs only when control reaches the
        // RHS. Hoisting it before the operator would defeat short-circuit.
        let right_staged = self.stage_composite(right_expression, ctx);
        let right_effect = right_staged.evaluation.effect;
        let right_contains_deferred_evaluation =
            right_staged.expression.contains_deferred_evaluation();
        let (right_setup, right_value) = right_staged.into_parts();
        let right_string = if right_setup.is_empty() {
            right_value
        } else {
            format!(
                "func() bool {{\n{}return {}\n}}()",
                Renderer.render_setup(&right_setup),
                right_value
            )
        };

        ValuePlan::computed(
            left_setup,
            GoExpression::binary(
                GoExpression::opaque_with_deferred_evaluation(
                    left_value,
                    left_contains_deferred_evaluation,
                ),
                operator.to_string(),
                GoExpression::opaque_with_deferred_evaluation(
                    right_string,
                    right_contains_deferred_evaluation || !right_setup.is_empty(),
                ),
            ),
            left_effect.combine(right_effect),
        )
    }

    /// Plan a prefix unary; `!` bridges through `emit_unary_not`.
    pub(crate) fn plan_unary(
        &mut self,
        operator: &UnaryOperator,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
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
            return ValuePlan::literal("-9223372036854775808".to_string());
        }

        if matches!(operator, UnaryOperator::Not) {
            return self.plan_unary_not(expression, ctx);
        }

        let op = match operator {
            UnaryOperator::Negative => "-",
            UnaryOperator::BitwiseNot => "^",
            UnaryOperator::Deref => "*",
            UnaryOperator::Not => unreachable!("Not handled above"),
        };
        self.plan_operand(expression, ctx).unary(op)
    }

    /// Plan `!` (logical-not). Comparisons flip operator because `!` binds
    /// tighter than `==` in Go (`!(a == b)` must not emit as `!a == b`).
    fn plan_unary_not(&mut self, expression: &Expression, ctx: ExpressionContext<'_>) -> ValuePlan {
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
            && flip_preserves_nan(cmp, left, right)
        {
            let plan = self.plan_binary(&flipped, left, right, ctx);
            return if preserve_parens {
                plan.parenthesized()
            } else {
                plan
            };
        }
        if matches!(target, Expression::Call { .. }) {
            let mut setup: Vec<LoweredStatement> = Vec::new();
            if let Some(negated) = self.try_emit_negated_call(&mut setup, target) {
                return ValuePlan::computed(
                    setup,
                    GoExpression::opaque_with_deferred_evaluation(wrap(negated), true),
                    EvaluationEffect::EffectfulCall,
                );
            }
        }

        let staged = self.stage_operand(expression, ctx);
        staged.unary("!")
    }

    fn plan_numeric_binary_with_casts(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        info: NumericBinaryEmitInfo,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let stages = vec![
            self.stage_operand(left_expression, ctx),
            self.stage_operand(right_expression, ctx),
        ];
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "_left");
        let effect = sequenced.effect;
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let mut left = values
            .next()
            .expect("numeric binary expression has a left operand");
        let mut right = values
            .next()
            .expect("numeric binary expression has a right operand");
        if let Some(ty) = &info.cast_left_to {
            left = GoExpression::Conversion {
                go_type: self.go_type_string(ty),
                value: Box::new(left),
            };
        }
        if let Some(ty) = &info.cast_right_to {
            right = GoExpression::Conversion {
                go_type: self.go_type_string(ty),
                value: Box::new(right),
            };
        }
        ValuePlan::computed(
            setup,
            GoExpression::binary(left, operator.to_string(), right),
            effect,
        )
    }
}

fn flip_preserves_nan(operator: &BinaryOperator, left: &Expression, right: &Expression) -> bool {
    matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
        || (is_non_float(left) && is_non_float(right))
}

fn is_non_float(expression: &Expression) -> bool {
    expression
        .get_type()
        .underlying_simple_kind()
        .is_some_and(|kind| !kind.is_float())
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

fn is_constant_binary_op(operator: &BinaryOperator) -> bool {
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
    )
}

impl Planner<'_> {
    fn is_go_constant(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Literal { literal, .. } => {
                !matches!(literal, Literal::FormatString(_) | Literal::Slice(_))
            }
            Expression::Identifier { value, .. } => self.identifier_is_const(value),
            Expression::DotAccess {
                expression: package,
                member,
                ..
            } => self.imported_member_is_const(package, member),
            Expression::Paren { expression, .. } => self.is_go_constant(expression),
            Expression::Unary {
                operator: UnaryOperator::Negative | UnaryOperator::BitwiseNot,
                expression,
                ..
            } => self.is_go_constant(expression),
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => {
                is_constant_binary_op(operator)
                    && self.is_go_constant(left)
                    && self.is_go_constant(right)
            }
            _ => false,
        }
    }

    fn identifier_is_const(&self, value: &str) -> bool {
        match self.scope.resolve_identifier_binding(value) {
            Some(binding) => binding
                .as_go_name()
                .is_some_and(|name| self.is_go_const_binding(name)),
            None => self.is_go_const_binding(value),
        }
    }

    fn imported_member_is_const(&self, package: &Expression, member: &str) -> bool {
        let Expression::Identifier { value, .. } = package.unwrap_parens() else {
            return false;
        };
        let module = self.canonical_module(value);
        let qualified = format!("{module}.{member}");
        let body = self
            .facts
            .definition(&qualified)
            .or_else(|| {
                self.facts
                    .definition(&self.facts.qualified_current_member(value, member))
            })
            .map(|definition| &definition.body);
        match body {
            Some(DefinitionBody::Value { .. }) if module.starts_with(go_name::GO_IMPORT_PREFIX) => {
                self.facts.is_const(&qualified)
            }
            Some(DefinitionBody::Value { .. }) => true,
            _ => false,
        }
    }

    pub(crate) fn contains_untyped_constant_shift(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Paren { expression, .. } | Expression::Unary { expression, .. } => {
                self.contains_untyped_constant_shift(expression)
            }
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => {
                (matches!(
                    operator,
                    BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                ) && self.is_go_constant(left)
                    && !self.is_go_constant(right))
                    || self.contains_untyped_constant_shift(left)
                    || self.contains_untyped_constant_shift(right)
            }
            _ => false,
        }
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
