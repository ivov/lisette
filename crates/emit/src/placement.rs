//! Value placement: callers name the destination via `ValuePlace` (for
//! value-producing expressions) or `BodyPlace` (for branch arm bodies);
//! this module decides how the value gets there.

use crate::Emitter;
use crate::ReturnContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FallibleEmitter};
use crate::expressions::context::ExpressionContext;
use crate::expressions::emission::EmittedExpression;
use crate::statements::assignments::is_lvalue_chain;
use crate::types::coercion::{Coercion, CoercionDirection};
use crate::write_line;
use syntax::ast::{Expression, Literal, UnaryOperator};
use syntax::types::{Type, peel_to_range_type};

/// Where a branch arm's body writes its result.
#[derive(Clone, Debug)]
pub(crate) enum BodyPlace<'a> {
    Statement,
    Assign {
        var: String,
        target_ty: Option<Type>,
    },
    Return(&'a ReturnContext),
}

impl BodyPlace<'_> {
    pub(crate) fn is_return(&self) -> bool {
        matches!(self, BodyPlace::Return(_))
    }
}

/// Append `panic("unreachable")` after a branch construct in return position
/// when the branch can fall through (no exhaustive default arm). Go would
/// otherwise reject the function for missing a tail return.
pub(crate) fn emit_unreachable_panic_if_needed(
    output: &mut String,
    place: &BodyPlace,
    is_exhaustive: bool,
) {
    if place.is_return() && !is_exhaustive {
        output.push_str("panic(\"unreachable\")\n");
    }
}

pub(crate) enum ValuePlace<'a> {
    /// Assignment to an existing Go variable.
    Assign {
        var: &'a str,
        target_ty: Option<&'a Type>,
    },
    /// Function tail-return position.
    Return(&'a ReturnContext),
    /// Try-block success tail: emits `return Ok(value)` / `return Some(value)`.
    FallibleSuccess(&'a Fallible),
    /// Recover-block success tail: emits the inner success value into the
    /// recover wrapper's success slot.
    RecoverSuccess(&'a Fallible),
    /// Materialize into a fresh operand temp var. Returns the temp name
    /// (or `""` for never/unit-shaped values).
    OperandTemp { ty: &'a Type },
    /// `break value` placement into the enclosing loop's result var, plus the
    /// `break` (label-aware) statement. Skipped when the value diverged
    /// (e.g. `break err?` that emitted a direct return).
    BreakValue,
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

fn is_unit_call(expression: &Expression) -> bool {
    expression.get_type().is_unit() && matches!(expression.unwrap_parens(), Expression::Call { .. })
}

fn requires_temp_var(expression: &Expression) -> bool {
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

/// Match `…; let X = <CF>; X` so the caller can emit `<CF>` directly into
/// the surrounding place, skipping the `X` materialization.
fn try_elide_tail_let(items: &[Expression]) -> Option<(&Expression, &[Expression])> {
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
    // `emit_branching_directly`; other shapes still stage through temps so
    // eliding the let would not save anything.
    if !matches!(
        value.as_ref(),
        Expression::If { .. } | Expression::Match { .. }
    ) {
        return None;
    }
    let rest = &items[..items.len() - 2];
    if crate::inline_uses::region_blocks_inline(rest.iter(), tail_name.as_str()) {
        return None;
    }
    Some((value.as_ref(), rest))
}

fn needs_explicit_type_declaration(
    emitter: &Emitter,
    value: &Expression,
    binding_ty: &Type,
) -> bool {
    if emitter.facts.as_interface(binding_ty).is_some() {
        let value_ty = value.get_type();
        if *binding_ty != value_ty {
            return true;
        }
    }
    if is_fn_alias_nominal(binding_ty) {
        let value_ty = value.get_type();
        if matches!(value_ty.unwrap_forall(), Type::Function { .. }) {
            return true;
        }
    }
    match unwrap_unary_negation(value) {
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { .. } => !matches!(binding_ty.get_name(), Some("int") | None),
            Literal::Float { .. } => !matches!(binding_ty.get_name(), Some("float64") | None),
            _ => false,
        },
        _ => false,
    }
}

fn unwrap_unary_negation(expression: &Expression) -> &Expression {
    match expression {
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => expression.as_ref(),
        Expression::Paren { expression, .. } => unwrap_unary_negation(expression),
        _ => expression,
    }
}

fn is_fn_alias_nominal(ty: &Type) -> bool {
    let Type::Nominal {
        underlying_ty: Some(inner),
        ..
    } = ty.unwrap_forall()
    else {
        return false;
    };
    matches!(inner.unwrap_forall(), Type::Function { .. })
}

fn expression_contains_binding(expression: &Expression, name: &str) -> bool {
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

/// `let mut x = arr[range]` would otherwise alias the backing array.
fn maybe_clone_subslice(
    emitter: &mut Emitter,
    value: &Expression,
    mutable: bool,
    expression: String,
) -> String {
    if !is_mutable_subslice(value, mutable) {
        return expression;
    }
    emitter.requirements.require_slices();
    format!("slices.Clone({})", expression)
}

fn is_mutable_subslice(value: &Expression, mutable: bool) -> bool {
    if !mutable {
        return false;
    }
    let value = value.unwrap_parens();
    let Expression::IndexedAccess {
        expression, index, ..
    } = value
    else {
        return false;
    };
    let is_range_index = matches!(**index, Expression::Range { .. })
        || peel_to_range_type(&index.get_type()).is_some();
    if !is_range_index {
        return false;
    }
    let collection_ty = if let Some(inner) = expression.deref_inner() {
        let inner_ty = inner.get_type();
        inner_ty.inner().unwrap_or(inner_ty)
    } else {
        expression.get_type()
    };
    collection_ty.has_name("Slice")
}

/// Pick the Go type for a `let` binding's `var X T` temp. Diverging values
/// use the binding type so dead `return x` paths still typecheck; branching
/// values that produce tuples widen slots to match the assignment site.
fn resolve_let_temp_decl_ty(emitter: &mut Emitter, value: &Expression, binding_ty: &Type) -> Type {
    let value_ty = value.get_type();
    let widens_to_interface =
        emitter.facts.as_interface(binding_ty).is_some() && *binding_ty != value_ty;
    if !value_ty.is_unit() && !value_ty.is_never() && widens_to_interface {
        return binding_ty.clone();
    }
    let base = if value_ty.is_unit() || value_ty.is_never() {
        if !binding_ty.is_unit() && !binding_ty.is_variable() {
            binding_ty.clone()
        } else {
            value_ty
        }
    } else {
        value_ty
    };
    let is_branching = matches!(
        value,
        Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. }
    );
    if is_branching && let Type::Tuple(slots) = &base {
        Type::Tuple(emitter.resolve_tuple_slot_types(slots.clone(), false))
    } else {
        base
    }
}

impl Emitter<'_> {
    /// Emit `expression` into `place`. Returns `Some(name)` for
    /// `OperandTemp` (the temp var name, or `""` for never/unit-shaped
    /// values); all other variants return `None`.
    pub(crate) fn emit_to_place(
        &mut self,
        output: &mut String,
        expression: &Expression,
        place: ValuePlace<'_>,
    ) -> Option<String> {
        match place {
            ValuePlace::Assign { var, target_ty } => {
                self.emit_assign(output, expression, var, target_ty);
                None
            }
            ValuePlace::Return(ctx) => {
                self.emit_function_returning_tail(output, expression, ctx);
                None
            }
            ValuePlace::FallibleSuccess(fallible) => {
                self.emit_try_tail(output, expression, fallible);
                None
            }
            ValuePlace::RecoverSuccess(fallible) => {
                self.emit_recover_tail(output, expression, fallible);
                None
            }
            ValuePlace::OperandTemp { ty } => {
                Some(self.emit_to_operand_temp(output, expression, ty))
            }
            ValuePlace::BreakValue => {
                self.emit_break_value(output, expression);
                None
            }
        }
    }

    pub(crate) fn emit_discard(&mut self, output: &mut String, value: &Expression) {
        let unwrapped = value.unwrap_parens();

        if is_side_effect_free_discard(unwrapped) {
            return;
        }

        if let Expression::Propagate { expression, .. } = unwrapped {
            let return_ctx = self.scope_return_context_fallback().clone();
            self.emit_propagate(output, expression, Some("_"), &return_ctx);
            return;
        }

        let value_ty = value.get_type();
        if value_ty.is_unit() || value_ty.is_variable() || value_ty.is_never() {
            let value_expression = self.emit_operand(output, value, ExpressionContext::value());
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
            && let Some(raw) = self.emit_go_call_discarded(output, unwrapped)
        {
            write_line!(output, "{}", raw);
            return;
        }

        let is_lowered_lisette_call = if let Expression::Call {
            expression: callee, ..
        } = unwrapped
        {
            self.classify_callee_abi(callee).is_some()
        } else {
            false
        };
        if is_lowered_lisette_call {
            let call_str = self.emit_call(output, value, None, ExpressionContext::value());
            write_line!(output, "{}", call_str);
            return;
        }

        let value_expression = self.emit_operand(output, value, ExpressionContext::value());
        write_line!(output, "_ = {}", value_expression);
    }

    /// Pick the Go name for a `let` binding: prefer the escaped `raw_go_name`,
    /// fall back to a fresh `identifier`-derived name on collision (already
    /// declared in scope, or `force_fresh` set when the binding shadows a
    /// reference inside the value expression).
    fn choose_let_go_name(
        &mut self,
        identifier: &str,
        raw_go_name: &str,
        force_fresh: bool,
    ) -> String {
        let escaped = crate::escape_reserved(raw_go_name);
        if force_fresh || self.is_declared(&escaped) {
            self.fresh_var(Some(identifier))
        } else {
            escaped.into_owned()
        }
    }

    /// Emit a unit-typed call as a statement, then store `struct{}{}` into
    /// `var`. Preserves the call's side effects while giving the var a
    /// well-typed value.
    fn emit_unit_call_into_var(&mut self, output: &mut String, value: &Expression, var: &str) {
        let call_str = self.emit_value(output, value, ExpressionContext::value());
        if !call_str.is_empty() {
            write_line!(output, "{call_str}");
        }
        write_line!(output, "{} = struct{{}}{{}}", var);
    }

    fn emit_assign(
        &mut self,
        output: &mut String,
        expression: &Expression,
        var: &str,
        target_ty: Option<&Type>,
    ) {
        let ty = expression.get_type();
        let is_fallible = ty.is_result() || ty.is_option();
        if is_fallible {
            self.emit_option_result_assignment(output, var, target_ty, expression);
            return;
        }

        if let Expression::Loop {
            body, needs_label, ..
        } = expression
        {
            self.push_loop(var);
            self.emit_labeled_loop(output, "for {\n", body, *needs_label);
            self.pop_loop();
            return;
        }

        if let Expression::Block { items, .. } = expression
            && items.len() > 1
        {
            output.push_str("{\n");
            self.emit_block_to_var_with_braces(output, expression, var, target_ty, true);
            output.push_str("}\n");
            return;
        }

        self.emit_block_to_var_with_braces(output, expression, var, target_ty, false);
    }

    fn emit_plain_assign(
        &mut self,
        output: &mut String,
        target_var: &str,
        expression: &Expression,
    ) {
        let expression_string = self.emit_operand(output, expression, ExpressionContext::value());
        write_line!(output, "{} = {}", target_var, expression_string);
    }

    fn emit_option_result_assignment(
        &mut self,
        output: &mut String,
        target_var: &str,
        target_ty: Option<&Type>,
        expression: &Expression,
    ) {
        let ty = target_ty
            .filter(|t| t.is_option() || t.is_result())
            .cloned()
            .unwrap_or_else(|| expression.get_type());
        let Some(fallible) = Fallible::from_type(&ty) else {
            self.emit_plain_assign(output, target_var, expression);
            return;
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
                        self.emit_plain_assign(output, target_var, expression);
                        return;
                    }
                };
                let mut fe = FallibleEmitter::new(self, &fallible);
                if kind == Some(ConstructorKind::Success)
                    || (kind == Some(ConstructorKind::Failure)
                        && fallible.err_constructor_takes_arg())
                {
                    let arg = fe.emitter.emit_composite_value(
                        output,
                        &args[0],
                        ExpressionContext::value(),
                    );
                    let call_str = fe.format_constructor_call(constructor_name, Some(&arg));
                    write_line!(output, "{} = {}", target_var, call_str);
                } else {
                    let call_str = fe.format_constructor_call(constructor_name, None);
                    write_line!(output, "{} = {}", target_var, call_str);
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(actual_expression)
                    == Some(ConstructorKind::Failure)
                {
                    let mut fe = FallibleEmitter::new(self, &fallible);
                    let call_str = fe.format_constructor_call(fallible.err_constructor(), None);
                    write_line!(output, "{} = {}", target_var, call_str);
                } else {
                    self.emit_plain_assign(output, target_var, expression);
                }
            }
            _ => {
                self.emit_block_to_var_with_braces(output, expression, target_var, None, false);
            }
        }
    }

    fn emit_block_to_var_with_braces(
        &mut self,
        output: &mut String,
        expression: &Expression,
        var: &str,
        target_ty: Option<&Type>,
        has_go_braces: bool,
    ) {
        let is_block = matches!(expression, Expression::Block { .. });
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };

        self.enter_block_scope(is_block, has_go_braces);

        if let Some((last, rest)) = items.split_last() {
            let is_new_target = self.scope.try_acquire_assign_target(var);
            for item in rest {
                self.emit_statement(output, item);
            }
            self.emit_tail_to_var(output, last, var, target_ty);
            if is_new_target {
                self.scope.release_assign_target(var);
            }
        }

        self.exit_block_scope(is_block, has_go_braces);
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

    fn emit_tail_to_var(
        &mut self,
        output: &mut String,
        last: &Expression,
        var: &str,
        target_ty: Option<&Type>,
    ) {
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
            self.emit_statement(output, last);
            return;
        }
        if last.get_type().is_never() {
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }
        if is_unit_call(last) {
            self.emit_unit_call_into_var(output, last, var);
            return;
        }
        if self.emit_append_to_var(output, var, last) {
            return;
        }
        if matches!(
            last,
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. }
        ) {
            self.emit_branching_directly(
                output,
                last,
                &BodyPlace::Assign {
                    var: var.to_string(),
                    target_ty: target_ty.cloned(),
                },
            );
            return;
        }
        let expression_string = self.emit_value(output, last, ExpressionContext::value());
        let expression_string =
            self.apply_type_coercion(output, target_ty, last, expression_string);
        write_line!(output, "{} = {}", var, expression_string);
    }

    fn emit_append_to_var(&mut self, output: &mut String, var: &str, last: &Expression) -> bool {
        let Expression::Call {
            expression: func,
            args,
            spread,
            ..
        } = last
        else {
            return false;
        };
        if !self.is_slice_append_or_extend(func) {
            return false;
        }

        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = func.as_ref()
        else {
            return true;
        };

        let is_extend = member == "extend";
        let unwrapped = receiver.unwrap_parens();
        let receiver_is_lvalue =
            is_lvalue_chain(unwrapped) && !self.contains_newtype_access(unwrapped);

        if receiver_is_lvalue {
            let receiver_lv = self.emit_left_value_capturing(output, unwrapped, false);
            let args_str =
                self.emit_append_args(output, func, args, (**spread).as_ref(), is_extend);
            write_line!(output, "{} = append({}, {})", var, receiver_lv, args_str);
        } else {
            let value_str = self.emit_value(output, last, ExpressionContext::value());
            write_line!(output, "{} = {}", var, value_str);
        }

        true
    }

    fn is_slice_append_or_extend(&self, func: &Expression) -> bool {
        if let Expression::DotAccess {
            expression, member, ..
        } = func
            && (member == "append" || member == "extend")
        {
            return expression.get_type().has_name("Slice");
        }
        false
    }

    fn emit_append_args(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        is_extend: bool,
    ) -> String {
        let stages: Vec<EmittedExpression> = args
            .iter()
            .map(|a| self.stage_composite(a, ExpressionContext::value()))
            .collect();
        let combine = Self::variadic_combine_for(function, spread, 0);
        let emitted_args =
            self.sequence_with_spread(output, stages, spread, false, "_arg", combine);
        let args_str = emitted_args.join(", ");
        let suffix = if is_extend { "..." } else { "" };
        format!("{}{}", args_str, suffix)
    }

    fn emit_block_to_tail(
        &mut self,
        output: &mut String,
        expression: &Expression,
        return_ctx: &ReturnContext,
    ) {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            std::slice::from_ref(expression)
        };

        let Some((last, rest)) = try_elide_tail_let(items).or_else(|| items.split_last()) else {
            return;
        };

        for item in rest {
            self.emit_statement(output, item);
        }

        let return_span = last.get_span();

        let last = if let Expression::Return { expression, .. } = last {
            expression.as_ref()
        } else {
            last
        };

        if last.get_type().is_unit() {
            if !matches!(last, Expression::Unit { .. }) {
                self.emit_statement(output, last);
            }
            return;
        }

        if last.get_type().is_never() {
            let directive = self.maybe_line_directive(&return_span);
            output.push_str(&directive);
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }

        let directive = self.maybe_line_directive(&return_span);
        match last {
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                output.push_str(&directive);
                self.emit_branching_directly(output, last, &BodyPlace::Return(return_ctx));
            }
            _ => {
                output.push_str(&directive);
                if self.emit_wrapped_return(output, last, return_ctx) {
                    return;
                }
                let expression_string = self.emit_tail_value(output, last);
                let return_ty = return_ctx.ty();
                let expression_string =
                    self.apply_type_coercion(output, return_ty, last, expression_string);
                write_line!(output, "return {}", expression_string);
            }
        }
    }

    /// Emit `last` as a tail value: a `Tuple` literal renders its slots with
    /// return-slot widening; everything else routes through `emit_value`.
    fn emit_tail_value(&mut self, output: &mut String, last: &Expression) -> String {
        if let Expression::Tuple { elements, ty, .. } = last {
            self.emit_tuple_value(output, elements, ty, true)
        } else {
            self.emit_value(output, last, ExpressionContext::value())
        }
    }

    pub(crate) fn emit_body_to_place(
        &mut self,
        output: &mut String,
        expression: &Expression,
        place: &BodyPlace,
    ) {
        match place {
            BodyPlace::Statement => self.emit_block(output, expression),
            BodyPlace::Assign { var, target_ty } => {
                let is_fallible =
                    expression.get_type().is_result() || expression.get_type().is_option();
                if is_fallible {
                    self.emit_option_result_assignment(output, var, target_ty.as_ref(), expression);
                } else {
                    self.emit_block_to_var_with_braces(
                        output,
                        expression,
                        var,
                        target_ty.as_ref(),
                        false,
                    );
                }
            }
            BodyPlace::Return(ctx) => self.emit_block_to_tail(output, expression, ctx),
        }
    }

    /// Function-body tail emission: returning tails dispatch on value vs
    /// branching shape; non-returning tails (statement-only / unit / never)
    /// fall back to a typed zero-value return when the signature still
    /// expects a return value.
    fn emit_function_returning_tail(
        &mut self,
        output: &mut String,
        last: &Expression,
        return_ctx: &ReturnContext,
    ) {
        let is_statement_only = matches!(
            last,
            Expression::Assignment { .. } | Expression::Let { .. } | Expression::Const { .. }
        );

        let needs_return = !matches!(last, Expression::Return { .. })
            && !is_statement_only
            && !last.get_type().is_unit()
            && !last.get_type().is_never();

        if !needs_return {
            self.emit_non_returning_function_tail(output, last, is_statement_only, return_ctx);
            return;
        }

        if crate::types::abi_transition::try_emit_lowered_tail_return(
            self, output, last, return_ctx,
        ) {
            return;
        }
        if self.emit_wrapped_return(output, last, return_ctx) {
            return;
        }
        self.emit_returning_function_tail(output, last, return_ctx);
    }

    fn emit_non_returning_function_tail(
        &mut self,
        output: &mut String,
        last: &Expression,
        is_statement_only: bool,
        return_ctx: &ReturnContext,
    ) {
        self.emit_statement(output, last);
        if last.get_type().is_never() && !Self::is_go_never(last) {
            output.push_str("panic(\"unreachable\")\n");
        }
        let last_is_unit_expr = !is_statement_only
            && !matches!(last, Expression::Return { .. })
            && last.get_type().is_unit();
        if (is_statement_only || last_is_unit_expr)
            && let Some(return_ty) = return_ctx.ty().filter(|ty| !ty.is_unit())
        {
            let return_ty = return_ty.clone();
            self.emit_zero_return(output, &return_ty);
        }
    }

    fn emit_returning_function_tail(
        &mut self,
        output: &mut String,
        last: &Expression,
        return_ctx: &ReturnContext,
    ) {
        if !requires_temp_var(last) {
            let expression = self.emit_tail_value(output, last);
            let return_ty = return_ctx.ty();
            let expression = self.apply_type_coercion(output, return_ty, last, expression);
            if !expression.is_empty() {
                write_line!(output, "return {}", expression);
            }
            return;
        }
        match last {
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                self.emit_branching_directly(output, last, &BodyPlace::Return(return_ctx));
            }
            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before emit")
            }
            Expression::Block { .. } | Expression::Loop { .. } | Expression::Propagate { .. } => {
                let expression = self.emit_operand(output, last, ExpressionContext::value());
                if !expression.is_empty() {
                    write_line!(output, "return {}", expression);
                }
            }
            _ => unreachable!("requires_temp_var returned true for unexpected expression"),
        }
    }

    /// Emit a `let identifier = value` binding. `raw_go_name == None` signals
    /// an unused binding (registered as `_` in scope). Dispatches on value
    /// shape: propagate `?` short-circuit, unit-call statement form, temp
    /// vs direct assignment.
    pub(crate) fn emit_let_value(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
        binding_ty: &Type,
        mutable: bool,
    ) {
        if matches!(value, Expression::Propagate { .. }) {
            self.emit_let_propagate(output, identifier, raw_go_name, value, binding_ty);
            return;
        }
        if is_unit_call(value) {
            self.emit_let_unit_call(output, identifier, raw_go_name, value);
            return;
        }
        let needs_temp = requires_temp_var(value);
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind(identifier, "_");
            if needs_temp {
                self.emit_let_temp(output, "_", value, binding_ty);
            } else {
                self.emit_discard(output, value);
            }
            return;
        };
        if needs_temp {
            let go_identifier = crate::escape_reserved(raw_go_name);
            if self.is_declared(&go_identifier) || expression_contains_binding(value, identifier) {
                let fresh = self.fresh_var(Some(identifier));
                self.emit_let_temp(output, &fresh, value, binding_ty);
                self.scope.bind(identifier, &fresh);
            } else {
                self.scope.bind(identifier, raw_go_name);
                self.emit_let_temp(output, &go_identifier, value, binding_ty);
            }
            return;
        }
        self.emit_let_direct(output, identifier, raw_go_name, value, binding_ty, mutable);
    }

    /// `let x = expr?`: propagate handles its own value emission and may
    /// declare `var x T` first when the binding widens to an interface.
    fn emit_let_propagate(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
        binding_ty: &Type,
    ) {
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind(identifier, "_");
            let return_ctx = self.scope_return_context_fallback().clone();
            self.emit_propagate_to_let(output, "_", value, &return_ctx);
            return;
        };
        let go_identifier = self.choose_let_go_name(identifier, raw_go_name, false);
        let widens_to_interface =
            self.facts.is_interface(binding_ty) && *binding_ty != value.get_type();
        if widens_to_interface {
            let var_ty = self.go_type_as_string(binding_ty);
            write_line!(output, "var {} {}", go_identifier, var_ty);
            self.declare(&go_identifier);
        }
        let return_ctx = self.scope_return_context_fallback().clone();
        self.emit_propagate_to_let(output, &go_identifier, value, &return_ctx);
        self.scope.bind(identifier, &go_identifier);
        self.try_declare(&go_identifier);
    }

    /// `let x = foo()` where `foo()` returns unit: emit the call as a
    /// statement, then declare the binding as `struct{}{}`.
    fn emit_let_unit_call(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
    ) {
        let value_expression = self.emit_value(output, value, ExpressionContext::value());
        write_line!(output, "{}", value_expression);
        let Some(raw_go_name) = raw_go_name else {
            return;
        };
        let escaped = crate::escape_reserved(raw_go_name);
        if self.is_declared(&escaped) {
            let fresh = self.fresh_var(Some(identifier));
            self.declare(&fresh);
            write_line!(output, "{} := struct{{}}{{}}", fresh);
            self.scope.bind(identifier, &fresh);
        } else {
            let go_identifier = self.scope.bind(identifier, raw_go_name);
            self.try_declare(&go_identifier);
            write_line!(output, "{} := struct{{}}{{}}", go_identifier);
        }
    }

    fn emit_let_direct(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: &str,
        value: &Expression,
        binding_ty: &Type,
        mutable: bool,
    ) {
        if !mutable
            && self.try_emit_let_into_wrapper_slot(
                output,
                identifier,
                raw_go_name,
                value,
                binding_ty,
            )
        {
            return;
        }

        let value_expression = self.emit_value(output, value, ExpressionContext::value());
        let coercion = Coercion::resolve(
            self,
            &value.get_type(),
            binding_ty,
            CoercionDirection::Internal,
        );
        let value_expression = coercion.apply(self, output, value_expression);
        let value_expression = maybe_clone_subslice(self, value, mutable, value_expression);

        let go_identifier = self.scope.bind(identifier, raw_go_name);
        let is_new = self.try_declare(&go_identifier);

        if !is_new || self.scope.is_active_assign_target(&go_identifier) {
            let fresh = self.fresh_var(Some(identifier));
            self.scope.bind(identifier, &fresh);
            self.try_declare(&fresh);
            write_line!(output, "{} := {}", fresh, value_expression);
        } else if needs_explicit_type_declaration(self, value, binding_ty) {
            let var_ty = self.go_type_as_string(binding_ty);
            write_line!(
                output,
                "var {} {} = {}",
                go_identifier,
                var_ty,
                value_expression
            );
        } else {
            write_line!(output, "{} := {}", go_identifier, value_expression);
        }
    }

    /// Route a slot-style Go-interop wrapper to write into the let's chosen
    /// Go name, eliminating the `name := result_N` alias. Returns true on hit.
    fn try_emit_let_into_wrapper_slot(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: &str,
        value: &Expression,
        binding_ty: &Type,
    ) -> bool {
        let go_identifier = crate::escape_reserved(raw_go_name);
        if self.is_declared(&go_identifier)
            || self.scope.is_active_assign_target(&go_identifier)
            || self.scope.has_binding_for_go_name(&go_identifier)
        {
            return false;
        }
        if value.get_type() != *binding_ty {
            return false;
        }
        let Some(strategy) = self.resolve_go_call_strategy(value) else {
            return false;
        };
        if matches!(
            strategy,
            crate::calls::go_interop::GoCallStrategy::Tuple { .. }
        ) {
            return false;
        }
        let target = crate::calls::go_interop::WrapperTarget::Slot(&go_identifier);
        if self
            .emit_go_wrapped_call_to(output, value, &strategy, binding_ty, target)
            .is_none()
        {
            return false;
        }
        // `open_wrapper_slot` / `emit_simple_wrapper_value` already declared
        // `go_identifier`; only the binding from the user-name still needs setup.
        self.scope.bind(identifier, go_identifier.as_ref());
        true
    }

    fn emit_let_temp(
        &mut self,
        output: &mut String,
        name: &str,
        value: &Expression,
        binding_ty: &Type,
    ) {
        if !self.is_declared(name) {
            self.emit_let_temp_var_decl(output, name, value, binding_ty);
            self.try_declare(name);
        }
        self.emit_to_place(
            output,
            value,
            ValuePlace::Assign {
                var: name,
                target_ty: Some(binding_ty),
            },
        );
    }

    fn emit_let_temp_var_decl(
        &mut self,
        output: &mut String,
        name: &str,
        value: &Expression,
        binding_ty: &Type,
    ) {
        if name == "_" {
            return;
        }
        let resolved_ty = resolve_let_temp_decl_ty(self, value, binding_ty);
        let has_variable_ok_ty = matches!(
            value,
            Expression::TryBlock { .. } | Expression::RecoverBlock { .. }
        ) && !resolved_ty.is_variable()
            && resolved_ty.ok_type().is_variable();

        let var_ty = if has_variable_ok_ty {
            if !binding_ty.is_variable() && !binding_ty.ok_type().is_variable() {
                self.go_type_as_string(binding_ty)
            } else if let Some(ctx_ty) = self.scope_return_context_fallback().ty().cloned() {
                if Fallible::from_type(&ctx_ty).is_some() {
                    self.go_type_as_string(&ctx_ty)
                } else {
                    self.go_type_as_string(&resolved_ty)
                }
            } else {
                self.go_type_as_string(&resolved_ty)
            }
        } else {
            self.go_type_as_string(&resolved_ty)
        };
        write_line!(output, "var {} {}", name, var_ty);
    }

    fn emit_to_operand_temp(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ty: &Type,
    ) -> String {
        if let Expression::Block { items, .. } = expression {
            if ty.is_never() || ty.is_unit() || matches!(ty, Type::Var { .. } | Type::Forall { .. })
            {
                self.emit_block(output, expression);
                return String::new();
            }
            let result_var = self.declare_result_var(output, ty);
            let needs_braces = items.len() > 1;
            if needs_braces {
                output.push_str("{\n");
            }
            self.emit_block_to_var_with_braces(output, expression, &result_var, None, needs_braces);
            if needs_braces {
                output.push_str("}\n");
            }
            return result_var;
        }
        if let Expression::Loop {
            body, needs_label, ..
        } = expression
        {
            let result_var = self.declare_result_var(output, ty);
            self.push_loop(result_var.clone());
            self.emit_labeled_loop(output, "for {\n", body, *needs_label);
            self.pop_loop();
            return result_var;
        }
        let result_var = self.declare_result_var(output, ty);
        self.emit_to_place(
            output,
            expression,
            ValuePlace::Assign {
                var: &result_var,
                target_ty: Some(ty),
            },
        );
        result_var
    }

    fn emit_try_tail(&mut self, output: &mut String, last: &Expression, fallible: &Fallible) {
        if last.diverges().is_some() || last.get_type().is_never() {
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }

        let is_statement_only = matches!(
            last,
            Expression::Let { .. }
                | Expression::Const { .. }
                | Expression::Assignment { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::For { .. }
                | Expression::Loop { .. }
        );
        let is_unit_call = is_unit_call(last);
        if is_statement_only || is_unit_call {
            self.emit_statement(output, last);
            self.emit_try_unit_return(output, fallible);
            return;
        }

        let final_expression = self.emit_value(output, last, ExpressionContext::value());
        if final_expression.is_empty() {
            self.emit_try_unit_return(output, fallible);
        } else {
            self.emit_try_success_return(output, &final_expression, fallible);
        }
    }

    fn emit_recover_tail(&mut self, output: &mut String, last: &Expression, fallible: &Fallible) {
        let item_ty = last.get_type();
        if item_ty.is_never() {
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }
        if item_ty.is_unit() || item_ty.is_variable() {
            self.emit_statement(output, last);
            self.emit_zero_return(output, fallible.ok_ty());
            return;
        }
        let expression = self.emit_value(output, last, ExpressionContext::value());
        write_line!(output, "return {}", expression);
    }

    fn emit_break_value(&mut self, output: &mut String, val: &Expression) {
        let val_str = self.emit_value(output, val, ExpressionContext::value());
        if val_str.is_empty() && matches!(val, Expression::Propagate { .. }) {
            return;
        }
        if let Some(var) = self.current_loop_result_var().map(str::to_string) {
            let is_unit_call = is_unit_call(val);
            if is_unit_call {
                if !val_str.is_empty() {
                    write_line!(output, "{}", val_str);
                }
                write_line!(output, "{} = struct{{}}{{}}", var);
            } else if !val_str.is_empty() {
                write_line!(output, "{} = {}", var, val_str);
            }
        } else if !val_str.is_empty() {
            write_line!(output, "_ = {}", val_str);
        }
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "break {}", label);
        } else {
            output.push_str("break\n");
        }
    }
}
