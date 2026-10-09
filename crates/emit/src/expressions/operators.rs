use crate::Planner;
use crate::analyze::facts::EmitFacts;
use crate::calls::predicates::strip_negations;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::plan::bodies::{LoweredBlock, Statement};
use crate::plan::go_expression::{BinaryOp, FunctionLiteralLayout, UnaryOp};
use crate::plan::values::{
    CaptureBoundary, ConstantKind, EvaluationEffect, GoExpression, ValuePlan,
};
use syntax::ast::{BinaryOperator, Expression, IdentifierResolution, Literal, UnaryOperator};
use syntax::program::DefinitionBody;
use syntax::types::Type;

struct NumericBinaryEmitInfo {
    cast_left_to: Option<Type>,
    cast_right_to: Option<Type>,
}

pub(crate) struct BinaryOperand {
    pub(crate) ty: Type,
    pub(crate) is_literal: bool,
}

impl BinaryOperand {
    pub(crate) fn of(expression: &Expression) -> Self {
        Self {
            ty: expression.get_type(),
            is_literal: is_literal_expression(expression),
        }
    }
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
            unreachable!("pipeline expressions are lowered during inference")
        }

        let left = BinaryOperand::of(left_expression);
        let right = BinaryOperand::of(right_expression);

        if let Some(casts) = is_casting_needed(&self.facts, operator, &left, &right) {
            let left_plan = self.plan_operand(left_expression, ctx);
            let right_plan = self.plan_operand(right_expression, ctx);
            return self.combine_binary(operator, left_plan, right_plan, Some(casts));
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
                let staged = self.plan_operand(left_expression, ctx);
                return staged.map_expression(|_, value| {
                    GoExpression::call(
                        GoExpression::name("complex".to_string()),
                        vec![
                            GoExpression::literal("0".to_string()),
                            GoExpression::binary(
                                value,
                                BinaryOp::Mul,
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
                let staged = self.plan_operand(right_expression, ctx);
                return staged.map_expression(|_, value| {
                    GoExpression::call(
                        GoExpression::name("complex".to_string()),
                        vec![
                            GoExpression::literal("0".to_string()),
                            GoExpression::binary(
                                value,
                                BinaryOp::Mul,
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

        let left_plan = self.lower_composite_value(left_expression, ctx);
        let right_plan = self.lower_composite_value(right_expression, ctx);
        self.combine_binary(operator, left_plan, right_plan, None)
    }

    pub(crate) fn plan_lowered_binary(
        &mut self,
        operator: &BinaryOperator,
        left: (&BinaryOperand, ValuePlan),
        right: (&BinaryOperand, ValuePlan),
    ) -> ValuePlan {
        let casts = is_casting_needed(&self.facts, operator, left.0, right.0);
        self.combine_binary(operator, left.1, right.1, casts)
    }

    fn combine_binary(
        &mut self,
        operator: &BinaryOperator,
        left: ValuePlan,
        right: ValuePlan,
        casts: Option<NumericBinaryEmitInfo>,
    ) -> ValuePlan {
        let sequenced =
            self.sequence_values(vec![left, right], CaptureBoundary::SiblingSequence, "left");
        let effect = sequenced.effect;
        let stability = sequenced.stability;
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let mut left = values.next().expect("binary expression has a left operand");
        let mut right = values
            .next()
            .expect("binary expression has a right operand");
        let Some(casts) = casts else {
            return ValuePlan::built_from(
                setup,
                GoExpression::binary(left, operator.into(), right),
                effect,
                stability,
            );
        };
        if let Some(ty) = &casts.cast_left_to {
            left = GoExpression::conversion(self.use_go_type(ty), left);
        }
        if let Some(ty) = &casts.cast_right_to {
            right = GoExpression::conversion(self.use_go_type(ty), right);
        }
        ValuePlan::computed(
            setup,
            GoExpression::binary(left, operator.into(), right),
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
        let left_staged = self.lower_composite_value(left_expression, ctx);
        let left_effect = left_staged.facts().effect;

        // Wrap RHS setup in an IIFE so it runs only when control reaches the
        // RHS. Hoisting it before the operator would defeat short-circuit.
        let right_staged = self.lower_composite_value(right_expression, ctx);
        let right_effect = right_staged.facts().effect;
        let (mut statements, right_value) = right_staged.into_parts();
        let right_value = if statements.is_empty() {
            right_value
        } else {
            statements.push(plain_return(right_value));
            GoExpression::immediate_call(
                "bool".to_string(),
                LoweredBlock { statements },
                FunctionLiteralLayout::MultiLine,
            )
        };

        let (left_setup, left_value) = left_staged.into_parts();
        ValuePlan::computed(
            left_setup,
            GoExpression::binary(left_value, operator.into(), right_value),
            left_effect.combine(right_effect),
        )
    }

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
            return ValuePlan::constant("-9223372036854775808".to_string(), ConstantKind::Int);
        }

        if matches!(operator, UnaryOperator::Not) {
            return self.plan_unary_not(expression, ctx);
        }

        let operand = self.plan_operand(expression, ctx);
        match operator {
            UnaryOperator::Negative => operand.unary(UnaryOp::Negate),
            UnaryOperator::BitwiseNot => operand.unary(UnaryOp::Complement),
            UnaryOperator::Deref => operand.dereference(),
            UnaryOperator::Not => unreachable!("Not handled above"),
        }
    }

    /// Plan `!` (logical-not). Comparisons flip operator because `!` binds
    /// tighter than `==` in Go (`!(a == b)` must not emit as `!a == b`).
    pub(crate) fn plan_unary_not(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let target = expression.unwrap_parens();
        if let Expression::Binary {
            operator: cmp,
            left,
            right,
            ..
        } = target
            && let Some(flipped) = flip_comparison(cmp)
            && flip_preserves_nan(&self.facts, cmp, left, right)
        {
            return self.plan_binary(&flipped, left, right, ctx);
        }
        let (innermost, inner_negated) = strip_negations(expression);
        if let Some(predicate) = self.lower_fused_predicate_value(innermost, !inner_negated) {
            return predicate;
        }
        if matches!(target, Expression::Call { .. }) {
            let mut setup: Vec<Statement> = Vec::new();
            if let Some(negated) = self.try_emit_negated_call(&mut setup, target) {
                return ValuePlan::computed(setup, negated, EvaluationEffect::EffectfulCall);
            }
        }

        let staged = self.plan_operand(expression, ctx);
        staged.unary(UnaryOp::Not)
    }
}

/// `!(a < b)` holds for a NaN operand where `a >= b` does not, so only `==`
/// and `!=` flip unconditionally.
pub(crate) fn flip_preserves_nan(
    facts: &EmitFacts<'_>,
    operator: &BinaryOperator,
    left: &Expression,
    right: &Expression,
) -> bool {
    matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
        || (is_non_float(facts, left) && is_non_float(facts, right))
}

fn is_non_float(facts: &EmitFacts<'_>, expression: &Expression) -> bool {
    facts
        .underlying_simple_kind(&expression.get_type())
        .is_some_and(|kind| !kind.is_float())
}

pub(crate) fn flip_comparison(operator: &BinaryOperator) -> Option<BinaryOperator> {
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

impl Planner<'_> {
    pub(crate) fn is_go_constant_expression(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Literal { literal, .. } => {
                !matches!(literal, Literal::FormatString(_) | Literal::Slice(_))
            }
            Expression::Identifier {
                value, resolution, ..
            } => self.identifier_is_const(value, resolution),
            Expression::DotAccess {
                expression: package,
                member,
                ..
            } => self.imported_member_is_const(package, member),
            Expression::Paren { expression, .. } => self.is_go_constant_expression(expression),
            Expression::Unary {
                operator: UnaryOperator::Negative | UnaryOperator::Not | UnaryOperator::BitwiseNot,
                expression,
                ..
            } => self.is_go_constant_expression(expression),
            Expression::Binary {
                operator,
                left,
                right,
                ..
            } => {
                !matches!(operator, BinaryOperator::Pipeline)
                    && self.is_go_constant_expression(left)
                    && self.is_go_constant_expression(right)
            }
            _ => false,
        }
    }

    fn identifier_is_const(&self, value: &str, resolution: &IdentifierResolution) -> bool {
        match self
            .scope
            .resolve_identifier_with_resolution(value, resolution)
        {
            Some(binding) => binding.is_go_const(),
            None => resolution
                .definition()
                .is_some_and(|symbol| self.package.is_go_const_binding(symbol)),
        }
    }

    fn imported_member_is_const(&self, package: &Expression, member: &str) -> bool {
        let package = package.unwrap_parens();
        let Expression::Identifier { value, .. } = package else {
            return false;
        };
        let package = package.get_type();
        let package = package.as_import_namespace().unwrap_or(value);
        let qualified = format!("{package}.{member}");
        let body = self
            .facts
            .definition(&qualified)
            .or_else(|| {
                self.facts
                    .definition(&self.facts.qualified_current_member(value, member))
            })
            .map(|definition| &definition.body);
        match body {
            Some(DefinitionBody::Value { .. })
                if package.starts_with(go_name::GO_IMPORT_PREFIX) =>
            {
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
                let is_untyped_shift = matches!(
                    operator,
                    BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                ) && self.is_go_constant_expression(left)
                    && !self.is_go_constant_expression(right);
                is_untyped_shift
                    || [left, right]
                        .into_iter()
                        .any(|operand| self.contains_untyped_constant_shift(operand))
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
fn matching_underlying_numeric(facts: &EmitFacts<'_>, left: &Type, right: &Type) -> Option<Type> {
    let left_underlying = facts.underlying_numeric_type(left)?;
    let right_underlying = facts.underlying_numeric_type(right)?;
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
    facts: &EmitFacts<'_>,
    operator: &BinaryOperator,
    left: &BinaryOperand,
    right: &BinaryOperand,
) -> Option<NumericBinaryEmitInfo> {
    if !is_numeric_binary_op(operator) {
        return None;
    }

    matching_underlying_numeric(facts, &left.ty, &right.ty)?;

    let left_is_aliased = facts.is_aliased_numeric_type(&left.ty);
    let right_is_aliased = facts.is_aliased_numeric_type(&right.ty);

    if left.ty == right.ty {
        return None;
    }

    match (left_is_aliased, right_is_aliased) {
        (true, false) => Some(NumericBinaryEmitInfo {
            cast_left_to: None,
            cast_right_to: cast_unless_literal(right.is_literal, &left.ty),
        }),
        (false, true) => Some(NumericBinaryEmitInfo {
            cast_left_to: cast_unless_literal(left.is_literal, &right.ty),
            cast_right_to: None,
        }),
        _ => None,
    }
}
