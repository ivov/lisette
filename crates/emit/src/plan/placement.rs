use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::analyze::inline_uses::region_blocks_inline;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FalliblePlanner};
use crate::definitions::functions::is_go_never;
use crate::expressions::emission::StagedExpression;
use crate::plan::bodies::{
    AssignForm, AssignPlan, BreakValueDisposition, BreakValuePlan, LoweredStatement, PlacePlan,
};
use crate::plan::calls::plan_variadic_spread;
use crate::plan::values::{ValuePlan, setup_from_string, value_plan_from_statements};
use crate::statements::assignments::is_lvalue_chain;
use crate::types::native::NativeGoType;
use crate::utils::contains_call;
use crate::write_line;
use syntax::ast::{Expression, Literal};
use syntax::types::Type;

/// Append `panic("unreachable")` after a branch construct in return position
/// when the branch can fall through (no exhaustive default arm). Go would
/// otherwise reject the function for missing a tail return.
pub(crate) fn emit_unreachable_panic_if_needed(
    output: &mut String,
    place: &PlacePlan,
    is_exhaustive: bool,
) {
    if place.is_return() && !is_exhaustive {
        output.push_str("panic(\"unreachable\")\n");
    }
}

/// True when discarding `expression` is safe to omit: its value has no
/// side effects. `FormatString` and `Slice` literals are excluded since they
/// can hold sub-expressions that do.
fn is_side_effect_free_discard(expression: &Expression) -> bool {
    match expression {
        Expression::Unit { .. } => true,
        Expression::Literal { literal, .. } => matches!(
            literal,
            Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_)
        ),
        _ => false,
    }
}

pub(crate) fn is_unit_call(expression: &Expression) -> bool {
    expression.get_type().is_unit() && matches!(expression.unwrap_parens(), Expression::Call { .. })
}

/// A `target = value` assignment with no lvalue capture.
fn simple_assign(target_var: &str, value: ValuePlan) -> LoweredStatement {
    LoweredStatement::Assign(AssignPlan {
        directive: String::new(),
        form: AssignForm::Simple {
            target_capture: Vec::new(),
            target_str: target_var.to_string(),
            value,
        },
    })
}

pub(crate) fn requires_temp_var(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Block { .. }
            | Expression::Loop { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::Select { .. }
    )
}

/// Match `...; let X = <CF>; X` so the caller can emit `<CF>` directly into
/// the surrounding place, skipping the `X` temp.
pub(crate) fn try_elide_tail_let(items: &[Expression]) -> Option<(&Expression, &[Expression])> {
    if items.len() < 2 {
        return None;
    }
    let last = items.last()?;
    let Expression::Identifier {
        value: tail_name, ..
    } = last
    else {
        return None;
    };
    let penultimate = &items[items.len() - 2];
    let Expression::Let {
        binding,
        value,
        else_block,
        mutable,
        ..
    } = penultimate
    else {
        return None;
    };
    if else_block.is_some() || *mutable {
        return None;
    }
    let syntax::ast::Pattern::Identifier { identifier, .. } = &binding.pattern else {
        return None;
    };
    if identifier != tail_name {
        return None;
    }
    // Only `If` and `Match` can be re-emitted at the surrounding place via
    // branch lowering (`lower_branching_to_block`); other shapes still stage
    // through temps so eliding the let would not save anything.
    if !matches!(
        value.as_ref(),
        Expression::If { .. } | Expression::Match { .. }
    ) {
        return None;
    }
    let rest = &items[..items.len() - 2];
    if region_blocks_inline(rest.iter(), tail_name.as_str()) {
        return None;
    }
    Some((value.as_ref(), rest))
}

pub(crate) fn expression_contains_binding(expression: &Expression, name: &str) -> bool {
    use syntax::ast::{Pattern, RestPattern, SelectArmPattern};
    fn pattern_contains_name(pattern: &Pattern, name: &str) -> bool {
        match pattern {
            Pattern::Identifier { identifier, .. } => identifier.as_str() == name,
            Pattern::EnumVariant { fields, .. } => {
                fields.iter().any(|f| pattern_contains_name(f, name))
            }
            Pattern::Struct { fields, .. } => {
                fields.iter().any(|f| pattern_contains_name(&f.value, name))
            }
            Pattern::Tuple { elements, .. } => {
                elements.iter().any(|e| pattern_contains_name(e, name))
            }
            Pattern::Slice { prefix, rest, .. } => {
                prefix.iter().any(|p| pattern_contains_name(p, name))
                    || matches!(rest, RestPattern::Bind { name: n, .. } if n == name)
            }
            Pattern::Or { patterns, .. } => patterns.iter().any(|p| pattern_contains_name(p, name)),
            Pattern::AsBinding {
                pattern,
                name: as_name,
                ..
            } => as_name == name || pattern_contains_name(pattern, name),
            Pattern::Literal { .. } | Pattern::Unit { .. } | Pattern::WildCard { .. } => false,
        }
    }
    match expression {
        Expression::Match { arms, .. } => arms
            .iter()
            .any(|arm| pattern_contains_name(&arm.pattern, name)),
        Expression::Block { items, .. } => items.iter().any(|item| match item {
            Expression::Let { binding, .. } => pattern_contains_name(&binding.pattern, name),
            _ => false,
        }),
        Expression::If {
            consequence,
            alternative,
            ..
        } => {
            expression_contains_binding(consequence, name)
                || expression_contains_binding(alternative, name)
        }
        Expression::Select { arms, .. } => arms.iter().any(|arm| match &arm.pattern {
            SelectArmPattern::Receive { binding, .. } => pattern_contains_name(binding, name),
            SelectArmPattern::MatchReceive { arms, .. } => {
                arms.iter().any(|a| pattern_contains_name(&a.pattern, name))
            }
            _ => false,
        }),
        Expression::Loop { body, .. } => expression_contains_binding(body, name),
        _ => false,
    }
}

impl Planner<'_> {
    pub(crate) fn emit_discard(
        &mut self,
        output: &mut String,
        value: &Expression,
        fx: &mut EmitEffects,
    ) {
        let unwrapped = value.unwrap_parens();

        if is_side_effect_free_discard(unwrapped) {
            return;
        }

        if let Expression::Propagate { expression, .. } = unwrapped {
            self.emit_propagate(output, expression, Some("_"), fx);
            return;
        }

        let value_ty = value.get_type();
        if value_ty.is_unit() || value_ty.is_variable() || value_ty.is_never() {
            let value_expression = self.emit_operand(output, value, ExpressionContext::value(), fx);
            if !value_expression.is_empty() {
                if matches!(unwrapped, Expression::Call { .. }) {
                    write_line!(output, "{}", value_expression);
                } else {
                    write_line!(output, "_ = {}", value_expression);
                }
            }
            return;
        }

        if let Expression::Call { .. } = unwrapped
            && let Some(raw) = self.emit_go_call_discarded(output, unwrapped, fx)
        {
            write_line!(output, "{}", raw);
            return;
        }

        let value_expression = self.emit_operand(output, value, ExpressionContext::value(), fx);
        write_line!(output, "_ = {}", value_expression);
    }

    /// Emit a unit-typed call as a statement, then store `struct{}{}` into
    /// `var`.
    fn lower_unit_call_into_var(
        &mut self,
        value: &Expression,
        var: &str,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut buffer = String::new();
        let call_str = self.emit_value(&mut buffer, value, ExpressionContext::value(), fx);
        if !call_str.is_empty() {
            write_line!(buffer, "{call_str}");
        }
        let mut statements = setup_from_string(buffer);
        statements.push(simple_assign(
            var,
            ValuePlan::Operand("struct{}{}".to_string()),
        ));
        statements
    }

    pub(crate) fn emit_assign(
        &mut self,
        output: &mut String,
        expression: &Expression,
        var: &str,
        target_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) {
        let ty = expression.get_type();
        let is_fallible = ty.is_result() || ty.is_option();
        if is_fallible {
            let statements = self.lower_option_result_assignment(var, target_ty, expression, fx);
            output.push_str(&Renderer.render_setup(&statements));
            return;
        }

        if let Expression::Loop {
            body, needs_label, ..
        } = expression
        {
            self.push_loop(var);
            self.emit_labeled_loop(output, "for {\n", body, *needs_label, fx);
            self.pop_loop();
            return;
        }

        if let Expression::Block { items, .. } = expression
            && items.len() > 1
        {
            output.push_str("{\n");
            let statements = self.lower_block_to_var(expression, var, target_ty, true, fx);
            output.push_str(&Renderer.render_setup(&statements));
            output.push_str("}\n");
            return;
        }

        let statements = self.lower_block_to_var(expression, var, target_ty, false, fx);
        output.push_str(&Renderer.render_setup(&statements));
    }

    fn lower_plain_assign(
        &mut self,
        target_var: &str,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut buffer = String::new();
        let expression_string =
            self.emit_operand(&mut buffer, expression, ExpressionContext::value(), fx);
        let value = value_plan_from_statements(setup_from_string(buffer), expression_string);
        vec![simple_assign(target_var, value)]
    }

    /// Assign an `Option`/`Result`-typed expression into `target_var`.
    /// `Ok`/`Err`/`Some`/`None` constructors become a structured `Simple`
    /// assignment of the constructor call; everything else falls back to a plain
    /// assign or `lower_block_to_var`.
    pub(crate) fn lower_option_result_assignment(
        &mut self,
        target_var: &str,
        target_ty: Option<&Type>,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let ty = target_ty
            .filter(|t| t.is_option() || t.is_result())
            .cloned()
            .unwrap_or_else(|| expression.get_type());
        let Some(fallible) = Fallible::from_type(&ty) else {
            return self.lower_plain_assign(target_var, expression, fx);
        };

        let actual_expression = if let Expression::Block { items, .. } = expression {
            if items.len() == 1 {
                &items[0]
            } else {
                expression
            }
        } else {
            expression
        };

        match actual_expression {
            Expression::Call {
                expression: callee,
                args,
                ..
            } => {
                let kind = fallible.classify_constructor(callee);
                let constructor_name = match kind {
                    Some(ConstructorKind::Success) => fallible.ok_constructor(),
                    Some(ConstructorKind::Failure) => fallible.err_constructor(),
                    None => {
                        return self.lower_plain_assign(target_var, expression, fx);
                    }
                };
                if kind == Some(ConstructorKind::Success)
                    || (kind == Some(ConstructorKind::Failure)
                        && fallible.err_constructor_takes_arg())
                {
                    let (arg_setup, call_str) = {
                        let mut fe = FalliblePlanner::new(self, &fallible, fx);
                        let mut arg_buffer = String::new();
                        let arg = fe.planner.emit_composite_value(
                            &mut arg_buffer,
                            &args[0],
                            ExpressionContext::value(),
                            fe.fx,
                        );
                        (
                            arg_buffer,
                            fe.format_constructor_call(constructor_name, Some(&arg)),
                        )
                    };
                    let value = value_plan_from_statements(setup_from_string(arg_setup), call_str);
                    vec![simple_assign(target_var, value)]
                } else {
                    let call_str = {
                        let mut fe = FalliblePlanner::new(self, &fallible, fx);
                        fe.format_constructor_call(constructor_name, None)
                    };
                    vec![simple_assign(target_var, ValuePlan::Operand(call_str))]
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(actual_expression)
                    == Some(ConstructorKind::Failure)
                {
                    let call_str = {
                        let mut fe = FalliblePlanner::new(self, &fallible, fx);
                        fe.format_constructor_call(fallible.err_constructor(), None)
                    };
                    vec![simple_assign(target_var, ValuePlan::Operand(call_str))]
                } else {
                    self.lower_plain_assign(target_var, expression, fx)
                }
            }
            _ => self.lower_block_to_var(expression, target_var, None, false, fx),
        }
    }

    /// Lower a block (or single expression) that assigns its tail into `var`.
    /// `has_go_braces` selects the scope discipline: a full Go-brace scope when
    /// the caller wraps the result in `{ }`, otherwise a binding frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_block_to_var(
        &mut self,
        expression: &Expression,
        var: &str,
        target_ty: Option<&Type>,
        has_go_braces: bool,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let is_block = matches!(expression, Expression::Block { .. });
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };

        self.enter_block_scope(is_block, has_go_braces);

        let mut statements = Vec::new();
        if let Some((last, rest)) = items.split_last() {
            let is_new_target = self.scope.try_acquire_assign_target(var);
            for item in rest {
                statements.push(self.lower_statement(item, fx));
            }
            statements.extend(self.lower_assign_tail(last, var, target_ty, fx));
            if is_new_target {
                self.scope.release_assign_target(var);
            }
        }

        self.exit_block_scope(is_block, has_go_braces);
        statements
    }

    fn enter_block_scope(&mut self, is_block: bool, has_go_braces: bool) {
        if !is_block {
            return;
        }
        if has_go_braces {
            self.enter_scope();
        } else {
            self.scope.push_binding_frame();
        }
    }

    fn exit_block_scope(&mut self, is_block: bool, has_go_braces: bool) {
        if !is_block {
            return;
        }
        if has_go_braces {
            self.exit_scope();
        } else {
            self.scope.pop_binding_frame();
        }
    }

    /// Lower a single tail expression in assign position into `var`.
    fn lower_assign_tail(
        &mut self,
        last: &Expression,
        var: &str,
        target_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        if matches!(
            last,
            Expression::Return { .. }
                | Expression::Break { .. }
                | Expression::Continue { .. }
                | Expression::Let { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::For { .. }
                | Expression::Const { .. }
        ) {
            return vec![self.lower_statement(last, fx)];
        }
        if last.get_type().is_never() {
            let mut statements = vec![self.lower_statement(last, fx)];
            if !is_go_never(last) {
                statements.push(LoweredStatement::RawGo(
                    "panic(\"unreachable\")\n".to_string(),
                ));
            }
            return statements;
        }
        if is_unit_call(last) {
            return self.lower_unit_call_into_var(last, var, fx);
        }
        if let Some(statements) = self.lower_append_to_var(var, last, fx) {
            return statements;
        }
        if matches!(
            last,
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. }
        ) {
            let place = PlacePlan::Assign {
                local: var,
                target_ty,
            };
            return self.lower_branching_to_block(last, &place, fx).statements;
        }
        let mut buffer = String::new();
        let expression_string = self.emit_value(&mut buffer, last, ExpressionContext::value(), fx);
        let expression_string =
            self.apply_type_coercion(&mut buffer, target_ty, last, expression_string, fx);
        let value = value_plan_from_statements(setup_from_string(buffer), expression_string);
        vec![simple_assign(var, value)]
    }

    /// `None` when `last` is not a slice `append`/`extend` call.
    fn lower_append_to_var(
        &mut self,
        var: &str,
        last: &Expression,
        fx: &mut EmitEffects,
    ) -> Option<Vec<LoweredStatement>> {
        let Expression::Call {
            expression: func,
            args,
            spread,
            ..
        } = last
        else {
            return None;
        };
        if !self.is_slice_append_or_extend(func) {
            return None;
        }

        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = func.as_ref()
        else {
            return Some(Vec::new());
        };

        let is_extend = member == "extend";
        let unwrapped = receiver.unwrap_parens();
        let receiver_is_lvalue =
            is_lvalue_chain(unwrapped) && !self.contains_newtype_access(unwrapped);

        let mut buffer = String::new();
        let value = if receiver_is_lvalue {
            let mut args_buffer = String::new();
            let args_str = self.emit_append_args(
                &mut args_buffer,
                func,
                args,
                (**spread).as_ref(),
                is_extend,
                fx,
            );
            let rhs_has_setup = !args_buffer.is_empty()
                || args.iter().any(contains_call)
                || (**spread).as_ref().is_some_and(contains_call);
            let receiver_lv =
                self.emit_left_value_capturing(&mut buffer, unwrapped, rhs_has_setup, fx);
            buffer.push_str(&args_buffer);
            format!("append({}, {})", receiver_lv, args_str)
        } else {
            self.emit_value(&mut buffer, last, ExpressionContext::value(), fx)
        };

        let mut statements = setup_from_string(buffer);
        statements.push(simple_assign(var, ValuePlan::Operand(value)));
        Some(statements)
    }

    fn is_slice_append_or_extend(&self, func: &Expression) -> bool {
        if let Expression::DotAccess {
            expression, member, ..
        } = func
            && (member == "append" || member == "extend")
        {
            return self.is_native_shape(&expression.get_type(), NativeGoType::Slice);
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_append_args(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        is_extend: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let stages: Vec<StagedExpression> = args
            .iter()
            .map(|a| self.stage_composite(a, ExpressionContext::value(), fx))
            .collect();
        let combine = plan_variadic_spread(function, spread).map(|p| p.combine(0));
        let (setup, emitted_args) =
            self.sequence_with_spread_structured(stages, spread, false, "_arg", combine, fx);
        output.push_str(&Renderer.render_setup(&setup));
        let args_str = emitted_args.join(", ");
        let suffix = if is_extend { "..." } else { "" };
        format!("{}{}", args_str, suffix)
    }

    /// Emit `last` as a tail value. Tuple literals widen slot types to the
    /// return-slot types.
    pub(crate) fn emit_tail_value(
        &mut self,
        output: &mut String,
        last: &Expression,
        fx: &mut EmitEffects,
    ) -> String {
        if let Expression::Tuple { elements, ty, .. } = last {
            self.emit_tuple_value(output, elements, ty, true, fx)
        } else {
            self.emit_value(output, last, ExpressionContext::value(), fx)
        }
    }

    pub(crate) fn emit_to_operand_temp(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let return_ctx = self.return_ctx();
        let _return_ctx = return_ctx.as_ref();
        if let Expression::Block { items, .. } = expression {
            if ty.is_never() || ty.is_unit() || matches!(ty, Type::Var { .. } | Type::Forall { .. })
            {
                self.emit_block(output, expression, fx);
                return String::new();
            }
            let result_var = self.declare_result_var(output, ty, fx);
            let needs_braces = items.len() > 1;
            if needs_braces {
                output.push_str("{\n");
            }
            let statements =
                self.lower_block_to_var(expression, &result_var, None, needs_braces, fx);
            output.push_str(&Renderer.render_setup(&statements));
            if needs_braces {
                output.push_str("}\n");
            }
            return result_var;
        }
        if let Expression::Loop {
            body, needs_label, ..
        } = expression
        {
            let result_var = self.declare_result_var(output, ty, fx);
            self.push_loop(result_var.clone());
            self.emit_labeled_loop(output, "for {\n", body, *needs_label, fx);
            self.pop_loop();
            return result_var;
        }
        let result_var = self.declare_result_var(output, ty, fx);
        self.emit_assign(output, expression, &result_var, Some(ty), fx);
        result_var
    }

    /// Build a `BreakValuePlan` for a `break value` statement.
    pub(crate) fn build_break_value_plan(
        &mut self,
        val: &Expression,
        directive: String,
        fx: &mut EmitEffects,
    ) -> BreakValuePlan {
        let value = self.plan_value(val, ExpressionContext::value(), fx);
        let value_is_empty = value.operand_text().is_some_and(str::is_empty);
        let is_propagate_diverged = value_is_empty && matches!(val, Expression::Propagate { .. });
        let disposition = if is_propagate_diverged {
            BreakValueDisposition::Diverged
        } else if let Some(result_var) = self.current_loop_result_var().map(str::to_string) {
            if is_unit_call(val) {
                BreakValueDisposition::UnitCallIntoResult { result_var }
            } else {
                BreakValueDisposition::AssignToResult { result_var }
            }
        } else {
            BreakValueDisposition::Discard
        };
        let label = self.current_loop_label().map(str::to_string);
        BreakValuePlan {
            directive,
            value,
            disposition,
            label,
        }
    }
}
