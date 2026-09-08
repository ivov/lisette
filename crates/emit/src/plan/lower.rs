use crate::Planner;
use crate::abi::transition::try_emit_lowered_tail_return;
use crate::calls::comma_ok::PairCondition;
use crate::calls::predicates::strip_negations;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::control_flow::targets::legalize_source_loop;
use crate::definitions::ConstScope;
use crate::definitions::functions::{is_breakless_loop, is_go_never, is_test_context_ty};
use crate::expressions::{flip_comparison, flip_preserves_nan};
use crate::names::go_name::{GeneratedPackage, testkit_qualifier};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    PlacePlan, define, directed, directed_first, expression_statement,
};
use crate::plan::go_expression::CompositeLayout;
use crate::plan::placement::{collapse_declared_temp, requires_temp_var, try_elide_tail_let};
use crate::plan::values::{GoExpression, OperandForm, ValuePlan};
use std::slice;
use syntax::ast::{
    BinaryOperator, Expression, IdentifierResolution, IfLetAlternative, Literal, MatchArm, Pattern,
    Span, UnaryOperator,
};
use syntax::types::Type;

fn if_let_match_arms(
    pattern: &Pattern,
    consequence: &Expression,
    alternative: &IfLetAlternative,
    span: Span,
) -> Vec<MatchArm> {
    let alternative = alternative
        .expression()
        .cloned()
        .unwrap_or(Expression::Unit {
            ty: Type::unit(),
            span,
        });
    vec![
        MatchArm {
            pattern: pattern.clone(),
            guard: None,
            expression: Box::new(consequence.clone()),
        },
        MatchArm {
            pattern: Pattern::WildCard {
                span: alternative.get_span(),
            },
            guard: None,
            expression: Box::new(alternative.clone()),
        },
    ]
}

impl Planner<'_> {
    /// Allocate a fresh operand-temp result var and its `var V T` declaration
    /// as a typed setup leaf. The control-flow that assigns it follows as a
    /// typed `If`/`Loop`/`Match`/`Select` statement.
    pub(crate) fn operand_temp_declaration(&mut self, ty: &Type) -> (String, LoweredStatement) {
        let result_var = self.fresh_var(None);
        let declaration = LoweredStatement::VarDecl {
            name: result_var.clone(),
            go_type: self.use_go_type(ty),
            value: None,
        };
        self.declare(&result_var);
        (result_var, declaration)
    }

    /// Lower a value-position `if`/`if let`/`match`/`select` to a fresh
    /// operand-temp variable. Only valid for non-never result types; never-typed
    /// branches route through `lower_to_operand_temp` as a diverging statement.
    pub(crate) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let target = GoExpression::name(result_var.clone());
        let block = self.lower_branching_to_block(
            expression,
            &PlacePlan::Assign {
                local: &target,
                target_ty: Some(ty),
            },
        );
        let mut setup = vec![declaration];
        setup.extend(block.statements);
        collapse_declared_temp(
            &mut setup,
            &result_var,
            self.short_declaration_keeps_type(ty),
        );
        ValuePlan::captured(setup, result_var)
    }

    /// Lower a value-position `loop` to a fresh operand-temp variable.
    /// Declares `var V T`, pushes `V` as the current loop result slot so
    /// `break value` assigns into it, lowers the loop, then renders.
    pub(crate) fn plan_loop_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let Expression::Loop { body, .. } = expression else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let plan = self.with_loop(result_var.clone(), |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        });
        ValuePlan::captured(vec![declaration, LoweredStatement::Loop(plan)], result_var)
    }

    fn lower_body_until_diverge(
        &mut self,
        rest: &[Expression],
        last: &Expression,
    ) -> (Vec<LoweredStatement>, bool) {
        let mut statements: Vec<LoweredStatement> = Vec::with_capacity(rest.len() + 1);
        for item in rest {
            let statement = self.lower_statement(item);
            let diverged = statement.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                return (statements, true);
            }
        }
        statements.extend(self.lower_return_tail(last));
        (statements, false)
    }

    pub(crate) fn lower_function_body(
        &mut self,
        body: &Expression,
        should_return: bool,
    ) -> LoweredBlock {
        if !should_return {
            return self.lower_block_as_body(body);
        }

        let items: &[Expression] = if let Expression::Block { items, .. } = body {
            items
        } else {
            slice::from_ref(body)
        };

        let Some((last, rest)) = items.split_last() else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        let (mut statements, diverged) = self.lower_body_until_diverge(rest, last);
        if diverged {
            return LoweredBlock { statements };
        }

        // A unit/statement-only body under a non-unit signature has no value to
        // return, so `lower_return_tail` emits it as a bare statement. A
        // function body must still close with an explicit zero-value return;
        // branch arms instead rely on a trailing unreachable panic.
        let is_statement_only = matches!(
            last,
            Expression::Assignment { .. } | Expression::Let { .. } | Expression::Const { .. }
        );
        let is_unit_tail = !is_statement_only
            && !matches!(last, Expression::Return { .. })
            && last.get_type().is_unit();
        let return_ctx = self.return_ctx();
        if (is_statement_only || is_unit_tail)
            && let Some(return_ty) = return_ctx.ty().filter(|ty| !ty.is_unit())
        {
            let return_ty = return_ty.clone();
            let zero = self.zero_value_expression(&return_ty);
            statements.push(plain_return(zero));
        }

        LoweredBlock { statements }
    }

    /// Lower a single statement in the enclosing return context.
    pub(crate) fn lower_statement(&mut self, expression: &Expression) -> LoweredStatement {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let plan = self.lower_if(
                    condition,
                    consequence,
                    alternative.as_deref(),
                    &PlacePlan::Statement,
                );
                self.directed_at(expression, LoweredStatement::If(plan))
            }
            Expression::Loop { body, .. } => {
                let plan = self.lower_infinite_loop(body);
                self.directed_at(expression, LoweredStatement::Loop(plan))
            }
            Expression::While {
                condition, body, ..
            } => {
                let plan = self.lower_while(condition, body);
                self.directed_at(expression, LoweredStatement::Loop(plan))
            }
            Expression::Block { .. } => LoweredStatement::Block(
                self.with_scope(|this| this.lower_block_as_body(expression)),
            ),
            Expression::For { .. } => self.lower_for_statement(expression),
            Expression::Continue { .. } => {
                let target = self
                    .current_loop_id()
                    .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source);
                self.directed_at(expression, LoweredStatement::Continue(target))
            }
            Expression::Break { value: None, .. } => {
                let target = self
                    .current_loop_id()
                    .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source);
                self.directed_at(expression, LoweredStatement::Break(target))
            }
            Expression::Break {
                value: Some(value), ..
            } => {
                let plan = self.build_break_value_plan(value);
                self.directed_at(expression, LoweredStatement::BreakValue(plan))
            }
            Expression::Const {
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let Some(value) = value.value() else {
                    return LoweredStatement::Block(LoweredBlock { statements: vec![] });
                };
                let plan = self.build_const_plan(identifier, value, ty, ConstScope::Local);
                self.directed_at(expression, LoweredStatement::Const(plan))
            }
            Expression::Return {
                expression: value, ..
            } => {
                let plan = self.build_return_plan(value);
                self.directed_at(expression, LoweredStatement::Return(plan))
            }
            Expression::Let {
                binding,
                value,
                mode,
                ..
            } => {
                let plan = self.build_let_plan(
                    binding,
                    value,
                    mode.else_block(),
                    binding.is_mutable(),
                    mode.is_assert(),
                );
                self.directed_at(expression, LoweredStatement::Let(plan))
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                let statement =
                    self.build_assignment_plan(target, value, compound_operator.as_ref());
                self.directed_at(expression, statement)
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => {
                let arms = if_let_match_arms(pattern, consequence, alternative, *span);
                let body = self.lower_match_to_block(scrutinee, &arms, &PlacePlan::Statement);
                self.directed_at(expression, LoweredStatement::Body(body))
            }
            Expression::Match { subject, arms, .. } => {
                let body = self.lower_match_to_block(subject, arms, &PlacePlan::Statement);
                self.directed_at(expression, LoweredStatement::Body(body))
            }
            Expression::Select { arms, .. } => {
                let plan = self.lower_select(arms, &PlacePlan::Statement);
                self.directed_at(expression, LoweredStatement::Select(plan))
            }
            Expression::WhileLet { .. } => self.lower_while_let_statement(expression),
            Expression::Assert { .. } => self.lower_assert_statement(expression),
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::ImplBlock { .. } => {
                unreachable!("the parser rejects item definitions inside function bodies")
            }
            Expression::Call { .. } if self.is_test_log_call(expression) => {
                self.lower_test_log_statement(expression)
            }
            _ => self.lower_expression_statement(expression),
        }
    }

    /// Wrap a lowered statement with `expression`'s source-line directive when
    /// sourcemaps are enabled (a no-op wrapper otherwise).
    fn directed_at(&self, expression: &Expression, stmt: LoweredStatement) -> LoweredStatement {
        directed(self.maybe_line_directive(&expression.get_span()), stmt)
    }

    /// Lower the statement-position fall-through: Task/Defer (async value),
    /// `expr?` propagation, or an otherwise-discarded expression value. The
    /// directive rides on the wrapper so the rendered body stays directive-free.
    fn lower_expression_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let unwrapped = expression.unwrap_parens();
        let directive = self.maybe_line_directive(&expression.get_span());
        let statement = if matches!(
            unwrapped,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(unwrapped, ExpressionContext::value());
            LoweredStatement::Body(LoweredBlock {
                statements: value.setup,
            })
        } else if let Expression::Propagate {
            expression: inner, ..
        } = unwrapped
        {
            LoweredStatement::Body(LoweredBlock {
                statements: self.lower_propagate_statement(inner),
            })
        } else {
            LoweredStatement::Body(LoweredBlock {
                statements: self.lower_discard_value(unwrapped),
            })
        };
        directed(directive, statement)
    }

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
            literal(span.file_id.to_string()),
            literal(span.byte_offset.to_string()),
            literal((span.byte_offset + span.byte_length).to_string()),
            literal(format!("\"{kind}\"")),
            literal(format!("\"{message}\"")),
        ];
        arguments.extend(operands);
        let fail = GoExpression::call(
            GoExpression::selector(GoExpression::name(handle), "FailAssert".to_string()),
            arguments,
        );
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

    fn lower_test_log_statement(&mut self, expression: &Expression) -> LoweredStatement {
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
        let call = GoExpression::call(
            GoExpression::selector(handle.expression, "Log".to_string()),
            vec![
                GoExpression::literal(span.file_id.to_string()),
                GoExpression::literal(span.byte_offset.to_string()),
                GoExpression::literal((span.byte_offset + span.byte_length).to_string()),
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                    vec![value.expression],
                ),
            ],
        );
        (statements, call)
    }

    /// `assert a <op> b`: compare the operands via the normal binary lowering,
    /// reporting both as `left`/`right`.
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

    /// `assert recv.equals(arg)`: compare via the canonical equals lowering, reporting `left`/`right`.
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

    /// `assert <expr>`: any other boolean, tested through its negation.
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
            let (setup, condition) = self.lower_condition(operand);
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

    /// A name reads the same twice unless the other operand calls something.
    fn stage_assert_operands(
        &mut self,
        left: &Expression,
        right: &Expression,
        literals: LiteralInlining,
        statements: &mut Vec<LoweredStatement>,
    ) -> (AssertOperand, AssertOperand) {
        let left_plan = self.lower_value(left, ExpressionContext::value());
        let right_plan = self.lower_value(right, ExpressionContext::value());
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

    /// The Go type to declare an operand's temp with, or `None` to render the
    /// operand twice instead.
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
            && match plan.evaluation.form {
                OperandForm::Name => names_inline,
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
        // Bind the temp to itself so the relation shape's synthetic identifier resolves.
        self.scope.bind(name.clone(), name.clone());
        // Under `:=` a Go constant may take its own default type, so a large
        // `uint64` literal would come back as an overflowing `int`.
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

    /// A `recv.equals(arg)` whose receiver has an `equals` the compiler can lower
    /// (a slice, a map, or any type with a usable `equals` method), so the failure
    /// can show both operands. Anything else falls back to the bare shape.
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

    /// Lower `while let P = scrutinee { body }`, wrapped as a `WhileLet`
    /// statement.
    fn lower_while_let_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } = expression
        else {
            unreachable!("lower_while_let_statement requires a WhileLet expression");
        };
        let directive = self.maybe_line_directive(&expression.get_span());
        let body = self.with_loop("_", |this| this.lower_while_let(pattern, scrutinee, body));
        directed(directive, LoweredStatement::WhileLet(body))
    }

    fn lower_infinite_loop(&mut self, body: &Expression) -> LoopPlan {
        self.with_loop("_", |this| {
            this.lower_loop_with_header(LoopHeader::Infinite, body)
        })
    }

    fn lower_condition(&mut self, condition: &Expression) -> (Vec<LoweredStatement>, GoExpression) {
        let plan = self.plan_operand(condition, ExpressionContext::value());
        plan.into_parts()
    }

    fn lower_if_condition(
        &mut self,
        condition: &Expression,
    ) -> (Vec<LoweredStatement>, PairCondition) {
        if let Some(fused) = self.lower_fused_predicate_condition(condition) {
            return fused;
        }
        let (setup, rendered) = self.lower_condition(condition);
        (
            setup,
            PairCondition {
                initializer: None,
                condition: rendered,
            },
        )
    }

    fn lower_while(&mut self, condition: &Expression, body: &Expression) -> LoopPlan {
        self.with_loop("_", |this| {
            let (target, negated) = strip_negations(condition);
            if let Some(exit) = this.lower_fused_predicate_value(target, !negated) {
                let (setup, failure) = exit.into_parts();
                return this.lower_loop_with_exit_test(setup, failure, body);
            }
            let (setup, rendered) = this.lower_condition(condition);
            if !setup.is_empty() {
                let exit = GoExpression::unary("!", rendered);
                return this.lower_loop_with_exit_test(setup, exit, body);
            }
            let header = if matches!(
                condition.unwrap_parens(),
                Expression::Literal {
                    literal: Literal::Boolean(true),
                    ..
                }
            ) {
                LoopHeader::Infinite
            } else {
                LoopHeader::While(rendered)
            };
            this.lower_loop_with_header(header, body)
        })
    }

    fn lower_loop_with_exit_test(
        &mut self,
        setup: Vec<LoweredStatement>,
        exit: GoExpression,
        body: &Expression,
    ) -> LoopPlan {
        let mut statements = setup;
        statements.push(LoweredStatement::If(IfPlan::plain(
            exit,
            LoweredBlock {
                statements: vec![LoweredStatement::Break(LoopTransfer::Unlabeled)],
            },
            ElseArm::None,
        )));
        let lowered_body = self.with_scope(|this| this.lower_block_as_body(body));
        statements.extend(lowered_body.statements);
        self.build_source_loop(
            Vec::new(),
            LoopHeader::Infinite,
            LoweredBlock { statements },
        )
    }

    /// Shared loop lowering once the header is known. The caller must have an
    /// active loop context.
    pub(crate) fn lower_loop_with_header(
        &mut self,
        header: LoopHeader,
        body: &Expression,
    ) -> LoopPlan {
        let lowered_body = self.with_scope(|this| this.lower_block_as_body(body));
        self.build_source_loop(Vec::new(), header, lowered_body)
    }

    pub(crate) fn build_source_loop(
        &mut self,
        prologue: Vec<LoweredStatement>,
        header: LoopHeader,
        mut body: LoweredBlock,
    ) -> LoopPlan {
        let target = self
            .current_loop_id()
            .expect("source loop plan requires an active loop context");
        let label = legalize_source_loop(&mut body, target);
        LoopPlan {
            prologue,
            kind: LoopKind::Source { label },
            header,
            body,
        }
    }

    /// Lower a branch arm body in statement position (`PlacePlan::Statement`).
    pub(crate) fn lower_block_as_body(&mut self, expression: &Expression) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };
        let statements = items
            .iter()
            .map(|item| self.lower_statement(item))
            .collect();
        LoweredBlock { statements }
    }

    /// Lower a branching expression (`if`, `if let`, `match`, `select`) into a
    /// `LoweredBlock` targeting `place`. Centralises the dispatch shared by old
    /// emit paths that need to render a branching tail/assignment.
    pub(crate) fn lower_branching_to_block(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let plan = self.lower_if(condition, consequence, alternative.as_deref(), place);
                LoweredBlock {
                    statements: vec![LoweredStatement::If(plan)],
                }
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => {
                let arms = if_let_match_arms(pattern, consequence, alternative, *span);
                self.lower_match_to_block(scrutinee, &arms, place)
            }
            Expression::Match { subject, arms, .. } => {
                self.lower_match_to_block(subject, arms, place)
            }
            Expression::Select { arms, .. } => LoweredBlock {
                statements: vec![LoweredStatement::Select(self.lower_select(arms, place))],
            },
            _ => unreachable!("lower_branching_to_block: expected if/if-let/match/select"),
        }
    }

    /// Lower a branch arm body into the given place.
    pub(crate) fn lower_block_to_place(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match place {
            PlacePlan::Statement => self.lower_block_as_body(expression),
            PlacePlan::Return => self.lower_block_to_return(expression),
            PlacePlan::Assign { local, target_ty } => {
                self.lower_block_to_assign(expression, local, *target_ty)
            }
        }
    }

    /// Lower a branch arm body in assign position. Fallible (`Result`/`Option`)
    /// targets route through `lower_option_result_assignment`; everything else
    /// flows through the shared `lower_block_to_var`.
    fn lower_block_to_assign(
        &mut self,
        expression: &Expression,
        local: &GoExpression,
        target_ty: Option<&Type>,
    ) -> LoweredBlock {
        if expression.get_type().is_result() || expression.get_type().is_option() {
            return LoweredBlock {
                statements: self.lower_option_result_assignment(local, target_ty, expression),
            };
        }
        LoweredBlock {
            statements: self.lower_block_to_var(expression, local, target_ty, false),
        }
    }

    /// Lower a block in return position: non-tail items become statements,
    /// the tail returns. A tail `let` (`let x = if ...; x`) is elided into the
    /// surrounding return place; function bodies skip that elision.
    fn lower_block_to_return(&mut self, expression: &Expression) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };

        let Some((last, rest)) = try_elide_tail_let(items).or_else(|| items.split_last()) else {
            return LoweredBlock {
                statements: Vec::new(),
            };
        };

        let (statements, _) = self.lower_body_until_diverge(rest, last);
        LoweredBlock { statements }
    }

    /// Lower a single tail expression in return position to its return
    /// statements. Shared by branch-arm return lowering and function-body
    /// lowering; leaf values and lowered-ABI returns become `Return` leaves,
    /// `if`/`if let`/`match`/`select` tails recurse structurally with a `Return` place.
    fn lower_return_tail(&mut self, last: &Expression) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        let return_span = last.get_span();
        let last = if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        };

        if last.get_type().is_unit() {
            if !matches!(last, Expression::Unit { .. }) {
                statements.push(self.lower_statement(last));
            }
            return statements;
        }

        if last.get_type().is_never() {
            return self.lower_never_return_tail(last, &return_span);
        }

        let directive = self.maybe_line_directive(&return_span);
        match last {
            Expression::If { .. } | Expression::Select { .. } => {
                let mut block = self.lower_branching_to_block(last, &PlacePlan::Return);
                let statement = block
                    .statements
                    .pop()
                    .expect("if and select lower to one statement");
                statements.push(directed(directive, statement));
            }
            Expression::IfLet { .. } | Expression::Match { .. } => {
                let block = self.lower_branching_to_block(last, &PlacePlan::Return);
                statements.extend(directed_first(directive, block.statements));
            }
            _ => {
                let tail = if let Some(tail) = try_emit_lowered_tail_return(self, last) {
                    tail
                } else if let Some(wrapped) = self.lower_wrapped_return(last) {
                    wrapped
                } else {
                    self.lower_plain_return_tail(last)
                };
                statements.extend(directed_first(directive, tail));
            }
        }

        statements
    }

    fn lower_never_return_tail(
        &mut self,
        last: &Expression,
        return_span: &Span,
    ) -> Vec<LoweredStatement> {
        let directive = self.maybe_line_directive(return_span);
        let mut statements = vec![self.lower_statement(last)];
        if !is_go_never(last) && !is_breakless_loop(last) {
            statements.push(LoweredStatement::UnreachablePanic);
        }
        directed_first(directive, statements)
    }

    fn lower_plain_return_tail(&mut self, last: &Expression) -> Vec<LoweredStatement> {
        if requires_temp_var(last) {
            let staged = self.plan_operand(last, ExpressionContext::value());
            let (mut statements, value) = staged.into_parts();
            if !value.is_empty() {
                statements.push(plain_return(value));
            }
            statements
        } else {
            let (mut statements, expression) = self.lower_tail_value(last);
            let return_ctx = self.return_ctx();
            let expression =
                self.apply_type_coercion(&mut statements, return_ctx.ty(), last, expression);
            statements.push(plain_return(expression));
            statements
        }
    }

    fn lower_if(
        &mut self,
        condition: &Expression,
        consequence: &Expression,
        alternative: Option<&Expression>,
        place: &PlacePlan,
    ) -> IfPlan {
        let (condition_setup, condition) = self.lower_if_condition(condition);

        let then_body = self.with_scope(|this| this.lower_block_to_place(consequence, place));

        let preceding_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_chain(alternative, preceding_diverges, place);

        IfPlan {
            condition_setup,
            initializer: condition.initializer,
            condition: condition.condition,
            then_body,
            else_arm,
        }
    }

    fn lower_else_chain(
        &mut self,
        alternative: Option<&Expression>,
        preceding_diverges: bool,
        place: &PlacePlan,
    ) -> ElseArm {
        let Some(alternative) = alternative else {
            return ElseArm::None;
        };

        if let Expression::If {
            condition,
            consequence,
            alternative: next_alternative,
            ..
        } = alternative
        {
            let (condition_setup, condition) = self.lower_if_condition(condition);

            // With-setup else-if renders as a nested block (`} else { setup; if
            // ... }`), so its body sits in an inner scope inside an outer scope
            // that also wraps the recursion. Plain else-if uses a single scope
            // around the body and recurses outside it.
            if !condition_setup.is_empty() {
                self.with_scope(|this| {
                    let then_body =
                        this.with_scope(|this| this.lower_block_to_place(consequence, place));
                    let inner = this.lower_else_chain(
                        next_alternative.as_deref(),
                        then_body.ends_with_diverge(),
                        place,
                    );
                    ElseArm::ElseIf(Box::new(IfPlan {
                        condition_setup,
                        initializer: condition.initializer,
                        condition: condition.condition,
                        then_body,
                        else_arm: inner,
                    }))
                })
            } else {
                let then_body =
                    self.with_scope(|this| this.lower_block_to_place(consequence, place));
                let inner = self.lower_else_chain(
                    next_alternative.as_deref(),
                    then_body.ends_with_diverge(),
                    place,
                );
                ElseArm::ElseIf(Box::new(IfPlan {
                    condition_setup,
                    initializer: condition.initializer,
                    condition: condition.condition,
                    then_body,
                    else_arm: inner,
                }))
            }
        } else if preceding_diverges {
            let body =
                self.with_binding_frame(|this| this.lower_block_to_place(alternative, place));
            ElseArm::from_body(body, true)
        } else {
            let body = self.with_scope(|this| this.lower_block_to_place(alternative, place));
            ElseArm::from_body(body, false)
        }
    }
}

/// The lowered pieces of an `assert`: the condition under which it fails, the
/// record kind, and any `Operand{...}` arguments appended to the failure call.
struct AssertShape {
    failure_condition: GoExpression,
    kind: &'static str,
    message: String,
    operands: Vec<GoExpression>,
}

/// An `assert` operand: the expression the test reads, and the Go value the
/// failure call reports.
struct AssertOperand {
    expression: Expression,
    rendered: GoExpression,
}

/// The equals lowering can read its left operand as a method receiver, where a
/// bare literal is not valid Go.
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

/// A typed identifier for an already-bound temp, so the rebuilt comparison casts as usual.
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
