use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::{BinaryOperand, flip_comparison, flip_preserves_nan};
use crate::names::go_name::{self, GeneratedPackage};
use crate::names::packages::PackageUse;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, define, expression_statement,
};
use crate::plan::go_expression::{CompositeLayout, UnaryOp};
use crate::plan::values::CaptureBoundary;
use crate::plan::values::{GoExpression, OperandForm, ValuePlan};
use crate::types::go_type::GoType;
use syntax::ast::{BinaryOperator, Expression, Span, UnaryOperator};
use syntax::program::{EqualityRecipe, OperandConversion};

pub(crate) fn testkit_operand_type() -> GoType {
    GoType::named(
        "Operand",
        &[],
        Some(PackageUse::generated(GeneratedPackage::TestKit)),
    )
}

pub(crate) fn test_context_call(
    handle: GoExpression,
    method: &str,
    span: Span,
    arguments: Vec<GoExpression>,
) -> GoExpression {
    let mut located_arguments = vec![
        GoExpression::literal(span.file_id.to_string()),
        GoExpression::literal(span.byte_offset.to_string()),
        GoExpression::literal((span.byte_offset + span.byte_length).to_string()),
    ];
    located_arguments.extend(arguments);
    GoExpression::call(
        GoExpression::selector(handle, method.to_string()),
        located_arguments,
    )
}

impl Planner<'_> {
    pub(crate) fn lower_assert_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let Expression::Assert {
            expression: operand,
            ..
        } = expression
        else {
            unreachable!("lower_assert_statement requires an Assert expression");
        };
        let operand = operand.unwrap_parens();
        self.require_testkit();

        let mut statements = Vec::new();
        let shape = if let Expression::Binary {
            operator,
            left,
            right,
            ..
        } = operand
            && is_assert_relation(operator)
        {
            let conversion = self.facts.checked(operand).operand_conversion();
            self.lower_relation_assert(operator, left, right, conversion, &mut statements)
        } else if let Some(recipe) = self.facts.checked(expression).equality()
            && let Some((recv, arg)) = equals_decomposition(operand)
        {
            self.lower_labeled_assert(recv, arg, recipe, &mut statements)
        } else {
            self.lower_bare_assert(operand, &mut statements)
        };

        let AssertShape {
            failure_condition,
            kind,
            message,
            operands,
        } = shape;
        let handle = self
            .scope
            .current_test_handle()
            .expect("assert without a test handle should be rejected by semantics")
            .clone();
        let span = operand.get_span();
        let literal = |text: String| GoExpression::literal(text);
        let mut arguments = vec![
            literal(format!("\"{kind}\"")),
            literal(format!("\"{message}\"")),
        ];
        arguments.extend(operands);
        let fail = test_context_call(
            GoExpression::identifier(handle),
            "FailAssert",
            span,
            arguments,
        );
        let test = LoweredStatement::If(IfPlan::plain(
            failure_condition,
            LoweredBlock {
                statements: vec![expression_statement(fail)],
            },
            ElseArm::None,
        ));
        if statements.is_empty() {
            return test;
        }
        statements.push(test.into());
        LoweredStatement::Block(LoweredBlock { statements })
    }

    pub(crate) fn is_test_log_call(&self, expression: &Expression) -> bool {
        let Expression::Call {
            expression: callee, ..
        } = expression.unwrap_parens()
        else {
            return false;
        };
        self.facts
            .checked(callee)
            .callee()
            .definition
            .as_deref()
            .is_some_and(|definition| definition == go_name::TEST_LOG_METHOD)
    }

    pub(crate) fn lower_test_log_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let (mut statements, call) = self.lower_test_log_call(expression);
        statements.push(expression_statement(call));
        LoweredStatement::Block(LoweredBlock { statements })
    }

    pub(crate) fn lower_test_log_call(
        &mut self,
        expression: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let Expression::Call {
            expression: callee,
            args: arguments,
            ..
        } = expression.unwrap_parens()
        else {
            unreachable!("lower_test_log_call requires a call");
        };
        let Expression::DotAccess {
            expression: receiver,
            ..
        } = callee.unwrap_parens()
        else {
            unreachable!("lower_test_log_call requires a method receiver");
        };
        let handle = self.lower_value(receiver, ExpressionContext::value());
        let value = self.lower_value(&arguments[0], ExpressionContext::value());
        let sequenced =
            self.sequence_values(vec![handle, value], CaptureBoundary::SiblingSequence, "log");
        let statements = sequenced.setup;
        let [handle, value]: [GoExpression; 2] = sequenced
            .values
            .try_into()
            .expect("handle and logged value");

        let span = arguments[0].get_span();
        let call = test_context_call(
            handle,
            "Log",
            span,
            vec![GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                vec![value],
            )],
        );
        (statements, call)
    }

    fn lower_relation_assert(
        &mut self,
        operator: &BinaryOperator,
        left: &Expression,
        right: &Expression,
        conversion: Option<OperandConversion>,
        statements: &mut Vec<Statement>,
    ) -> AssertShape {
        let (lhs, rhs) =
            self.stage_assert_operands(left, right, LiteralInlining::Allowed, statements);
        let flipped = flip_comparison(operator)
            .filter(|_| flip_preserves_nan(&self.facts, operator, left, right));
        let operands = paired_operands(lhs.value.expression(), rhs.value.expression());
        let (cond_setup, condition) = self
            .plan_lowered_binary(
                flipped.as_ref().unwrap_or(operator),
                (&lhs.operand, lhs.value),
                (&rhs.operand, rhs.value),
                conversion,
            )
            .into_parts();
        statements.extend(cond_setup);
        AssertShape {
            failure_condition: match flipped {
                Some(_) => condition,
                None => GoExpression::unary(UnaryOp::Not, condition),
            },
            kind: "relation",
            message: format!("expected {operator}"),
            operands,
        }
    }

    fn lower_labeled_assert(
        &mut self,
        recv: &Expression,
        arg: &Expression,
        recipe: &EqualityRecipe,
        statements: &mut Vec<Statement>,
    ) -> AssertShape {
        let (lhs, rhs) = self.stage_assert_operands(recv, arg, LiteralInlining::Denied, statements);
        let (lhs, rhs) = (lhs.value.expression(), rhs.value.expression());
        let failure_condition = self.inequality_expression(lhs.clone(), rhs.clone(), recipe);
        AssertShape {
            failure_condition,
            kind: "labeled",
            message: "expected ==".to_string(),
            operands: paired_operands(lhs, rhs),
        }
    }

    fn lower_bare_assert(
        &mut self,
        operand: &Expression,
        statements: &mut Vec<Statement>,
    ) -> AssertShape {
        let ctx = ExpressionContext::value();
        let failure_condition = if let Expression::Unary {
            operator: UnaryOperator::Not,
            expression: negated,
            ..
        } = operand
        {
            let (setup, condition) = self.plan_operand(negated, ctx).into_parts();
            statements.extend(setup);
            condition
        } else if matches!(operand, Expression::Binary { .. }) {
            let (setup, condition) = self.plan_operand(operand, ctx).into_parts();
            statements.extend(setup);
            GoExpression::unary(UnaryOp::Not, condition)
        } else {
            let (setup, condition) = self.plan_unary_not(operand, ctx).into_parts();
            statements.extend(setup);
            condition
        };
        AssertShape {
            failure_condition,
            kind: "bare",
            message: "assertion failed".to_string(),
            operands: Vec::new(),
        }
    }

    fn stage_assert_operands(
        &mut self,
        left: &Expression,
        right: &Expression,
        literals: LiteralInlining,
        statements: &mut Vec<Statement>,
    ) -> (AssertOperand, AssertOperand) {
        let left_plan = self.lower_value(left, ExpressionContext::value());
        let right_plan = self.lower_value(right, ExpressionContext::value());
        let names_inline = left_plan.setup().is_empty()
            && right_plan.setup().is_empty()
            && left_plan.effects().can_duplicate()
            && right_plan.effects().can_duplicate();
        let left_temp = self.assert_operand_temp_type(left, &left_plan, literals, names_inline);
        let right_temp = self.assert_operand_temp_type(right, &right_plan, literals, names_inline);
        let lhs = self.bind_assert_operand(left, left_plan, "assertLeft", left_temp, statements);
        let rhs =
            self.bind_assert_operand(right, right_plan, "assertRight", right_temp, statements);
        (lhs, rhs)
    }

    fn assert_operand_temp_type(
        &mut self,
        expression: &Expression,
        plan: &ValuePlan,
        literals: LiteralInlining,
        names_inline: bool,
    ) -> Option<GoType> {
        let expression_ty = expression.get_type();
        let go_type = self.go_type(&expression_ty);
        let inlines = plan.setup().is_empty()
            && match plan.expression().syntax_form() {
                OperandForm::Name => names_inline && plan.expression().as_identifier().is_some(),
                OperandForm::Call => false,
                _ => {
                    let constant = plan.expression().constant_kind();
                    matches!(literals, LiteralInlining::Allowed)
                        && constant.is_some()
                        && self
                            .constant_needs_go_type(constant, &expression_ty)
                            .is_none()
                }
            };
        (!inlines).then_some(go_type)
    }

    fn bind_assert_operand(
        &mut self,
        expression: &Expression,
        plan: ValuePlan,
        hint: &'static str,
        temp_type: Option<GoType>,
        statements: &mut Vec<Statement>,
    ) -> AssertOperand {
        let constant = plan.expression().constant_kind();
        let (setup, plan) = plan.split_setup();
        statements.extend(setup);
        let Some(go_type) = temp_type else {
            return AssertOperand {
                operand: BinaryOperand::of(expression),
                value: plan,
            };
        };
        let (_, value) = plan.into_parts();
        let name = self.fresh_declared(hint);
        let constant_needs_type = self
            .constant_needs_go_type(constant, &expression.get_type())
            .is_some();
        statements.push(
            if constant_needs_type || self.is_go_constant_expression(expression) {
                LoweredStatement::VarDecl {
                    name: name.clone(),
                    go_type,
                    value: Some(value),
                }
                .into()
            } else {
                define(name.clone(), value)
            },
        );
        AssertOperand {
            operand: BinaryOperand {
                ty: expression.get_type(),
                is_literal: false,
            },
            value: ValuePlan::captured(Vec::new(), name),
        }
    }
}

fn equals_decomposition(operand: &Expression) -> Option<(&Expression, &Expression)> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = operand
    else {
        return None;
    };
    let Expression::DotAccess {
        expression: recv,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    if member != "equals" || args.len() != 1 {
        return None;
    }
    Some((recv.unwrap_parens(), &args[0]))
}

struct AssertShape {
    failure_condition: GoExpression,
    kind: &'static str,
    message: String,
    operands: Vec<GoExpression>,
}

struct AssertOperand {
    operand: BinaryOperand,
    value: ValuePlan,
}

#[derive(Clone, Copy)]
enum LiteralInlining {
    Allowed,
    Denied,
}

fn paired_operands(lhs: &GoExpression, rhs: &GoExpression) -> Vec<GoExpression> {
    let operand = |label: &str, value: &GoExpression| {
        GoExpression::struct_literal(
            testkit_operand_type(),
            vec![
                (
                    "Label".to_string(),
                    GoExpression::literal(format!("\"{label}\"")),
                ),
                (
                    "Value".to_string(),
                    GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                        vec![value.clone()],
                    ),
                ),
            ],
            CompositeLayout::Inline { padded: false },
        )
    };
    vec![operand("left", lhs), operand("right", rhs)]
}

fn is_assert_relation(operator: &BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    )
}
