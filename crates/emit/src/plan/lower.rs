use crate::EmitEffects;
use crate::Planner;
use crate::ReturnContext;
use crate::abi::transition::try_emit_lowered_tail_return;
use crate::context::expression::ExpressionContext;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::control_flow::propagation::plain_return;
use crate::definitions::functions::is_go_never;
use crate::plan::bodies::{
    ElseArm, ExpressionStatementForm, ExpressionStatementPlan, IfPlan, LoopPlan, LoweredBlock,
    LoweredStatement, MatchStatementPlan, PlacePlan, WhileLetPlan,
};
use crate::plan::placement::{requires_temp_var, try_elide_tail_let};
use crate::plan::values::setup_from_string;
use crate::write_line;
use syntax::ast::{Expression, Literal};
use syntax::types::Type;

impl Planner<'_> {
    /// Allocate a fresh operand-temp result var and its `var V T` declaration
    /// as a typed setup leaf. The control-flow that assigns it follows as a
    /// typed `If`/`Loop`/`Match`/`Select` statement.
    fn operand_temp_declaration(
        &mut self,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> (String, LoweredStatement) {
        let result_var = self.fresh_var(None);
        let declaration = format!("var {} {}\n", result_var, self.go_type_string(ty, fx));
        self.declare(&result_var);
        (result_var, LoweredStatement::RawGo(declaration))
    }

    /// Plan a value-position `if` as a fresh operand-temp variable: a `var V T`
    /// declaration leaf plus a typed `If` statement that assigns `V`. Returns
    /// the setup statements and `V`.
    pub(crate) fn plan_if_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
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
        let return_ctx = ambient
            .cloned()
            .or_else(|| self.current_return_ctx().cloned())
            .expect("operand-position control flow requires an enclosing return context");
        let plan = self.lower_if(
            String::new(),
            condition,
            consequence,
            alternative,
            &PlacePlan::Assign {
                local: &result_var,
                target_ty: Some(ty),
                return_ctx: &return_ctx,
            },
            fx,
        );
        (vec![declaration, LoweredStatement::If(plan)], result_var)
    }

    /// Lower a value-position `match`/`select` to a fresh operand-temp
    /// variable. Only valid for non-never result types; never-typed branches
    /// route through `emit_to_operand_temp` as a diverging statement.
    pub(crate) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let (result_var, declaration) = self.operand_temp_declaration(ty, fx);
        let return_ctx = ambient
            .cloned()
            .or_else(|| self.current_return_ctx().cloned())
            .expect("operand-position control flow requires an enclosing return context");
        let block = self.lower_branching_to_block(
            expression,
            &PlacePlan::Assign {
                local: &result_var,
                target_ty: Some(ty),
                return_ctx: &return_ctx,
            },
            fx,
        );
        let mut setup = vec![declaration];
        setup.extend(block.statements);
        (setup, result_var)
    }

    /// Lower a value-position `loop` to a fresh operand-temp variable.
    /// Declares `var V T`, pushes `V` as the current loop result slot so
    /// `break value` assigns into it, lowers the loop, then renders.
    pub(crate) fn plan_loop_as_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::Loop {
            body, needs_label, ..
        } = expression
        else {
            unreachable!("plan_loop_as_operand_temp called on non-Loop expression");
        };
        let (result_var, declaration) = self.operand_temp_declaration(ty, fx);
        let return_ctx = ambient
            .cloned()
            .or_else(|| self.current_return_ctx().cloned())
            .expect("operand-position control flow requires an enclosing return context");
        self.push_loop(result_var.clone());
        let plan = self.lower_loop_with_header(
            String::new(),
            "for {\n".to_string(),
            body,
            *needs_label,
            &return_ctx,
            fx,
        );
        self.pop_loop();
        (vec![declaration, LoweredStatement::Loop(plan)], result_var)
    }

    fn lower_body_until_diverge(
        &mut self,
        rest: &[Expression],
        last: &Expression,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, bool) {
        let mut statements: Vec<LoweredStatement> = Vec::with_capacity(rest.len() + 1);
        for item in rest {
            let statement = self.lower_statement(item, return_ctx, fx);
            let diverged = statement.blocks_fallthrough();
            statements.push(statement);
            if diverged {
                return (statements, true);
            }
        }
        statements.extend(self.lower_return_tail(last, return_ctx, fx));
        (statements, false)
    }

    pub(crate) fn lower_function_body(
        &mut self,
        body: &Expression,
        should_return: bool,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        if !should_return {
            return self.lower_block_as_body(body, return_ctx, fx);
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

        let (mut statements, diverged) = self.lower_body_until_diverge(rest, last, return_ctx, fx);
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
        return_ctx: &ReturnContext,
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
                    directive,
                    condition,
                    consequence,
                    alternative,
                    &PlacePlan::statement(return_ctx),
                    fx,
                );
                LoweredStatement::If(plan)
            }
            Expression::Loop {
                body, needs_label, ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                LoweredStatement::Loop(self.lower_infinite_loop(
                    directive,
                    body,
                    *needs_label,
                    return_ctx,
                    fx,
                ))
            }
            Expression::While {
                condition,
                body,
                needs_label,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                LoweredStatement::Loop(self.lower_while(
                    directive,
                    condition,
                    body,
                    *needs_label,
                    return_ctx,
                    fx,
                ))
            }
            Expression::Block { .. } => {
                self.enter_scope();
                let body = self.lower_block_as_body(expression, return_ctx, fx);
                self.exit_scope();
                LoweredStatement::Block(body)
            }
            Expression::For { .. } => self.lower_for_statement(expression, return_ctx, fx),
            Expression::Continue { .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                LoweredStatement::Continue {
                    directive,
                    label: self.current_loop_label().map(str::to_string),
                }
            }
            Expression::Break { value: None, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                LoweredStatement::Break {
                    directive,
                    label: self.current_loop_label().map(str::to_string),
                }
            }
            Expression::Break {
                value: Some(value), ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_break_value_plan(value, directive, fx);
                LoweredStatement::BreakValue(plan)
            }
            Expression::Const {
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_const_plan(identifier, value, ty, directive, fx);
                LoweredStatement::Const(plan)
            }
            Expression::Return {
                expression: value, ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_return_plan(value, return_ctx, directive, fx);
                LoweredStatement::Return(plan)
            }
            Expression::Let {
                binding,
                value,
                mutable,
                else_block,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_let_plan(
                    binding,
                    value,
                    else_block.as_deref(),
                    *mutable,
                    return_ctx,
                    directive,
                    fx,
                );
                LoweredStatement::Let(plan)
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let plan = self.build_assignment_plan(
                    target,
                    value,
                    compound_operator.as_ref(),
                    return_ctx,
                    directive,
                    fx,
                );
                LoweredStatement::Assign(plan)
            }
            Expression::Match { subject, arms, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let body =
                    self.lower_match_to_block(subject, arms, &PlacePlan::statement(return_ctx), fx);
                LoweredStatement::Match(MatchStatementPlan { directive, body })
            }
            Expression::Select { arms, .. } => {
                let directive = self.maybe_line_directive(&expression.get_span());
                let mut plan = self.lower_select(arms, &PlacePlan::statement(return_ctx), fx);
                plan.directive = directive;
                LoweredStatement::Select(plan)
            }
            Expression::WhileLet { .. } => {
                self.lower_while_let_statement(expression, return_ctx, fx)
            }
            // Top-level items (Struct/Enum/etc) shouldn't appear inside
            // function bodies, but dispatch handles them defensively. They
            // carry their own directive (via `emit_top_item`) so the wrapper
            // does not add one.
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::ValueEnum { .. }
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
            _ => self.lower_expression_statement(expression, return_ctx, fx),
        }
    }

    /// Lower the statement-position fall-through: Task/Defer (async value),
    /// `expr?` propagation, or an otherwise-discarded expression value. The
    /// directive rides on the wrapper so the rendered body stays directive-free.
    fn lower_expression_statement(
        &mut self,
        expression: &Expression,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let unwrapped = expression.unwrap_parens();
        let directive = self.maybe_line_directive(&expression.get_span());
        let form = if matches!(
            unwrapped,
            Expression::Task { .. } | Expression::Defer { .. }
        ) {
            let value = self.plan_operand(
                unwrapped,
                ExpressionContext::value().with_ambient_return_ctx(return_ctx),
                fx,
            );
            ExpressionStatementForm::Async { value }
        } else if let Expression::Propagate {
            expression: inner, ..
        } = unwrapped
        {
            ExpressionStatementForm::Propagate {
                body: LoweredBlock {
                    statements: self.lower_propagate_statement(inner, return_ctx, fx),
                },
            }
        } else {
            let mut rendered = String::new();
            self.emit_discard(&mut rendered, unwrapped, return_ctx, fx);
            ExpressionStatementForm::Discard {
                body: LoweredBlock {
                    statements: vec![LoweredStatement::RawGo(rendered)],
                },
            }
        };
        LoweredStatement::Expression(ExpressionStatementPlan { directive, form })
    }

    /// Lower `while let P = scrutinee { body }` via the `emit_while_let` string
    /// bridge, wrapped as a `WhileLet` statement.
    fn lower_while_let_statement(
        &mut self,
        expression: &Expression,
        return_ctx: &ReturnContext,
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
        let mut buffer = String::new();
        self.push_loop("_");
        self.emit_while_let(
            &mut buffer,
            pattern,
            typed_pattern.as_ref(),
            scrutinee,
            body,
            *needs_label,
            return_ctx,
            fx,
        );
        self.pop_loop();
        let body = LoweredBlock {
            statements: vec![LoweredStatement::RawGo(buffer)],
        };
        LoweredStatement::WhileLet(WhileLetPlan { directive, body })
    }

    fn lower_infinite_loop(
        &mut self,
        directive: String,
        body: &Expression,
        needs_label: bool,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoopPlan {
        self.push_loop("_");
        let plan = self.lower_loop_with_header(
            directive,
            "for {\n".to_string(),
            body,
            needs_label,
            return_ctx,
            fx,
        );
        self.pop_loop();
        plan
    }

    fn lower_while(
        &mut self,
        directive: String,
        condition: &Expression,
        body: &Expression,
        needs_label: bool,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoopPlan {
        self.push_loop("_");
        let (setup, rendered) = self.capture_emission(&mut String::new(), |this, buffer| {
            this.emit_operand(
                buffer,
                condition,
                ExpressionContext::value().condition(),
                fx,
            )
        });
        let header = if !setup.is_empty() {
            // Condition produced setup statements (temps); they must re-run each
            // iteration, so move everything inside the loop.
            format!("for {{\n{}if !({}) {{ break }}\n", setup, rendered)
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
        let plan =
            self.lower_loop_with_header(directive, header, body, needs_label, return_ctx, fx);
        self.pop_loop();
        plan
    }

    /// Shared loop lowering once the header is known: set the label, lower
    /// the body in a fresh scope. Caller owns `push_loop`/`pop_loop`.
    fn lower_loop_with_header(
        &mut self,
        directive: String,
        header: String,
        body: &Expression,
        needs_label: bool,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoopPlan {
        self.set_current_loop_label_if_needed(needs_label);
        let label = self.current_loop_label().map(str::to_string);
        self.enter_scope();
        let lowered_body = self.lower_block_as_body(body, return_ctx, fx);
        self.exit_scope();
        LoopPlan {
            directive,
            prologue: String::new(),
            label,
            header,
            body: lowered_body,
        }
    }

    /// Lower a branch arm body in statement position (the `PlacePlan::Statement`
    /// equivalent of `emit_block`).
    pub(crate) fn lower_block_as_body(
        &mut self,
        expression: &Expression,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };
        let statements = items
            .iter()
            .map(|item| self.lower_statement(item, return_ctx, fx))
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
                let plan = self.lower_if(
                    String::new(),
                    condition,
                    consequence,
                    alternative,
                    place,
                    fx,
                );
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
            PlacePlan::Statement { return_ctx } => {
                self.lower_block_as_body(expression, return_ctx, fx)
            }
            PlacePlan::Return(return_ctx) => self.lower_block_to_return(expression, return_ctx, fx),
            PlacePlan::Assign {
                local,
                target_ty,
                return_ctx,
            } => self.lower_block_to_assign(expression, local, *target_ty, return_ctx, fx),
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
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> LoweredBlock {
        if expression.get_type().is_result() || expression.get_type().is_option() {
            return LoweredBlock {
                statements: self
                    .lower_option_result_assignment(local, target_ty, expression, return_ctx, fx),
            };
        }
        LoweredBlock {
            statements: self
                .lower_block_to_var(expression, local, target_ty, false, return_ctx, fx),
        }
    }

    /// Lower a block in return position: non-tail items become statements,
    /// the tail returns. A tail `let` (`let x = if ...; x`) is elided into the
    /// surrounding return place; function bodies skip that elision.
    fn lower_block_to_return(
        &mut self,
        expression: &Expression,
        return_ctx: &ReturnContext,
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

        let (statements, _) = self.lower_body_until_diverge(rest, last, return_ctx, fx);
        LoweredBlock { statements }
    }

    /// Lower a single tail expression in return position to its return
    /// statements. Shared by branch-arm return lowering and function-body
    /// lowering; only leaf values and lowered-ABI returns become `RawGo`,
    /// `if`/`match`/`select` tails recurse structurally with a `Return` place.
    pub(crate) fn lower_return_tail(
        &mut self,
        last: &Expression,
        return_ctx: &ReturnContext,
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
                statements.push(self.lower_statement(last, return_ctx, fx));
            }
            return statements;
        }

        if last.get_type().is_never() {
            return self.lower_never_return_tail(last, &return_span, return_ctx, fx);
        }

        let directive = self.maybe_line_directive(&return_span);
        match last {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let plan = self.lower_if(
                    directive,
                    condition,
                    consequence,
                    alternative,
                    &PlacePlan::Return(return_ctx),
                    fx,
                );
                statements.push(LoweredStatement::If(plan));
            }
            Expression::Match { subject, arms, .. } => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                let block =
                    self.lower_match_to_block(subject, arms, &PlacePlan::Return(return_ctx), fx);
                statements.extend(block.statements);
            }
            Expression::Select { arms, .. } => {
                let mut plan = self.lower_select(arms, &PlacePlan::Return(return_ctx), fx);
                plan.directive = directive;
                statements.push(LoweredStatement::Select(plan));
            }
            _ => {
                if !directive.is_empty() {
                    statements.push(LoweredStatement::RawGo(directive));
                }
                if let Some(tail) = try_emit_lowered_tail_return(self, last, return_ctx, fx) {
                    statements.extend(tail);
                } else if let Some(wrapped) = self.lower_wrapped_return(last, return_ctx, fx) {
                    statements.extend(wrapped);
                } else {
                    statements.extend(self.lower_plain_return_tail(last, return_ctx, fx));
                }
            }
        }

        statements
    }

    fn lower_never_return_tail(
        &mut self,
        last: &Expression,
        return_span: &syntax::ast::Span,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut statements = setup_from_string(self.maybe_line_directive(return_span));
        statements.push(self.lower_statement(last, return_ctx, fx));
        if !is_go_never(last) {
            statements.push(LoweredStatement::RawGo(
                "panic(\"unreachable\")\n".to_string(),
            ));
        }
        statements
    }

    /// Kept as `RawGo`, not `ReturnForm::Plain`: a structured `Return` would
    /// flatten the enclosing `else` for a multi-line return value.
    fn lower_plain_return_tail(
        &mut self,
        last: &Expression,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut buffer = String::new();
        if requires_temp_var(last) {
            let expression_string = self.emit_operand(
                &mut buffer,
                last,
                ExpressionContext::value().with_ambient_return_ctx(return_ctx),
                fx,
            );
            if !expression_string.is_empty() {
                write_line!(buffer, "return {}", expression_string);
            }
        } else {
            let expression_string = self.emit_tail_value(&mut buffer, last, return_ctx, fx);
            let return_ty = return_ctx.ty();
            let expression_string =
                self.apply_type_coercion(&mut buffer, return_ty, last, expression_string, fx);
            write_line!(buffer, "return {}", expression_string);
        }
        vec![LoweredStatement::RawGo(buffer)]
    }

    pub(crate) fn lower_if(
        &mut self,
        directive: String,
        condition: &Expression,
        consequence: &Expression,
        alternative: &Expression,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> IfPlan {
        let (condition_setup, condition_string) =
            self.capture_emission(&mut String::new(), |this, buffer| {
                this.emit_operand(
                    buffer,
                    condition,
                    ExpressionContext::value().condition(),
                    fx,
                )
            });
        let condition = wrap_if_struct_literal(condition_string);

        self.enter_scope();
        let then_body = self.lower_block_to_place(consequence, place, fx);
        self.exit_scope();

        let preceding_diverges = then_body.ends_with_diverge();
        let else_arm = self.lower_else_chain(alternative, preceding_diverges, place, fx);

        IfPlan {
            directive,
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
            let (condition_setup, condition_string) =
                self.capture_emission(&mut String::new(), |this, buffer| {
                    this.emit_operand(
                        buffer,
                        condition,
                        ExpressionContext::value().condition(),
                        fx,
                    )
                });
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
                    directive: String::new(),
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
                    directive: String::new(),
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
