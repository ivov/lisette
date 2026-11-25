use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::expressions::operand_order::SequencedArray;
use crate::lowering::expressions::operators::flip_preserves_nan;
use crate::lowering::expressions::{BinaryOperand, flip_comparison};
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::effect_statement;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, define,
};
use crate::target::ir::go_expression::{CompositeLayout, GoBinaryOperator, GoUnaryOperator};
use crate::target::ir::values::CaptureBoundary;
use crate::target::ir::values::{GoExpression, OperandForm};
use crate::target::names::go_name::{self, GeneratedPackage};
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;
use syntax::ast::{BinaryOperator, Expression, Span, UnaryOperator};
use syntax::program::{EqualityRecipe, OperandConversion};

pub(in crate::lowering) fn testkit_operand_type() -> GoType {
    GoType::named(
        "Operand",
        &[],
        Some(PackageUse::generated(GeneratedPackage::TestKit)),
    )
}

pub(in crate::lowering) fn test_context_call(
    handle: GoExpression,
    method: &str,
    span: Span,
    arguments: Vec<GoExpression>,
) -> GoExpression {
    let mut located_arguments = vec![
        GoExpression::integer(i128::from(span.file_id)),
        GoExpression::integer(i128::from(span.byte_offset)),
        GoExpression::integer(i128::from(span.byte_offset + span.byte_length)),
    ];
    located_arguments.extend(arguments);
    GoExpression::call(
        GoExpression::selector(handle, method.to_string()),
        located_arguments,
    )
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_assert_statement(
        &mut self,
        expression: &Expression,
        operand: &Expression,
    ) -> LoweredStatement {
        self.require_testkit();

        let mut statements = Vec::new();
        let Some(shape) = self.plan_assert_shape(expression, operand, &mut statements) else {
            return LoweredStatement::Block(LoweredBlock { statements });
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
        let literal = |text: String| GoExpression::token(text);
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
                statements: vec![effect_statement(fail)],
            },
            ElseArm::None,
        ));

        if statements.is_empty() {
            return test;
        }
        statements.push(test.into());
        LoweredStatement::Block(LoweredBlock { statements })
    }

    fn plan_assert_shape(
        &mut self,
        expression: &Expression,
        operand: &Expression,
        statements: &mut Vec<Statement>,
    ) -> Option<AssertShape> {
        if let Expression::Binary {
            operator,
            left,
            right,
            ..
        } = operand
            && is_assert_relation(operator)
        {
            let conversion = self.facts.checked(operand).operand_conversion();
            return self.lower_relation_assert(operator, left, right, conversion, statements);
        }
        if let Some(recipe) = self.facts.checked(expression).equality()
            && let Some((recv, arg)) = equals_decomposition(operand)
        {
            return self.lower_labeled_assert(recv, arg, recipe, statements);
        }
        self.lower_bare_assert(operand, statements)
    }

    pub(in crate::lowering) fn lower_test_log_statement(
        &mut self,
        call: TestLogCall<'_>,
    ) -> LoweredStatement {
        let (mut statements, call) = self.plan_test_log_call(call);
        statements.push(effect_statement(call));
        LoweredStatement::Block(LoweredBlock { statements })
    }

    pub(in crate::lowering) fn plan_test_log_call(
        &mut self,
        call: TestLogCall<'_>,
    ) -> (Vec<Statement>, GoExpression) {
        let TestLogCall { handle, value } = call;
        let handle = self.plan_value(handle, ExpressionContext::value());
        let logged = value;
        let value = self.plan_value(logged, ExpressionContext::value());
        let value = self.convert_to_recorded_slot(logged, value);
        let SequencedArray {
            setup: statements,
            values: [handle, value],
            ..
        } = self.sequence_value_array([handle, value], CaptureBoundary::SiblingSequence, "log");

        let span = logged.get_span();
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
    ) -> Option<AssertShape> {
        let (lhs, rhs) =
            self.plan_assert_operands(left, right, LiteralInlining::Allowed, statements)?;
        let flipped = flip_comparison(operator)
            .filter(|_| flip_preserves_nan(&self.facts, operator, left, right));
        let operands = paired_operands(lhs.value.expression()?, rhs.value.expression()?);
        let (condition_setup, condition) = self
            .plan_lowered_binary(
                flipped.as_ref().unwrap_or(operator),
                (&lhs.operand, lhs.value),
                (&rhs.operand, rhs.value),
                conversion,
            )
            .into_parts();

        statements.extend(condition_setup);
        Some(AssertShape {
            failure_condition: match flipped {
                Some(_) => condition,
                None => GoExpression::unary(GoUnaryOperator::Not, condition),
            },
            kind: "relation",
            message: format!("expected {operator}"),
            operands,
        })
    }

    fn lower_labeled_assert(
        &mut self,
        recv: &Expression,
        arg: &Expression,
        recipe: &EqualityRecipe,
        statements: &mut Vec<Statement>,
    ) -> Option<AssertShape> {
        let (lhs, rhs) =
            self.plan_assert_operands(recv, arg, LiteralInlining::Denied, statements)?;
        let (lhs, rhs) = (lhs.value.expression()?, rhs.value.expression()?);
        let failure_condition =
            self.equality_test(lhs.clone(), rhs.clone(), recipe, GoBinaryOperator::NotEqual);
        Some(AssertShape {
            failure_condition,
            kind: "labeled",
            message: "expected ==".to_string(),
            operands: paired_operands(lhs, rhs),
        })
    }

    fn lower_bare_assert(
        &mut self,
        operand: &Expression,
        statements: &mut Vec<Statement>,
    ) -> Option<AssertShape> {
        let ctx = ExpressionContext::value();
        let condition = if let Expression::Unary {
            operator: UnaryOperator::Not,
            expression: negated,
            ..
        } = operand
        {
            self.plan_operand(negated, ctx)
        } else if matches!(operand, Expression::Binary { .. }) {
            self.plan_operand(operand, ctx).unary(GoUnaryOperator::Not)
        } else {
            self.plan_unary_not(operand, ctx)
        };
        let (setup, condition) = condition.split_setup();
        statements.extend(setup);

        Some(AssertShape {
            failure_condition: condition.expression()?.clone(),
            kind: "bare",
            message: "assertion failed".to_string(),
            operands: Vec::new(),
        })
    }

    fn plan_assert_operands(
        &mut self,
        left: &Expression,
        right: &Expression,
        literals: LiteralInlining,
        statements: &mut Vec<Statement>,
    ) -> Option<(AssertOperand, AssertOperand)> {
        let left_plan = self.plan_value(left, ExpressionContext::value());
        if left_plan.diverges() {
            statements.extend(left_plan.split_setup().0);
            return None;
        }
        let right_plan = self.plan_value(right, ExpressionContext::value());
        if right_plan.diverges() {
            statements.extend(right_plan.split_setup().0);
            return None;
        }
        let names_inline = left_plan.setup().is_empty()
            && right_plan.setup().is_empty()
            && left_plan.effects().can_duplicate()
            && right_plan.effects().can_duplicate();
        let left_temp =
            self.queries()
                .assert_operand_temp_type(left, &left_plan, literals, names_inline);
        let right_temp =
            self.queries()
                .assert_operand_temp_type(right, &right_plan, literals, names_inline);
        let lhs = self.bind_assert_operand(left, left_plan, "assertLeft", left_temp, statements);
        let rhs =
            self.bind_assert_operand(right, right_plan, "assertRight", right_temp, statements);
        Some((lhs, rhs))
    }

    fn bind_assert_operand(
        &mut self,
        expression: &Expression,
        plan: ValuePlan,
        hint: &'static str,
        temp_type: Option<GoType>,
        statements: &mut Vec<Statement>,
    ) -> AssertOperand {
        let constant = plan.expression().and_then(GoExpression::constant_kind);
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
            .queries()
            .constant_needs_go_type(constant, &expression.get_type())
            .is_some();
        statements.push(
            if constant_needs_type || self.queries().is_go_constant_expression(expression) {
                LoweredStatement::VarDeclaration {
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
    } = &**callee
    else {
        return None;
    };

    if member != "equals" || args.len() != 1 {
        return None;
    }
    Some((recv, &args[0]))
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
                    GoExpression::token(format!("\"{label}\"")),
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

#[derive(Clone, Copy)]
pub(in crate::lowering) struct TestLogCall<'e> {
    handle: &'e Expression,
    value: &'e Expression,
}

pub(in crate::lowering) fn test_log_call<'e>(
    facts: &EmitFacts<'_>,
    expression: &'e Expression,
) -> Option<TestLogCall<'e>> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return None;
    };
    let is_test_log = facts
        .checked(callee)
        .callee()
        .definition
        .as_deref()
        .is_some_and(|definition| definition == go_name::TEST_LOG_METHOD);
    let Expression::DotAccess {
        expression: handle, ..
    } = &**callee
    else {
        return None;
    };
    let [value] = args.as_slice() else {
        return None;
    };
    is_test_log.then_some(TestLogCall { handle, value })
}

impl PlannerQueries<'_, '_> {
    fn assert_operand_temp_type(
        &self,
        expression: &Expression,
        plan: &ValuePlan,
        literals: LiteralInlining,
        names_inline: bool,
    ) -> Option<GoType> {
        let expression_ty = expression.get_type();
        let go_type = self.go_type(&expression_ty);
        let inlines = plan.setup().is_empty()
            && match plan.expression().map(GoExpression::syntax_form) {
                None | Some(OperandForm::Call) => false,
                Some(OperandForm::Name) => {
                    names_inline
                        && plan
                            .expression()
                            .and_then(GoExpression::as_identifier)
                            .is_some()
                }
                Some(_) => {
                    let constant = plan.expression().and_then(GoExpression::constant_kind);
                    matches!(literals, LiteralInlining::Allowed)
                        && constant.is_some()
                        && self
                            .constant_needs_go_type(constant, &expression_ty)
                            .is_none()
                }
            };

        (!inlines).then_some(go_type)
    }
}
