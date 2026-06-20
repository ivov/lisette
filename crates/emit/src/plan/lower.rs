use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::abi::transition::try_emit_lowered_tail_return;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::definitions::functions::{is_breakless_loop, is_go_never};
use crate::names::go_name;
use crate::plan::bodies::{
    ElseArm, ExpressionStatementForm, ExpressionStatementPlan, IfPlan, LoopPlan, LoweredBlock,
    LoweredStatement, MatchStatementPlan, PlacePlan, WhileLetPlan, directed,
};
use crate::plan::placement::{requires_temp_var, try_elide_tail_let};
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use crate::utils::wrap_if_struct_literal;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::types::Type;

impl Planner<'_> {
    /// Allocate a fresh operand-temp result var and its `var V T` declaration
    /// as a typed setup leaf. The control-flow that assigns it follows as a
    /// typed `If`/`Loop`/`Match`/`Select` statement.
    pub(crate) fn operand_temp_declaration(
        &mut self,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> (String, LoweredStatement) {
        let result_var = self.fresh_var(None);
        let declaration = LoweredStatement::VarDecl {
            name: result_var.clone(),
            go_type: self.go_type_string(ty, fx),
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
        fx: &mut EmitEffects,
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
        let (result_var, declaration) = self.operand_temp_declaration(ty, fx);
        let plan = self.lower_if(
            condition,
            consequence,
            alternative,
            &PlacePlan::Assign {
                local: &result_var,
                target_ty: Some(ty),
            },
            fx,
        );
        value_plan_from_statements(vec![declaration, LoweredStatement::If(plan)], result_var)
    }

    /// Lower a value-position `match`/`select` to a fresh operand-temp
    /// variable. Only valid for non-never result types; never-typed branches
    /// route through `lower_to_operand_temp` as a diverging statement.
    pub(crate) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let (result_var, declaration) = self.operand_temp_declaration(ty, fx);
        let block = self.lower_branching_to_block(
            expression,
            &PlacePlan::Assign {
                local: &result_var,
                target_ty: Some(ty),
            },
            fx,
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
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let Expression::Loop {
            body, needs_label, ..
        } = expression
        else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty, fx);
        self.push_loop(result_var.clone());
        let plan = self.lower_loop_with_header("for {\n".to_string(), body, *needs_label, fx);
        self.pop_loop();
        value_plan_from_statements(vec![declaration, LoweredStatement::Loop(plan)], result_var)
    }

    fn lower_body_until_diverge(
        &mut self,
        rest: &[Expression],
        last: &Expression,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, bool) {
        let mut statements: Vec<LoweredStatement> = Vec::with_capacity(rest.len() + 1);
        for item in rest {
            let statement = self.lower_statement(item, fx);
            let diverged = statement.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                return (statements, true);
            }
        }
        statements.extend(self.lower_return_tail(last, fx));
        (statements, false)
    }

    pub(crate) fn lower_function_body(
        &mut self,
        body: &Expression,
        should_return: bool,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        if !should_return {
            return self.lower_block_as_body(body, fx);
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

        let (mut statements, diverged) = self.lower_body_until_diverge(rest, last, fx);
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
            fx.extend(&effects);
            statements.push(plain_return(zero));
        }

        LoweredBlock { statements }
    }

    /// Lower a single statement. Structured variants are produced where lowering
    /// has reached the construct; everything else captures the existing emitter
    /// output as `RawGo`. `return_ctx` is the enclosing function/lambda/try/
    /// recover return context, threaded so nested `return` lowering has an
    /// explicit context.
    pub(crate) fn lower_statement(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.lower_if(
                    condition,
                    consequence,
                    alternative,
                    &PlacePlan::Statement,
                    fx,
                );
                directed(directive, LoweredStatement::If(plan))
            }
            Expression::Loop {
                body, needs_label, ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                directed(
                    directive,
                    LoweredStatement::Loop(self.lower_infinite_loop(body, *needs_label, fx)),
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
                    LoweredStatement::Loop(self.lower_while(condition, body, *needs_label, fx)),
                )
            }
            Expression::Block { .. } => {
                self.enter_scope();
                let body = self.lower_block_as_body(expression, fx);
                self.exit_scope();
                LoweredStatement::Block(body)
            }
            Expression::For { .. } => self.lower_for_statement(expression, fx),
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
                let plan = self.build_break_value_plan(value, fx);
                directed(directive, LoweredStatement::BreakValue(plan))
            }
            Expression::Const {
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_const_plan(identifier, value, ty, fx);
                directed(directive, LoweredStatement::Const(plan))
            }
            Expression::Return {
                expression: value, ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_return_plan(value, fx);
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
                let plan = self.build_let_plan(
                    binding,
                    value,
                    else_block.as_deref(),
                    *mutable,
                    *assert,
                    fx,
                );
                directed(directive, LoweredStatement::Let(plan))
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan =
                    self.build_assignment_plan(target, value, compound_operator.as_ref(), fx);
                directed(directive, LoweredStatement::Assign(plan))
            }
            Expression::Match { subject, arms, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let body = self.lower_match_to_block(subject, arms, &PlacePlan::Statement, fx);
                directed(
                    directive,
                    LoweredStatement::Match(MatchStatementPlan { body }),
                )
            }
            Expression::Select { arms, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.lower_select(arms, &PlacePlan::Statement, fx);
                directed(directive, LoweredStatement::Select(plan))
            }
            Expression::WhileLet { .. } => self.lower_while_let_statement(expression, fx),
            Expression::Assert { .. } => self.lower_assert_statement(expression, fx),
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
                let code = self.emit_top_item(expression, fx);
                let mut buffer = directive;
                if !code.is_empty() {
                    buffer.push_str(&code);
                    buffer.push('\n');
                }
                LoweredStatement::RawGo(buffer)
            }
            _ => self.lower_expression_statement(expression, fx),
        }
    }

    /// Lower the statement-position fall-through: Task/Defer (async value),
    /// `expr?` propagation, or an otherwise-discarded expression value. The
    /// directive rides on the wrapper so the rendered body stays directive-free.
    fn lower_expression_statement(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let unwrapped = expression.unwrap_parens();
        let directive = self.maybe_line_directive(&expression.get_span());
        let form = if matches!(
            unwrapped,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(unwrapped, ExpressionContext::value(), fx);
            ExpressionStatementForm::Async { value }
        } else if let Expression::Propagate {
            expression: inner, ..
        } = unwrapped
        {
            ExpressionStatementForm::Propagate {
                body: LoweredBlock {
                    statements: self.lower_propagate_statement(inner, fx),
                },
            }
        } else {
            ExpressionStatementForm::Discard {
                body: LoweredBlock {
                    statements: self.lower_discard_value(unwrapped, fx),
                },
            }
        };
        directed(
            directive,
            LoweredStatement::Expression(ExpressionStatementPlan { form }),
        )
    }

    pub(crate) fn lower_assert_statement(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let Expression::Assert {
            expression: operand,
            ..
        } = expression
        else {
            unreachable!("lower_assert_statement requires an Assert expression");
        };
        let operand = operand.unwrap_parens();
        fx.require_testkit();

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
            self.lower_relation_assert(operator, left, right, &mut statements, fx)
        } else if let Some((recv, arg)) = self.as_equals_decomposition(operand) {
            self.lower_labeled_assert(recv, arg, &mut statements, fx)
        } else {
            self.lower_bare_assert(operand, &mut statements, fx)
        };

        let AssertShape {
            condition,
            kind,
            operands,
        } = shape;
        let handle = self
            .current_test_handle()
            .expect("assert without a test handle should be rejected by semantics");
        let span = operand.get_span();
        statements.push(LoweredStatement::RawGo(format!(
            "if !({condition}) {{\n{handle}.FailAssert({}, {}, {}, \"{kind}\", \"assertion failed\"{operands})\n}}\n",
            span.file_id,
            span.byte_offset,
            span.byte_offset + span.byte_length,
        )));
        LoweredStatement::Block(LoweredBlock { statements })
    }

    /// `assert a <op> b`: compare the captured temps via the normal binary
    /// lowering, reporting both as `left`/`right`.
    fn lower_relation_assert(
        &mut self,
        operator: &BinaryOperator,
        left: &Expression,
        right: &Expression,
        statements: &mut Vec<LoweredStatement>,
        fx: &mut EmitEffects,
    ) -> AssertShape {
        fx.require_fmt();
        let (test_kit, fmt) = (
            go_name::GeneratedPackage::TestKit.qualifier(),
            go_name::GeneratedPackage::Fmt.qualifier(),
        );
        let lhs = self.stage_assert_operand(left, "assertLeft", statements, fx);
        let rhs = self.stage_assert_operand(right, "assertRight", statements, fx);
        let left_ref = temp_identifier(&lhs, left);
        let right_ref = temp_identifier(&rhs, right);
        let (cond_setup, condition) = self
            .plan_binary(
                operator,
                &left_ref,
                &right_ref,
                ExpressionContext::value(),
                fx,
            )
            .into_parts();
        statements.extend(cond_setup);
        AssertShape {
            condition,
            kind: "relation",
            operands: format!(
                ", {test_kit}.Operand{{Value: {fmt}.Sprintf(\"left: %v, right: %v\", {lhs}, {rhs})}}"
            ),
        }
    }

    /// `assert recv.equals(arg)`: compare via the canonical equals lowering,
    /// reporting each operand by its source text and value.
    fn lower_labeled_assert(
        &mut self,
        recv: &Expression,
        arg: &Expression,
        statements: &mut Vec<LoweredStatement>,
        fx: &mut EmitEffects,
    ) -> AssertShape {
        fx.require_fmt();
        let (test_kit, fmt) = (
            go_name::GeneratedPackage::TestKit.qualifier(),
            go_name::GeneratedPackage::Fmt.qualifier(),
        );
        let recv_ty = recv.get_type();
        let (recv_span, arg_span) = (recv.get_span(), arg.get_span());
        let lhs = self.stage_assert_operand(recv, "assertLeft", statements, fx);
        let rhs = self.stage_assert_operand(arg, "assertRight", statements, fx);
        let condition = self.render_equality(&lhs, &rhs, &recv_ty, fx);
        AssertShape {
            condition,
            kind: "labeled",
            operands: format!(
                ", {test_kit}.Operand{{Label: \"left\", Value: {fmt}.Sprintf(\"%v\", {lhs}), Lo: {}, Hi: {}}}, {test_kit}.Operand{{Label: \"right\", Value: {fmt}.Sprintf(\"%v\", {rhs}), Lo: {}, Hi: {}}}",
                recv_span.byte_offset,
                recv_span.byte_offset + recv_span.byte_length,
                arg_span.byte_offset,
                arg_span.byte_offset + arg_span.byte_length,
            ),
        }
    }

    /// `assert <expr>`: any other boolean, lowered as-is (no decomposition).
    fn lower_bare_assert(
        &mut self,
        operand: &Expression,
        statements: &mut Vec<LoweredStatement>,
        fx: &mut EmitEffects,
    ) -> AssertShape {
        let (setup, condition) = self.lower_condition(operand, fx);
        statements.extend(setup);
        AssertShape {
            condition,
            kind: "bare",
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
        fx: &mut EmitEffects,
    ) -> String {
        let (setup, value) = self
            .lower_value(expression, ExpressionContext::value(), fx)
            .into_parts();
        statements.extend(setup);
        let name = self.fresh_var(Some(hint));
        self.declare(&name);
        // Bind the temp to itself so the relation shape's synthetic identifier resolves.
        self.scope.bind(name.clone(), name.clone());
        let go_type = self.go_type_string(&expression.get_type(), fx);
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
    fn lower_while_let_statement(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
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
            fx,
        );
        self.pop_loop();
        directed(directive, LoweredStatement::WhileLet(WhileLetPlan { body }))
    }

    fn lower_infinite_loop(
        &mut self,
        body: &Expression,
        needs_label: bool,
        fx: &mut EmitEffects,
    ) -> LoopPlan {
        self.push_loop("_");
        let plan = self.lower_loop_with_header("for {\n".to_string(), body, needs_label, fx);
        self.pop_loop();
        plan
    }

    fn lower_condition(
        &mut self,
        condition: &Expression,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let plan = self.plan_operand(condition, ExpressionContext::value().condition(), fx);
        plan.into_parts()
    }

    fn lower_while(
        &mut self,
        condition: &Expression,
        body: &Expression,
        needs_label: bool,
        fx: &mut EmitEffects,
    ) -> LoopPlan {
        self.push_loop("_");
        let (setup, rendered) = self.lower_condition(condition, fx);
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
        let plan = self.lower_loop_with_header(header, body, needs_label, fx);
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
        fx: &mut EmitEffects,
    ) -> LoopPlan {
        self.set_current_loop_label_if_needed(needs_label);
        let label = self.current_loop_label().map(str::to_string);
        self.enter_scope();
        let lowered_body = self.lower_block_as_body(body, fx);
        self.exit_scope();
        LoopPlan {
            prologue: Vec::new(),
            label,
            header,
            body: lowered_body,
        }
    }

    /// Lower a branch arm body in statement position (`PlacePlan::Statement`).
    pub(crate) fn lower_block_as_body(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };
        let statements = items
            .iter()
            .map(|item| self.lower_statement(item, fx))
            .collect();
        LoweredBlock { statements }
    }

    /// Lower a branching expression (`if`, `match`, `select`) into a
    /// `LoweredBlock` targeting `place`. Centralises the dispatch shared by old
    /// emit paths that need to render a branching tail/assignment.
    pub(crate) fn lower_branching_to_block(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let plan = self.lower_if(condition, consequence, alternative, place, fx);
                LoweredBlock {
                    statements: vec![LoweredStatement::If(plan)],
                }
            }
            Expression::Match { subject, arms, .. } => {
                self.lower_match_to_block(subject, arms, place, fx)
            }
            Expression::Select { arms, .. } => LoweredBlock {
                statements: vec![LoweredStatement::Select(self.lower_select(arms, place, fx))],
            },
            _ => unreachable!("lower_branching_to_block: expected if/match/select"),
        }
    }

    /// Lower a branch arm body into the given place.
    pub(crate) fn lower_block_to_place(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        match place {
            PlacePlan::Statement => self.lower_block_as_body(expression, fx),
            PlacePlan::Return => self.lower_block_to_return(expression, fx),
            PlacePlan::Assign { local, target_ty } => {
                self.lower_block_to_assign(expression, local, *target_ty, fx)
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
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        if expression.get_type().is_result() || expression.get_type().is_option() {
            return LoweredBlock {
                statements: self.lower_option_result_assignment(local, target_ty, expression, fx),
            };
        }
        LoweredBlock {
            statements: self.lower_block_to_var(expression, local, target_ty, false, fx),
        }
    }

    /// Lower a block in return position: non-tail items become statements,
    /// the tail returns. A tail `let` (`let x = if ...; x`) is elided into the
    /// surrounding return place; function bodies skip that elision.
    fn lower_block_to_return(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
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

        let (statements, _) = self.lower_body_until_diverge(rest, last, fx);
        LoweredBlock { statements }
    }

    /// Lower a single tail expression in return position to its return
    /// statements. Shared by branch-arm return lowering and function-body
    /// lowering; only leaf values and lowered-ABI returns become `RawGo`,
    /// `if`/`match`/`select` tails recurse structurally with a `Return` place.
    pub(crate) fn lower_return_tail(
        &mut self,
        last: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        let return_span = last.get_span();
        let last = if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        };

        if last.get_type().is_unit() {
            if !matches!(last, Expression::Unit { .. }) {
                statements.push(self.lower_statement(last, fx));
            }
            return statements;
        }

        if last.get_type().is_never() {
            return self.lower_never_return_tail(last, &return_span, fx);
        }

        let directive = self.maybe_line_directive(&return_span);
        match last {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let plan =
                    self.lower_if(condition, consequence, alternative, &PlacePlan::Return, fx);
                statements.push(directed(directive, LoweredStatement::If(plan)));
            }
            Expression::Match { subject, arms, .. } => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                let block = self.lower_match_to_block(subject, arms, &PlacePlan::Return, fx);
                statements.extend(block.statements);
            }
            Expression::Select { arms, .. } => {
                let plan = self.lower_select(arms, &PlacePlan::Return, fx);
                statements.push(directed(directive, LoweredStatement::Select(plan)));
            }
            _ => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                if let Some(tail) = try_emit_lowered_tail_return(self, last, fx) {
                    statements.extend(tail);
                } else if let Some(wrapped) = self.lower_wrapped_return(last, fx) {
                    statements.extend(wrapped);
                } else {
                    statements.extend(self.lower_plain_return_tail(last, fx));
                }
            }
        }

        statements
    }

    fn lower_never_return_tail(
        &mut self,
        last: &Expression,
        return_span: &syntax::ast::Span,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let directive = self.maybe_line_directive(return_span);
        let mut statements: Vec<LoweredStatement> = Vec::new();
        if !directive.is_empty() {
            statements.push(LoweredStatement::RawGo(directive));
        }
        statements.push(self.lower_statement(last, fx));
        if !is_go_never(last) && !is_breakless_loop(last) {
            statements.push(LoweredStatement::UnreachablePanic);
        }
        statements
    }

    /// Kept as `RawGo`, not `ReturnForm::Plain`: a structured `Return` would
    /// flatten the enclosing `else` for a multi-line return value.
    fn lower_plain_return_tail(
        &mut self,
        last: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        if requires_temp_var(last) {
            let staged = self.stage_operand(last, ExpressionContext::value(), fx);
            let mut statements = staged.setup;
            if !staged.value.is_empty() {
                statements.push(plain_return(staged.value));
            }
            statements
        } else {
            let (mut statements, expression_string) = self.lower_tail_value(last, fx);
            let return_ctx = self.return_ctx();
            let mut coercion = String::new();
            let expression_string = self.apply_type_coercion(
                &mut coercion,
                return_ctx.ty(),
                last,
                expression_string,
                fx,
            );
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
        fx: &mut EmitEffects,
    ) -> IfPlan {
        let (condition_setup, condition_string) = self.lower_condition(condition, fx);
        let condition = wrap_if_struct_literal(condition_string);

        self.enter_scope();
        let then_body = self.lower_block_to_place(consequence, place, fx);
        self.exit_scope();

        let preceding_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_chain(alternative, preceding_diverges, place, fx);

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
        fx: &mut EmitEffects,
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
            let (condition_setup, condition_string) = self.lower_condition(condition, fx);
            let condition = wrap_if_struct_literal(condition_string);

            // With-setup else-if renders as a nested block (`} else { setup; if
            // ... }`), so its body sits in an inner scope inside an outer scope
            // that also wraps the recursion. Plain else-if uses a single scope
            // around the body and recurses outside it.
            if !condition_setup.is_empty() {
                self.enter_scope();
                self.enter_scope();
                let then_body = self.lower_block_to_place(consequence, place, fx);
                self.exit_scope();
                let inner = self.lower_else_chain(
                    next_alternative,
                    then_body.ends_with_diverge(),
                    place,
                    fx,
                );
                self.exit_scope();
                ElseArm::ElseIf(Box::new(IfPlan {
                    condition_setup,
                    condition,
                    then_body,
                    else_arm: inner,
                }))
            } else {
                self.enter_scope();
                let then_body = self.lower_block_to_place(consequence, place, fx);
                self.exit_scope();
                let inner = self.lower_else_chain(
                    next_alternative,
                    then_body.ends_with_diverge(),
                    place,
                    fx,
                );
                ElseArm::ElseIf(Box::new(IfPlan {
                    condition_setup,
                    condition,
                    then_body,
                    else_arm: inner,
                }))
            }
        } else if preceding_diverges {
            let body = self.lower_block_to_place(alternative, place, fx);
            ElseArm::Else { body, inline: true }
        } else {
            self.enter_scope();
            let body = self.lower_block_to_place(alternative, place, fx);
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
    operands: String,
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
