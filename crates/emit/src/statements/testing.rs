use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::definitions::functions::is_test_context_ty;
use crate::expressions::{flip_comparison, flip_preserves_nan};
use crate::names::go_name::{GeneratedPackage, testkit_qualifier};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, define, expression_statement,
};
use crate::plan::go_expression::CompositeLayout;
use crate::plan::values::{GoExpression, OperandForm, ValuePlan};
use syntax::ast::{BinaryOperator, Expression, IdentifierResolution, Span, UnaryOperator};

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
            self.lower_relation_assert(operator, left, right, &mut statements)
        } else if let Some((recv, arg)) = self.as_equals_decomposition(operand) {
            self.lower_labeled_assert(recv, arg, &mut statements)
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
            .current_test_handle()
            .expect("assert without a test handle should be rejected by semantics");
        let span = operand.get_span();
        let literal = |text: String| GoExpression::literal(text);
        let mut arguments = vec![
            literal(format!("\"{kind}\"")),
            literal(format!("\"{message}\"")),
        ];
        arguments.extend(operands);
        let fail = test_context_call(GoExpression::name(handle), "FailAssert", span, arguments);
        let test = LoweredStatement::If(IfPlan::plain(
            failure_condition,
            LoweredBlock {
                statements: vec![expression_statement(fail)],
            },
            ElseArm::None,
        ));
        // The block exists only to scope the temps.
        if statements.is_empty() {
            return test;
        }
        statements.push(test);
        LoweredStatement::Block(LoweredBlock { statements })
    }

    pub(crate) fn is_test_log_call(&self, expression: &Expression) -> bool {
        let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression.unwrap_parens()
        else {
            return false;
        };
        if args.len() != 1 {
            return false;
        }
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee.unwrap_parens()
        else {
            return false;
        };
        member.as_str() == "log" && is_test_context_ty(&receiver.get_type())
    }

    pub(crate) fn lower_test_log_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let (mut statements, call) = self.lower_test_log_call(expression);
        statements.push(expression_statement(call));
        LoweredStatement::Block(LoweredBlock { statements })
    }

    pub(crate) fn lower_test_log_call(
        &mut self,
        expression: &Expression,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let Expression::Call {
            expression: callee,
            args,
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
        let mut statements = Vec::new();
        let handle = self.lower_value(receiver, ExpressionContext::value());
        statements.extend(handle.setup);
        let value = self.lower_value(&args[0], ExpressionContext::value());
        statements.extend(value.setup);

        let span = args[0].get_span();
        let call = test_context_call(
            handle.expression,
            "Log",
            span,
            vec![GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                vec![value.expression],
            )],
        );
        (statements, call)
    }

    fn lower_relation_assert(
        &mut self,
        operator: &BinaryOperator,
        left: &Expression,
        right: &Expression,
        statements: &mut Vec<LoweredStatement>,
    ) -> AssertShape {
        let (lhs, rhs) =
            self.stage_assert_operands(left, right, LiteralInlining::Allowed, statements);
        let flipped = flip_comparison(operator)
            .filter(|_| flip_preserves_nan(&self.facts, operator, left, right));
        let (cond_setup, condition) = self
            .plan_binary(
                flipped.as_ref().unwrap_or(operator),
                &lhs.expression,
                &rhs.expression,
                ExpressionContext::value(),
            )
            .into_parts();
        statements.extend(cond_setup);
        AssertShape {
            failure_condition: match flipped {
                Some(_) => condition,
                None => GoExpression::unary("!", condition),
            },
            kind: "relation",
            message: format!("expected {operator}"),
            operands: paired_operands(&lhs.rendered, &rhs.rendered),
        }
    }

    fn lower_labeled_assert(
        &mut self,
        recv: &Expression,
        arg: &Expression,
        statements: &mut Vec<LoweredStatement>,
    ) -> AssertShape {
        let recv_ty = recv.get_type();
        let (lhs, rhs) = self.stage_assert_operands(recv, arg, LiteralInlining::Denied, statements);
        let failure_condition =
            self.inequality_expression(lhs.rendered.clone(), rhs.rendered.clone(), &recv_ty, &[]);
        AssertShape {
            failure_condition,
            kind: "labeled",
            message: "expected ==".to_string(),
            operands: paired_operands(&lhs.rendered, &rhs.rendered),
        }
    }

    fn lower_bare_assert(
        &mut self,
        operand: &Expression,
        statements: &mut Vec<LoweredStatement>,
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
            GoExpression::unary("!", condition)
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
        statements: &mut Vec<LoweredStatement>,
    ) -> (AssertOperand, AssertOperand) {
        let left_plan = self.lower_value(left, ExpressionContext::value());
        let right_plan = self.lower_value(right, ExpressionContext::value());
        // Calls may change a local before the failure report reads it again.
        let names_inline = left_plan.setup.is_empty()
            && right_plan.setup.is_empty()
            && !left_plan.evaluation.effect.has_call()
            && !right_plan.evaluation.effect.has_call();
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
    ) -> Option<String> {
        let expression_ty = expression.get_type();
        let go_type = self.use_go_type(&expression_ty);
        let inlines = plan.setup.is_empty()
            && match plan.expression.syntax_form() {
                OperandForm::Name => names_inline && plan.expression.as_identifier().is_some(),
                OperandForm::Call => false,
                _ => {
                    let constant = plan.expression.constant_kind();
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
        hint: &str,
        temp_type: Option<String>,
        statements: &mut Vec<LoweredStatement>,
    ) -> AssertOperand {
        let constant = plan.expression.constant_kind();
        let (setup, value) = plan.into_parts();
        statements.extend(setup);
        let Some(go_type) = temp_type else {
            return AssertOperand {
                expression: expression.clone(),
                rendered: value,
            };
        };
        let name = self.fresh_var(Some(hint));
        self.declare(&name);
        // The rebuilt comparison must resolve this temporary's name.
        self.scope.bind(name.clone(), name.clone());
        // An untyped integer can overflow if `:=` infers `int`.
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
            } else {
                define(name.clone(), value)
            },
        );
        AssertOperand {
            expression: temp_identifier(&name, expression),
            rendered: GoExpression::name(name),
        }
    }

    fn as_equals_decomposition<'a>(
        &self,
        operand: &'a Expression,
    ) -> Option<(&'a Expression, &'a Expression)> {
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
        let recv_ty = self.facts.peel_alias(&recv.get_type());
        (recv_ty.is_slice() || recv_ty.is_map() || self.type_has_equals(&recv_ty, &[]))
            .then(|| (recv.unwrap_parens(), &args[0]))
    }
}

struct AssertShape {
    failure_condition: GoExpression,
    kind: &'static str,
    message: String,
    operands: Vec<GoExpression>,
}

struct AssertOperand {
    expression: Expression,
    rendered: GoExpression,
}

// Bare literals are not valid Go method receivers.
#[derive(Clone, Copy)]
enum LiteralInlining {
    Allowed,
    Denied,
}

fn paired_operands(lhs: &GoExpression, rhs: &GoExpression) -> Vec<GoExpression> {
    let test_kit = testkit_qualifier();
    let operand = |label: &str, value: &GoExpression| {
        GoExpression::composite(
            Some(format!("{test_kit}.Operand")),
            vec![
                (
                    Some(GoExpression::name("Label".to_string())),
                    GoExpression::literal(format!("\"{label}\"")),
                ),
                (
                    Some(GoExpression::name("Value".to_string())),
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

fn temp_identifier(name: &str, original: &Expression) -> Expression {
    Expression::Identifier {
        value: name.into(),
        ty: original.get_type(),
        span: original.get_span(),
        resolution: IdentifierResolution::Unresolved,
    }
}

fn is_assert_relation(operator: &BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    )
}
