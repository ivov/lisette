use crate::Planner;
use crate::analyze::facts::EmitFacts;
use crate::calls::predicates::strip_negations;
use crate::ir::bodies::{
    Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, assign, define,
};
use crate::ir::go_expression::{BinaryOp, GoExpressionNode, UnaryOp};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{CaptureBoundary, ConstantKind, GoExpression, ValuePlan};
use crate::state::expression::ExpressionContext;
use syntax::ast::{BinaryOperator, BindingId, Expression, Literal, UnaryOperator};
use syntax::constant::{self, Constant, ConstantSource};
use syntax::program::{self, OperandConversion};
use syntax::types::{self, ScaledImaginary, Type};

struct NumericBinaryEmitInfo {
    cast_left_to: Option<Type>,
    cast_right_to: Option<Type>,
}

fn imaginary_product(value: GoExpression, coefficient: f64) -> GoExpression {
    GoExpression::call(
        GoExpression::external_name("complex".to_string()),
        vec![
            GoExpression::literal("0".to_string()),
            GoExpression::binary(
                value,
                BinaryOp::Mul,
                GoExpression::literal(coefficient.to_string()),
            ),
        ],
    )
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
    pub(crate) fn plan_binary(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        conversion: Option<OperandConversion>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if matches!(operator, BinaryOperator::Pipeline) {
            unreachable!("pipeline expressions are lowered during inference")
        }

        let left = BinaryOperand::of(left_expression);
        let right = BinaryOperand::of(right_expression);
        let typed_left = (matches!(
            operator,
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
        ) && self.is_go_constant_expression(left_expression)
            && !self.is_go_constant_expression(right_expression))
        .then_some(&left.ty);

        if let Some(casts) = conversion.map(|conversion| operand_casts(conversion, &left, &right)) {
            let left_plan = self.plan_operand(left_expression, ctx);
            let right_plan = self.plan_operand(right_expression, ctx);
            return self.combine_binary(operator, (typed_left, left_plan), right_plan, Some(casts));
        }

        let left_ty = &left.ty;
        let right_ty = &right.ty;

        if matches!(operator, BinaryOperator::Multiplication)
            && let Some(scaled) = types::scaled_imaginary(
                left_expression.imaginary_coefficient(),
                left_ty,
                right_expression.imaginary_coefficient(),
                right_ty,
            )
        {
            let (operand, coefficient) = match scaled {
                ScaledImaginary::FloatLeft(coefficient) => (left_expression, coefficient),
                ScaledImaginary::FloatRight(coefficient) => (right_expression, coefficient),
            };
            let staged = self.plan_operand(operand, ctx);
            return staged.map_expression(|_, value| imaginary_product(value, coefficient));
        }

        if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
            return self.plan_short_circuit_binary(
                operator,
                left_expression,
                right_expression,
                ctx,
            );
        }

        let left_plan = self.plan_composite_value(left_expression, ctx);
        let right_plan = self.plan_composite_value(right_expression, ctx);
        self.combine_binary(operator, (typed_left, left_plan), right_plan, None)
    }

    pub(crate) fn plan_lowered_binary(
        &mut self,
        operator: &BinaryOperator,
        left: (&BinaryOperand, ValuePlan),
        right: (&BinaryOperand, ValuePlan),
        conversion: Option<OperandConversion>,
    ) -> ValuePlan {
        let casts = conversion.map(|conversion| operand_casts(conversion, left.0, right.0));
        self.combine_binary(operator, (None, left.1), right.1, casts)
    }

    fn combine_binary(
        &mut self,
        operator: &BinaryOperator,
        (typed_left, left): (Option<&Type>, ValuePlan),
        right: ValuePlan,
        casts: Option<NumericBinaryEmitInfo>,
    ) -> ValuePlan {
        let sequenced =
            self.sequence_values(vec![left, right], CaptureBoundary::SiblingSequence, "left");
        let stability = sequenced.stability;
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let mut left = values.next().expect("binary expression has a left operand");
        let mut right = values
            .next()
            .expect("binary expression has a right operand");
        if let Some(left_ty) = typed_left {
            left = GoExpression::conversion(self.go_type(left_ty), left);
        }
        let Some(casts) = casts else {
            return ValuePlan::built_from(
                setup,
                GoExpression::binary(left, operator.into(), right),
                stability,
            );
        };
        if let Some(ty) = &casts.cast_left_to {
            left = GoExpression::conversion(self.go_type(ty), left);
        }
        if let Some(ty) = &casts.cast_right_to {
            right = GoExpression::conversion(self.go_type(ty), right);
        }
        ValuePlan::computed(setup, GoExpression::binary(left, operator.into(), right))
    }

    fn plan_short_circuit_binary(
        &mut self,
        operator: &BinaryOperator,
        left_expression: &Expression,
        right_expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let left_staged = self.plan_composite_value(left_expression, ctx);
        let right_staged = self.plan_composite_value(right_expression, ctx);
        let (mut setup, left_value) = left_staged.into_parts();
        let (mut statements, mut right_value) = right_staged.into_parts();
        if let Some(inlined) = inline_single_read(&statements, &right_value) {
            statements.clear();
            right_value = inlined;
        }
        if statements.is_empty() {
            return ValuePlan::computed(
                setup,
                GoExpression::binary(left_value, operator.into(), right_value),
            );
        }

        let result = self.fresh_declared("cond");
        let read = GoExpression::identifier(result.clone());
        let reaches_right = match operator {
            BinaryOperator::And => read.clone(),
            _ => GoExpression::unary(UnaryOp::Not, read.clone()),
        };
        statements.push(assign(read, right_value));
        setup.push(define(result.clone(), left_value));
        setup.push(
            LoweredStatement::If(IfPlan::plain(
                reaches_right,
                LoweredBlock { statements },
                ElseArm::None,
            ))
            .into(),
        );
        ValuePlan::captured(setup, result)
    }

    pub(crate) fn plan_unary(
        &mut self,
        operator: &UnaryOperator,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
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

    pub(crate) fn plan_unary_not(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if let Expression::Binary {
            operator: cmp,
            left,
            right,
            ..
        } = expression
            && let Some(flipped) = flip_comparison(cmp)
            && self.facts.flip_preserves_nan(cmp, left, right)
        {
            let conversion = self.facts.checked(expression).operand_conversion();
            return self.plan_binary(&flipped, left, right, conversion, ctx);
        }
        let (innermost, inner_negated) = strip_negations(expression);
        if let Some(predicate) = self.try_plan_fused_predicate_value(innermost, !inner_negated) {
            return predicate;
        }
        if matches!(expression, Expression::Call { .. }) {
            let mut setup: Vec<Statement> = Vec::new();
            if let Some(negated) = self.try_plan_negated_call(&mut setup, expression) {
                return ValuePlan::computed(setup, negated);
            }
        }

        let staged = self.plan_operand(expression, ctx);
        staged.unary(UnaryOp::Not)
    }

    pub(crate) fn is_go_constant_expression(&self, expression: &Expression) -> bool {
        !matches!(constant::evaluate(expression, self), Ok(None))
    }
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
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => is_literal_expression(expression),
        _ => false,
    }
}

fn cast_unless_literal(is_literal: bool, target: &Type) -> Option<Type> {
    if is_literal {
        None
    } else {
        Some(target.clone())
    }
}

fn operand_casts(
    conversion: OperandConversion,
    left: &BinaryOperand,
    right: &BinaryOperand,
) -> NumericBinaryEmitInfo {
    match conversion {
        OperandConversion::Left => NumericBinaryEmitInfo {
            cast_left_to: cast_unless_literal(left.is_literal, &right.ty),
            cast_right_to: None,
        },
        OperandConversion::Right => NumericBinaryEmitInfo {
            cast_left_to: None,
            cast_right_to: cast_unless_literal(right.is_literal, &left.ty),
        },
    }
}

fn inline_single_read(statements: &[Statement], read: &GoExpression) -> Option<GoExpression> {
    let [statement] = statements else {
        return None;
    };
    let LoweredStatement::Define(Definition { names, value }) = &statement.kind else {
        return None;
    };
    let [name] = names.as_slice() else {
        return None;
    };
    let mut inlined = read.node().clone();
    let mut reads = 0;
    (fold_read(&mut inlined, name, value.node(), &mut reads) && reads == 1)
        .then(|| read.with_node(inlined))
}

fn fold_read(
    node: &mut GoExpressionNode,
    name: &GoIdentifier,
    value: &GoExpressionNode,
    reads: &mut usize,
) -> bool {
    match node {
        GoExpressionNode::Identifier(read) if read.refers_to_same(name) => {
            *reads += 1;
            *node = value.clone();
            true
        }
        GoExpressionNode::Literal(_) => true,
        GoExpressionNode::Selector { base, .. } => fold_read(base, name, value, reads),
        GoExpressionNode::Unary { operand, .. } => fold_read(operand, name, value, reads),
        GoExpressionNode::Binary {
            operator,
            left,
            right,
        } => {
            !matches!(operator, BinaryOp::And | BinaryOp::Or)
                && fold_read(left, name, value, reads)
                && fold_read(right, name, value, reads)
        }
        _ => false,
    }
}

impl EmitFacts<'_> {
    pub(crate) fn flip_preserves_nan(
        &self,
        operator: &BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> bool {
        matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
            || (self.is_non_float(left) && self.is_non_float(right))
    }

    fn is_non_float(&self, expression: &Expression) -> bool {
        self.underlying_simple_kind(&expression.get_type())
            .is_some_and(|kind| !kind.is_float())
    }
}

impl ConstantSource for Planner<'_> {
    fn definition(&self, qualified_name: &str) -> Option<&program::Definition> {
        self.facts.definition(qualified_name)
    }

    fn local_constant(&self, binding: BindingId) -> Option<Constant> {
        self.scope.local_constant(binding)
    }
}
