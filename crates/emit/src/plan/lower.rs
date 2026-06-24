use crate::Planner;
use crate::Renderer;
use crate::abi::transition::try_emit_lowered_tail_return;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::definitions::functions::{is_breakless_loop, is_go_never, is_test_context_ty};
use crate::names::go_name::{prelude_qualifier, testkit_qualifier};
use crate::plan::bodies::{
    ElseArm, ExpressionStatementForm, ExpressionStatementPlan, IfPlan, LoopPlan, LoweredBlock,
    LoweredStatement, MatchStatementPlan, PlacePlan, WhileLetPlan, directed,
};
use crate::plan::placement::{requires_temp_var, try_elide_tail_let};
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use crate::utils::wrap_if_struct_literal;
use syntax::ast::{BinaryOperator, Expression, Literal, MatchArm, Pattern, TypedPattern};
use syntax::types::Type;

fn if_let_match_arms(
    pattern: &Pattern,
    typed_pattern: &Option<TypedPattern>,
    consequence: &Expression,
    alternative: &Expression,
) -> Vec<MatchArm> {
    vec![
        MatchArm {
            pattern: pattern.clone(),
            guard: None,
            typed_pattern: typed_pattern.clone(),
            expression: Box::new(consequence.clone()),
        },
        MatchArm {
            pattern: Pattern::WildCard {
                span: alternative.get_span(),
            },
            guard: None,
            typed_pattern: None,
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
            go_type: self.go_type_string(ty),
            value: None,
        };
        self.declare(&result_var);
        (result_var, declaration)
    }

    /// Plan a value-position `if` as a fresh operand-temp variable: a `var V T`
    /// declaration leaf plus a typed `If` statement that assigns `V`. Returns
    /// the setup statements and `V`.
    pub(crate) fn plan_if_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let Expression::If {
            condition,
            consequence,
            alternative,
            ..
        } = expression
        else {
            unreachable!("plan_if_as_operand_temp called on non-If expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let plan = self.lower_if(
            condition,
            consequence,
            alternative,
            &PlacePlan::Assign {
                local: &result_var,
                target_ty: Some(ty),
            },
        );
        value_plan_from_statements(vec![declaration, LoweredStatement::If(plan)], result_var)
    }

    /// Lower a value-position `if let`/`match`/`select` to a fresh operand-temp
    /// variable. Only valid for non-never result types; never-typed branches
    /// route through `lower_to_operand_temp` as a diverging statement.
    pub(crate) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let block = self.lower_branching_to_block(
            expression,
            &PlacePlan::Assign {
                local: &result_var,
                target_ty: Some(ty),
            },
        );
        let mut setup = vec![declaration];
        setup.extend(block.statements);
        value_plan_from_statements(setup, result_var)
    }

    /// Lower a value-position `loop` to a fresh operand-temp variable.
    /// Declares `var V T`, pushes `V` as the current loop result slot so
    /// `break value` assigns into it, lowers the loop, then renders.
    pub(crate) fn plan_loop_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        let Expression::Loop {
            body, needs_label, ..
        } = expression
        else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        self.push_loop(result_var.clone());
        let plan = self.lower_loop_with_header("for {\n".to_string(), body, *needs_label);
        self.pop_loop();
        value_plan_from_statements(vec![declaration, LoweredStatement::Loop(plan)], result_var)
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
            std::slice::from_ref(body)
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
            let (zero, effects) = self.zero_value(&return_ty);
            self.absorb_effects(&effects);
            statements.push(plain_return(zero));
        }

        LoweredBlock { statements }
    }

    /// Lower a single statement. Structured variants are produced where lowering
    /// has reached the construct; everything else captures the existing emitter
    /// output as `RawGo`. `return_ctx` is the enclosing function/lambda/try/
    /// recover return context, threaded so nested `return` lowering has an
    /// explicit context.
    pub(crate) fn lower_statement(&mut self, expression: &Expression) -> LoweredStatement {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan =
                    self.lower_if(condition, consequence, alternative, &PlacePlan::Statement);
                directed(directive, LoweredStatement::If(plan))
            }
            Expression::Loop {
                body, needs_label, ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                directed(
                    directive,
                    LoweredStatement::Loop(self.lower_infinite_loop(body, *needs_label)),
                )
            }
            Expression::While {
                condition,
                body,
                needs_label,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                directed(
                    directive,
                    LoweredStatement::Loop(self.lower_while(condition, body, *needs_label)),
                )
            }
            Expression::Block { .. } => {
                self.enter_scope();
                let body = self.lower_block_as_body(expression);
                self.exit_scope();
                LoweredStatement::Block(body)
            }
            Expression::For { .. } => self.lower_for_statement(expression),
            Expression::Continue { .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                directed(
                    directive,
                    LoweredStatement::Continue {
                        label: self.current_loop_label().map(str::to_string),
                    },
                )
            }
            Expression::Break { value: None, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                directed(
                    directive,
                    LoweredStatement::Break {
                        label: self.current_loop_label().map(str::to_string),
                    },
                )
            }
            Expression::Break {
                value: Some(value), ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_break_value_plan(value);
                directed(directive, LoweredStatement::BreakValue(plan))
            }
            Expression::Const {
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_const_plan(identifier, value, ty);
                directed(directive, LoweredStatement::Const(plan))
            }
            Expression::Return {
                expression: value, ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_return_plan(value);
                directed(directive, LoweredStatement::Return(plan))
            }
            Expression::Let {
                binding,
                value,
                mutable,
                else_block,
                assert,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan =
                    self.build_let_plan(binding, value, else_block.as_deref(), *mutable, *assert);
                directed(directive, LoweredStatement::Let(plan))
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_assignment_plan(target, value, compound_operator.as_ref());
                directed(directive, LoweredStatement::Assign(plan))
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                typed_pattern,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let arms = if_let_match_arms(pattern, typed_pattern, consequence, alternative);
                let body = self.lower_match_to_block(scrutinee, &arms, &PlacePlan::Statement);
                directed(
                    directive,
                    LoweredStatement::Match(MatchStatementPlan { body }),
                )
            }
            Expression::Match { subject, arms, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let body = self.lower_match_to_block(subject, arms, &PlacePlan::Statement);
                directed(
                    directive,
                    LoweredStatement::Match(MatchStatementPlan { body }),
                )
            }
            Expression::Select { arms, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.lower_select(arms, &PlacePlan::Statement);
                directed(directive, LoweredStatement::Select(plan))
            }
            Expression::WhileLet { .. } => self.lower_while_let_statement(expression),
            Expression::Assert { .. } => self.lower_assert_statement(expression),
            // Top-level items (Struct/Enum/etc) shouldn't appear inside
            // function bodies, but dispatch handles them defensively. They
            // carry their own directive (via `emit_top_item`) so the wrapper
            // does not add one.
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::ImplBlock { .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let code = self.emit_top_item(expression);
                let mut buffer = directive;
                if !code.is_empty() {
                    buffer.push_str(&code);
                    buffer.push('\n');
                }
                LoweredStatement::RawGo(buffer)
            }
            Expression::Call { .. } if self.is_test_log_call(expression) => {
                self.lower_test_log_statement(expression)
            }
            _ => self.lower_expression_statement(expression),
        }
    }

    /// Lower the statement-position fall-through: Task/Defer (async value),
    /// `expr?` propagation, or an otherwise-discarded expression value. The
    /// directive rides on the wrapper so the rendered body stays directive-free.
    fn lower_expression_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let unwrapped = expression.unwrap_parens();
        let directive = self.maybe_line_directive(&expression.get_span());
        let form = if matches!(
            unwrapped,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(unwrapped, ExpressionContext::value());
            ExpressionStatementForm::Async { value }
        } else if let Expression::Propagate {
            expression: inner, ..
        } = unwrapped
        {
            ExpressionStatementForm::Propagate {
                body: LoweredBlock {
                    statements: self.lower_propagate_statement(inner),
                },
            }
        } else {
            ExpressionStatementForm::Discard {
                body: LoweredBlock {
                    statements: self.lower_discard_value(unwrapped),
                },
            }
        };
        directed(
            directive,
            LoweredStatement::Expression(ExpressionStatementPlan { form }),
        )
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

        // Each shape stages its operands into `statements` and returns the
        // condition, the record kind, and any operand arguments for the call.
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
            condition,
            kind,
            message,
            operands,
        } = shape;
        let handle = self
            .current_test_handle()
            .expect("assert without a test handle should be rejected by semantics");
        let span = operand.get_span();
        statements.push(LoweredStatement::RawGo(format!(
            "if !({condition}) {{\n{handle}.FailAssert({}, {}, {}, \"{kind}\", \"{message}\"{operands})\n}}\n",
            span.file_id,
            span.byte_offset,
            span.byte_offset + span.byte_length,
        )));
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
        statements.push(LoweredStatement::RawGo(format!("{call}\n")));
        LoweredStatement::Block(LoweredBlock { statements })
    }

    pub(crate) fn lower_test_log_call(
        &mut self,
        expression: &Expression,
    ) -> (Vec<LoweredStatement>, String) {
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
        self.require_testkit();
        self.require_stdlib();

        let mut statements = Vec::new();
        let (recv_setup, handle) = self
            .lower_value(receiver, ExpressionContext::value())
            .into_parts();
        statements.extend(recv_setup);
        let (arg_setup, value) = self
            .lower_value(&args[0], ExpressionContext::value())
            .into_parts();
        statements.extend(arg_setup);

        let prelude = prelude_qualifier();
        let span = args[0].get_span();
        let call = format!(
            "{handle}.Log({}, {}, {}, {prelude}.Debug({value}))",
            span.file_id,
            span.byte_offset,
            span.byte_offset + span.byte_length,
        );
        (statements, call)
    }

    /// `assert a <op> b`: compare the captured temps via the normal binary
    /// lowering, reporting both as `left`/`right`.
    fn lower_relation_assert(
        &mut self,
        operator: &BinaryOperator,
        left: &Expression,
        right: &Expression,
        statements: &mut Vec<LoweredStatement>,
    ) -> AssertShape {
        self.require_stdlib();
        let lhs = self.stage_assert_operand(left, "assertLeft", statements);
        let rhs = self.stage_assert_operand(right, "assertRight", statements);
        let left_ref = temp_identifier(&lhs, left);
        let right_ref = temp_identifier(&rhs, right);
        let (cond_setup, condition) = self
            .plan_binary(operator, &left_ref, &right_ref, ExpressionContext::value())
            .into_parts();
        statements.extend(cond_setup);
        AssertShape {
            condition,
            kind: "relation",
            message: format!("expected {operator}"),
            operands: paired_operands(&lhs, &rhs),
        }
    }

    /// `assert recv.equals(arg)`: compare via the canonical equals lowering, reporting `left`/`right`.
    fn lower_labeled_assert(
        &mut self,
        recv: &Expression,
        arg: &Expression,
        statements: &mut Vec<LoweredStatement>,
    ) -> AssertShape {
        self.require_stdlib();
        let recv_ty = recv.get_type();
        let lhs = self.stage_assert_operand(recv, "assertLeft", statements);
        let rhs = self.stage_assert_operand(arg, "assertRight", statements);
        let condition = self.render_equality(&lhs, &rhs, &recv_ty);
        AssertShape {
            condition,
            kind: "labeled",
            message: "expected ==".to_string(),
            operands: paired_operands(&lhs, &rhs),
        }
    }

    /// `assert <expr>`: any other boolean, lowered as-is (no decomposition).
    fn lower_bare_assert(
        &mut self,
        operand: &Expression,
        statements: &mut Vec<LoweredStatement>,
    ) -> AssertShape {
        let (setup, condition) = self.lower_condition(operand);
        statements.extend(setup);
        AssertShape {
            condition,
            kind: "bare",
            message: "assertion failed".to_string(),
            operands: String::new(),
        }
    }

    /// Capture an `assert` operand into a fresh temp, declared with its inferred
    /// type so an untyped constant (e.g. a large `uint64` literal) keeps its type.
    fn stage_assert_operand(
        &mut self,
        expression: &Expression,
        hint: &str,
        statements: &mut Vec<LoweredStatement>,
    ) -> String {
        let (setup, value) = self
            .lower_value(expression, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        let name = self.fresh_var(Some(hint));
        self.declare(&name);
        // Bind the temp to itself so the relation shape's synthetic identifier resolves.
        self.scope.bind(name.clone(), name.clone());
        let go_type = self.go_type_string(&expression.get_type());
        statements.push(LoweredStatement::VarDecl {
            name: name.clone(),
            go_type,
            value: Some(value),
        });
        name
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
        (recv_ty.is_slice() || recv_ty.is_map() || self.type_has_equals(&recv_ty))
            .then(|| (recv.unwrap_parens(), &args[0]))
    }

    /// Lower `while let P = scrutinee { body }`, wrapped as a `WhileLet`
    /// statement.
    fn lower_while_let_statement(&mut self, expression: &Expression) -> LoweredStatement {
        let Expression::WhileLet {
            pattern,
            typed_pattern,
            scrutinee,
            body,
            needs_label,
            ..
        } = expression
        else {
            unreachable!("lower_while_let_statement requires a WhileLet expression");
        };
        let directive = self.maybe_line_directive(&expression.get_span());
        self.push_loop("_");
        let body = self.lower_while_let(
            pattern,
            typed_pattern.as_ref(),
            scrutinee,
            body,
            *needs_label,
        );
        self.pop_loop();
        directed(directive, LoweredStatement::WhileLet(WhileLetPlan { body }))
    }

    fn lower_infinite_loop(&mut self, body: &Expression, needs_label: bool) -> LoopPlan {
        self.push_loop("_");
        let plan = self.lower_loop_with_header("for {\n".to_string(), body, needs_label);
        self.pop_loop();
        plan
    }

    fn lower_condition(&mut self, condition: &Expression) -> (Vec<LoweredStatement>, String) {
        let plan = self.plan_operand(condition, ExpressionContext::value().condition());
        plan.into_parts()
    }

    fn lower_while(
        &mut self,
        condition: &Expression,
        body: &Expression,
        needs_label: bool,
    ) -> LoopPlan {
        self.push_loop("_");
        let (setup, rendered) = self.lower_condition(condition);
        let header = if !setup.is_empty() {
            // Condition produced setup statements (temps); they must re-run each
            // iteration, so move everything inside the loop.
            let setup_text = Renderer.render_setup(&setup);
            format!("for {{\n{}if !({}) {{ break }}\n", setup_text, rendered)
        } else if matches!(
            condition.unwrap_parens(),
            Expression::Literal {
                literal: Literal::Boolean(true),
                ..
            }
        ) {
            "for {\n".to_string()
        } else {
            format!("for {} {{\n", wrap_if_struct_literal(rendered))
        };
        let plan = self.lower_loop_with_header(header, body, needs_label);
        self.pop_loop();
        plan
    }

    /// Shared loop lowering once the header is known: set the label, lower
    /// the body in a fresh scope. Caller owns `push_loop`/`pop_loop`.
    pub(crate) fn lower_loop_with_header(
        &mut self,
        header: String,
        body: &Expression,
        needs_label: bool,
    ) -> LoopPlan {
        self.set_current_loop_label_if_needed(needs_label);
        let label = self.current_loop_label().map(str::to_string);
        self.enter_scope();
        let lowered_body = self.lower_block_as_body(body);
        self.exit_scope();
        LoopPlan {
            prologue: Vec::new(),
            label,
            header,
            body: lowered_body,
        }
    }

    /// Lower a branch arm body in statement position (`PlacePlan::Statement`).
    pub(crate) fn lower_block_as_body(&mut self, expression: &Expression) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
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
                let plan = self.lower_if(condition, consequence, alternative, place);
                LoweredBlock {
                    statements: vec![LoweredStatement::If(plan)],
                }
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                typed_pattern,
                ..
            } => {
                let arms = if_let_match_arms(pattern, typed_pattern, consequence, alternative);
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
        local: &str,
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
            std::slice::from_ref(expression)
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
    /// lowering; only leaf values and lowered-ABI returns become `RawGo`,
    /// `if`/`if let`/`match`/`select` tails recurse structurally with a `Return` place.
    pub(crate) fn lower_return_tail(&mut self, last: &Expression) -> Vec<LoweredStatement> {
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
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let plan = self.lower_if(condition, consequence, alternative, &PlacePlan::Return);
                statements.push(directed(directive, LoweredStatement::If(plan)));
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                typed_pattern,
                ..
            } => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                let arms = if_let_match_arms(pattern, typed_pattern, consequence, alternative);
                let block = self.lower_match_to_block(scrutinee, &arms, &PlacePlan::Return);
                statements.extend(block.statements);
            }
            Expression::Match { subject, arms, .. } => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                let block = self.lower_match_to_block(subject, arms, &PlacePlan::Return);
                statements.extend(block.statements);
            }
            Expression::Select { arms, .. } => {
                let plan = self.lower_select(arms, &PlacePlan::Return);
                statements.push(directed(directive, LoweredStatement::Select(plan)));
            }
            _ => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                if let Some(tail) = try_emit_lowered_tail_return(self, last) {
                    statements.extend(tail);
                } else if let Some(wrapped) = self.lower_wrapped_return(last) {
                    statements.extend(wrapped);
                } else {
                    statements.extend(self.lower_plain_return_tail(last));
                }
            }
        }

        statements
    }

    fn lower_never_return_tail(
        &mut self,
        last: &Expression,
        return_span: &syntax::ast::Span,
    ) -> Vec<LoweredStatement> {
        let directive = self.maybe_line_directive(return_span);
        let mut statements: Vec<LoweredStatement> = Vec::new();
        if !directive.is_empty() {
            statements.push(LoweredStatement::RawGo(directive));
        }
        statements.push(self.lower_statement(last));
        if !is_go_never(last) && !is_breakless_loop(last) {
            statements.push(LoweredStatement::UnreachablePanic);
        }
        statements
    }

    /// Kept as `RawGo`, not `ReturnForm::Plain`: a structured `Return` would
    /// flatten the enclosing `else` for a multi-line return value.
    fn lower_plain_return_tail(&mut self, last: &Expression) -> Vec<LoweredStatement> {
        if requires_temp_var(last) {
            let staged = self.stage_operand(last, ExpressionContext::value());
            let mut statements = staged.setup;
            if !staged.value.is_empty() {
                statements.push(plain_return(staged.value));
            }
            statements
        } else {
            let (mut statements, expression_string) = self.lower_tail_value(last);
            let return_ctx = self.return_ctx();
            let mut coercion = String::new();
            let expression_string =
                self.apply_type_coercion(&mut coercion, return_ctx.ty(), last, expression_string);
            if !coercion.is_empty() {
                statements.push(LoweredStatement::RawGo(coercion));
            }
            statements.push(plain_return(expression_string));
            statements
        }
    }

    pub(crate) fn lower_if(
        &mut self,
        condition: &Expression,
        consequence: &Expression,
        alternative: &Expression,
        place: &PlacePlan,
    ) -> IfPlan {
        let (condition_setup, condition_string) = self.lower_condition(condition);
        let condition = wrap_if_struct_literal(condition_string);

        self.enter_scope();
        let then_body = self.lower_block_to_place(consequence, place);
        self.exit_scope();

        let preceding_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_chain(alternative, preceding_diverges, place);

        IfPlan {
            condition_setup,
            condition,
            then_body,
            else_arm,
        }
    }

    fn lower_else_chain(
        &mut self,
        alternative: &Expression,
        preceding_diverges: bool,
        place: &PlacePlan,
    ) -> ElseArm {
        let is_empty_alternative = match alternative {
            Expression::Unit { .. } => true,
            Expression::Block { items, .. } => items.is_empty(),
            _ => false,
        };
        if is_empty_alternative {
            return ElseArm::None;
        }

        if let Expression::If {
            condition,
            consequence,
            alternative: next_alternative,
            ..
        } = alternative
        {
            let (condition_setup, condition_string) = self.lower_condition(condition);
            let condition = wrap_if_struct_literal(condition_string);

            // With-setup else-if renders as a nested block (`} else { setup; if
            // ... }`), so its body sits in an inner scope inside an outer scope
            // that also wraps the recursion. Plain else-if uses a single scope
            // around the body and recurses outside it.
            if !condition_setup.is_empty() {
                self.enter_scope();
                self.enter_scope();
                let then_body = self.lower_block_to_place(consequence, place);
                self.exit_scope();
                let inner =
                    self.lower_else_chain(next_alternative, then_body.ends_with_diverge(), place);
                self.exit_scope();
                ElseArm::ElseIf(Box::new(IfPlan {
                    condition_setup,
                    condition,
                    then_body,
                    else_arm: inner,
                }))
            } else {
                self.enter_scope();
                let then_body = self.lower_block_to_place(consequence, place);
                self.exit_scope();
                let inner =
                    self.lower_else_chain(next_alternative, then_body.ends_with_diverge(), place);
                ElseArm::ElseIf(Box::new(IfPlan {
                    condition_setup,
                    condition,
                    then_body,
                    else_arm: inner,
                }))
            }
        } else if preceding_diverges {
            let body = self.lower_block_to_place(alternative, place);
            ElseArm::Else { body, inline: true }
        } else {
            self.enter_scope();
            let body = self.lower_block_to_place(alternative, place);
            self.exit_scope();
            ElseArm::Else {
                body,
                inline: false,
            }
        }
    }
}

/// The lowered pieces of an `assert`: the boolean condition, the record kind,
/// and any `, Operand{...}` arguments appended to the failure call.
struct AssertShape {
    condition: String,
    kind: &'static str,
    message: String,
    operands: String,
}

fn paired_operands(lhs: &str, rhs: &str) -> String {
    let (test_kit, prelude) = (testkit_qualifier(), prelude_qualifier());
    format!(
        ", {test_kit}.Operand{{Label: \"left\", Value: {prelude}.Debug({lhs})}}, {test_kit}.Operand{{Label: \"right\", Value: {prelude}.Debug({rhs})}}"
    )
}

/// A typed identifier for an already-bound temp, so the rebuilt comparison casts as usual.
fn temp_identifier(name: &str, original: &Expression) -> Expression {
    Expression::Identifier {
        value: name.into(),
        ty: original.get_type(),
        span: original.get_span(),
        binding_id: None,
        qualified: None,
    }
}

fn is_assert_relation(operator: &BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    )
}
