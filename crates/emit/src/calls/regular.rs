use crate::abi::is_tagged_shape_fn_value;
use crate::calls::dispatch::{
    CallArgShape, all_type_params_inferrable, is_prelude_variant_constructor,
};

use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableParamAbi, CallableReturnAbi};
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::transition::{emit_fn_arg_shape_adapter, emit_lisette_callback_wrapper};
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::VariadicCombine;
use crate::names::generics::extract_type_mapping;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::{ArgumentPlan, CallPlan, CallableOrigin, ResolvedCallee};
use crate::plan::values::{
    CaptureBoundary, EvaluationEffect, GoExpression, SequencedValues, ValuePlan,
};
use crate::utils::reads_mutable_operand;
use crate::write_line;
use syntax::ast::{Expression, Literal};
use syntax::types::Type;

struct CallArgsContext<'plan, 'facts> {
    plan: &'plan CallPlan<'facts>,
    /// Suppresses the Go-fn identity short-circuit on fn-typed params
    /// dispatching into prelude generic helpers (e.g. `OptionAndThen`).
    spread: Option<&'plan Expression>,
    wrap_spread_to_any: bool,
    combine_variadic: Option<VariadicCombine>,
    capture_boundary: CaptureBoundary,
}

/// Escape-aware close-quote search; plain `find` would collide with `\"` inside the literal.
fn find_go_string_literal_close(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn lowers_to_bare_sprintf(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::FormatString(_),
            ..
        } => true,
        Expression::Call {
            expression: callee, ..
        } => callee.unwrap_parens().as_dotted_path().as_deref() == Some("fmt.Sprintf"),
        _ => false,
    }
}

/// Collapse redundant fmt wrappers:
/// - `fmt.Print{ln}(fmt.Sprintf(...))` → `fmt.Printf(..., "\n")`
/// - `fmt.Print{ln}(fmt.Sprint(x))` → `fmt.Print{ln}(x)`
fn collapse_fmt_print(
    function_string: &str,
    args: &[Expression],
    args_strings: &[String],
    call_str: String,
) -> String {
    if function_string != "fmt.Print" && function_string != "fmt.Println" {
        return call_str;
    }
    if args_strings.len() != 1 {
        return call_str;
    }
    let arg = &args_strings[0];

    if let Some(arg_expression) = args.first()
        && lowers_to_bare_sprintf(arg_expression)
        && let Some(inner) = arg
            .strip_prefix("fmt.Sprintf(")
            .and_then(|s| s.strip_suffix(')'))
    {
        let suffix = if function_string == "fmt.Println" {
            "\\n"
        } else {
            ""
        };
        if suffix.is_empty() {
            return format!("fmt.Printf({})", inner);
        }
        if let Some(close_quote) = find_go_string_literal_close(inner) {
            let format_open = &inner[..close_quote];
            let close_and_rest = &inner[close_quote..];
            return format!("fmt.Printf({}{}{})", format_open, suffix, close_and_rest);
        }
        return call_str;
    }

    if let Some(arg_expression) = args.first()
        && let Expression::Call {
            expression: inner_callee,
            args: inner_args,
            spread,
            ..
        } = arg_expression.unwrap_parens()
        && spread.is_none()
        && inner_args.len() == 1
        && inner_callee.unwrap_parens().as_dotted_path().as_deref() == Some("fmt.Sprint")
        && let Some(inner) = arg
            .strip_prefix("fmt.Sprint(")
            .and_then(|s| s.strip_suffix(')'))
    {
        return format!("{}({})", function_string, inner);
    }

    call_str
}

impl<'a> Planner<'a> {
    /// Lower a regular call: typed setup plus the call value text.
    pub(super) fn lower_regular_call(
        &mut self,
        call_expression: &Expression,
        call_plan: &CallPlan<'a>,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let Expression::Call {
            expression: callee,
            args,
            resolved_type_args,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("lower_regular_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = (**spread).as_ref();

        if let Some(go_name) = self.get_callee_go_name(function).map(str::to_string) {
            let stages: Vec<ValuePlan> = args
                .iter()
                .map(|a| self.stage_operand(a, ExpressionContext::value()))
                .collect();
            let wrap_to_any = spread_needs_any_wrap(function, spread);
            let combine = call_plan.variadic_combine(0);
            let sequenced = self.sequence_with_spread_values(
                stages,
                spread,
                wrap_to_any,
                "_arg",
                combine,
                expression_ctx.capture_boundary(),
            );
            let effect = self.regular_call_effect(function, sequenced.effect);
            let (setup, args_strings) = sequenced.into_rendered();
            let expression = GoExpression::call(
                GoExpression::opaque(go_name),
                args_strings.into_iter().map(GoExpression::opaque).collect(),
            );
            return if self.callee_lowers_to_type_construction(function) {
                ValuePlan::observable_call(setup, expression, effect)
            } else {
                ValuePlan::plain_call(setup, expression, effect)
            };
        }

        let callee_staged = self.stage_operand(function, expression_ctx.callee());
        let callee_effect = callee_staged.evaluation.effect;
        let (mut setup, mut function_string) = callee_staged.into_parts();

        if function.deref_inner().is_some() {
            function_string = format!("({})", function_string);
        }

        let type_args_string = self.resolve_call_type_args(
            function,
            &call_plan.resolved,
            resolved_type_args,
            call_ty,
            CallArgShape {
                value_count: args.len(),
                has_spread: spread.is_some(),
            },
            &mut function_string,
            expression_ctx,
        );

        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            wrap_spread_to_any: spread_needs_any_wrap(function, spread),
            combine_variadic: call_plan.variadic_combine(0),
            capture_boundary: expression_ctx.capture_boundary(),
        };
        let sequenced_args = self.emit_call_args(args, &args_ctx);
        let args_effect = sequenced_args.effect;
        let (args_setup, args_strings) = sequenced_args.into_rendered();

        let callee_needs_pin = setup.is_empty()
            && type_args_string.is_empty()
            && reads_mutable_operand(function)
            && (!args_setup.is_empty()
                || (!matches!(function, Expression::Call { .. }) && args_effect.has_call()));
        if callee_needs_pin {
            function_string =
                self.hoist_tmp_value_statement(&mut setup, "callee", &function_string);
        }

        let call_str = format!(
            "{}{}({})",
            function_string,
            type_args_string,
            args_strings.join(", ")
        );
        let call_str = collapse_fmt_print(&function_string, args, &args_strings, call_str);

        setup.extend(args_setup);

        let effect = self
            .regular_call_effect(function, args_effect)
            .combine(callee_effect);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(
                setup,
                GoExpression::opaque_with_deferred_evaluation(call_str, true),
                effect,
            )
        } else {
            ValuePlan::plain_call(
                setup,
                GoExpression::opaque_with_deferred_evaluation(call_str, true),
                effect,
            )
        }
    }

    fn regular_call_effect(
        &self,
        function: &Expression,
        argument_effect: EvaluationEffect,
    ) -> EvaluationEffect {
        if self.is_pure_constructor_callee(function) {
            EvaluationEffect::PureCall.combine(argument_effect)
        } else {
            EvaluationEffect::EffectfulCall
        }
    }

    fn callee_collapsed_recipe(&self, callee: &ResolvedCallee<'_>) -> Option<String> {
        callee.id.as_deref()?;
        callee
            .definition?
            .go_type_param_recipe()
            .map(str::to_string)
    }

    /// True when Go can infer every type parameter of a collapsed callee from
    /// its value parameters. A var present only in the return type, or only in a
    /// trailing `VarArgs<T>` the call leaves empty, is not inferable, so the
    /// recipe must be rebuilt.
    fn collapsed_callee_fully_inferable(
        &self,
        callee: &ResolvedCallee<'_>,
        arg_shape: CallArgShape,
    ) -> bool {
        let Some(Type::Forall { vars, body }) = callee.declared.as_ref() else {
            return false;
        };
        let Type::Function(f) = body.as_ref() else {
            return false;
        };
        all_type_params_inferrable(vars, &f.params, 0, arg_shape)
    }

    fn reconstruct_collapsed_call_type_args(
        &mut self,
        callee: &ResolvedCallee<'_>,
        recipe: &str,
    ) -> Option<String> {
        let definition_ty = callee.declared.clone()?;
        let Type::Forall { body, .. } = definition_ty else {
            return None;
        };
        let mut mapping = rustc_hash::FxHashMap::default();
        extract_type_mapping(&body, &callee.instantiated, &mut mapping);
        self.reconstruct_collapsed_type_args(recipe, &mapping)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_call_type_args(
        &mut self,
        function: &Expression,
        callee: &ResolvedCallee<'_>,
        type_args: &[Type],
        call_ty: Option<&Type>,
        arg_shape: CallArgShape,
        function_string: &mut String,
        ctx: ExpressionContext<'_>,
    ) -> String {
        let has_value_args = arg_shape.value_count > 0 || arg_shape.has_spread;
        if let Some(recipe) = self.callee_collapsed_recipe(callee) {
            if has_value_args && self.collapsed_callee_fully_inferable(callee, arg_shape) {
                return String::new();
            }
            return self
                .reconstruct_collapsed_call_type_args(callee, &recipe)
                .unwrap_or_default();
        }

        let mut type_args_string = self.format_type_args(type_args);

        let slot_ty = ctx.expected_slot_type();

        if type_args_string.is_empty()
            && let Some(inferred) =
                self.infer_return_only_type_args(function, callee.declared.as_ref(), arg_shape)
        {
            type_args_string = match slot_ty {
                Some(t) => self.prelude_container_type_args(t).unwrap_or(inferred),
                None => inferred,
            };
        }

        if type_args_string.is_empty() && is_prelude_variant_constructor(function) {
            let mut candidate = call_ty.and_then(|t| self.prelude_container_type_args(t));
            if candidate.is_none() {
                candidate = slot_ty.and_then(|t| self.prelude_container_type_args(t));
            }
            type_args_string = candidate.unwrap_or_default();
        }

        if !type_args_string.is_empty()
            && let Some(bracket_start) = function_string.find('[')
        {
            function_string.truncate(bracket_start);
        }

        type_args_string
    }

    /// Stage and sequence the call arguments, returning the structured setup
    /// (per-arg setup plus eval-order temp captures) and the rendered arg
    /// values. The caller flushes the setup before the call expression.
    fn emit_call_args(
        &mut self,
        args: &[Expression],
        ctx: &CallArgsContext<'_, '_>,
    ) -> SequencedValues {
        let mut stages: Vec<ValuePlan> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.lower_call_arg(arg, i, ctx))
            .collect();

        if let Some(spread) = ctx.spread
            && let Some(stage) =
                self.lower_variadic_spread_slot_bridge(spread, ctx.plan.resolved.abi.params.last())
        {
            stages.push(stage);
            let spread_index = stages.len() - 1;
            let mut sequenced = self.sequence_values(stages, ctx.capture_boundary, "_arg");
            self.finalize_spread_stage(
                &mut sequenced.values,
                spread_index,
                ctx.wrap_spread_to_any,
                ctx.combine_variadic.clone(),
            );
            return sequenced;
        }

        self.sequence_args_with_spread_adapter_values(
            stages,
            ctx.spread,
            ctx.plan
                .resolved
                .declared
                .as_ref()
                .and_then(|ty| ty.unwrap_forall().get_function_params()),
            ctx.wrap_spread_to_any,
            ctx.combine_variadic.clone(),
            ctx.capture_boundary,
        )
    }

    /// Classify and lower a single call argument: dispatch is plan-driven and
    /// returns typed setup. The plain `Direct` / `TaggedGoLowering` paths produce
    /// typed `TempBind` setup; adapter and slot-bridge paths retain their own
    /// structured setup until sequencing.
    fn lower_call_arg(
        &mut self,
        arg: &Expression,
        index: usize,
        ctx: &CallArgsContext<'_, '_>,
    ) -> ValuePlan {
        let param = ctx.plan.resolved.abi.param(index);
        let effective_param_ty = param.map(|param| &param.instantiated);
        let generic_param_ty = param.and_then(|param| param.declared.as_ref());
        let declared_param_ty = generic_param_ty;

        let plan = ctx
            .plan
            .arguments
            .get(index)
            .expect("CallPlan has one argument plan per argument");

        match plan {
            ArgumentPlan::GoCallbackAdapter {
                source,
                target,
                transition,
            } => self.lower_callback_wrapper(
                arg,
                effective_param_ty.expect("GoCallbackAdapter requires effective_param_ty"),
                source,
                target,
                *transition,
            ),
            ArgumentPlan::LoweredFnShapeAdapter => self
                .lower_adapt_lowered_fn_arg_shape(
                    arg,
                    generic_param_ty.expect("LoweredFnShapeAdapter requires generic_param_ty"),
                )
                .expect("detect_lowered_fn_arg_shape ensures Some"),
            ArgumentPlan::GoSlotBridge => self
                .lower_go_slot_bridge(arg, param.expect("GoSlotBridge requires a parameter ABI")),
            ArgumentPlan::TaggedGoLowering => {
                let target =
                    effective_param_ty.expect("TaggedGoLowering requires effective_param_ty");
                let arg_ctx = direct_arg_emit_ctx(Some(target), true);
                let argument = self.lower_composite_value(arg, arg_ctx);
                argument.map_rendered_as_computed(|setup, value, _contains_deferred_evaluation| {
                    let lowered = self.emit_lower_arg_to_tagged(setup, &value, target);
                    GoExpression::opaque_with_deferred_evaluation(lowered, true)
                })
            }
            ArgumentPlan::Direct => {
                self.lower_direct_arg(arg, ctx, effective_param_ty, declared_param_ty)
            }
        }
    }

    /// Pre-plan adaptations for a single argument. Mirrors the prior
    /// `try_emit_*` chain in order; the first hit wins. Returns `Direct` for
    /// the fallback path (which still handles tagged-Go suppression inline).
    pub(crate) fn plan_argument(
        &self,
        arg: &Expression,
        callee: &ResolvedCallee<'_>,
        param: Option<&CallableParamAbi>,
    ) -> ArgumentPlan {
        let effective_param_ty = param.map(|param| &param.instantiated);
        let declared_param_ty = param.and_then(|param| param.declared.as_ref());
        if matches!(callee.origin, CallableOrigin::GoInterop)
            && let Some((source, target, transition)) =
                self.detect_callback_wrapper(arg, effective_param_ty)
        {
            return ArgumentPlan::GoCallbackAdapter {
                source,
                target,
                transition,
            };
        }
        if self
            .detect_lowered_fn_arg_shape(arg, declared_param_ty)
            .is_some()
        {
            return ArgumentPlan::LoweredFnShapeAdapter;
        }
        if param.is_some_and(|param| self.argument_needs_slot_bridge(arg, param)) {
            return ArgumentPlan::GoSlotBridge;
        }
        let suppress = would_suppress_tagged_go(callee, declared_param_ty);
        if suppress
            && self
                .detect_lower_arg_to_tagged(arg, effective_param_ty)
                .is_some()
        {
            return ArgumentPlan::TaggedGoLowering;
        }
        ArgumentPlan::Direct
    }

    fn lower_direct_arg(
        &mut self,
        arg: &Expression,
        ctx: &CallArgsContext<'_, '_>,
        effective_param_ty: Option<&Type>,
        declared_param_ty: Option<&Type>,
    ) -> ValuePlan {
        let suppress = would_suppress_tagged_go(&ctx.plan.resolved, declared_param_ty);
        let arg_ctx = direct_arg_emit_ctx(effective_param_ty, suppress);
        let argument = self.lower_composite_value(arg, arg_ctx);
        argument.map_rendered_as_computed(|setup, value, contains_deferred_evaluation| {
            let final_value = match effective_param_ty {
                Some(target) => {
                    let coercion = CoercionPlan::internal(self, &arg.get_type(), target);
                    let (coercion_setup, coerced) = coercion.lower(self, value);
                    setup.extend(coercion_setup);
                    coerced
                }
                None => value,
            };
            GoExpression::opaque_with_deferred_evaluation(final_value, contains_deferred_evaluation)
        })
    }

    /// Adapt a lowered-return fn arg when its shape disagrees with the
    /// callee's generic-param shape.
    pub(crate) fn try_adapt_lowered_fn_arg_shape(
        &mut self,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<ValuePlan> {
        self.detect_lowered_fn_arg_shape(arg, generic_param_ty)?;
        self.lower_adapt_lowered_fn_arg_shape(arg, generic_param_ty.unwrap())
    }

    /// Detect whether `arg`'s fn-shape disagrees with the callee's generic
    /// param shape (Lisette callback adapter trigger). Pure detection.
    fn fn_arg_shapes(
        &self,
        arg: &Expression,
        raw_param_ty: &Type,
    ) -> Option<(CallableReturnAbi, Type, CallableReturnAbi)> {
        let variadic_inner = (raw_param_ty.get_name() == Some("VarArgs"))
            .then(|| raw_param_ty.inner())
            .flatten();
        let param_ty = variadic_inner.as_ref().unwrap_or(raw_param_ty);
        let param_fn = self
            .facts
            .resolve_to_function_type(param_ty.unwrap_forall())?;
        let param_ret = param_fn.get_function_ret()?;
        let param_abi = self
            .classify_direct_emission(param_ret)
            .unwrap_or_else(|| self.value_return_abi(param_ret));

        let arg_ty = arg.get_type();
        let arg_fn = self
            .facts
            .resolve_to_function_type(arg_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_abi = self.classify_direct_emission(arg_ret)?;

        Some((param_abi, arg_fn, arg_abi))
    }

    pub(crate) fn detect_lowered_fn_arg_shape(
        &self,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<()> {
        if is_tagged_shape_fn_value(arg) {
            return None;
        }
        let raw_param_ty = generic_param_ty?;
        let (param_abi, _arg_fn, arg_abi) = self.fn_arg_shapes(arg, raw_param_ty)?;
        if param_abi == arg_abi {
            return None;
        }
        Some(())
    }

    fn lower_adapt_lowered_fn_arg_shape(
        &mut self,
        arg: &Expression,
        generic_param_ty: &Type,
    ) -> Option<ValuePlan> {
        let (param_abi, arg_fn, arg_abi) = self.fn_arg_shapes(arg, generic_param_ty)?;
        let argument = self.lower_value(arg, ExpressionContext::value());
        Some(argument.map_rendered_as_computed(|setup, value, _| {
            let mut buffer = String::new();
            let adapted =
                emit_fn_arg_shape_adapter(self, &mut buffer, &value, &arg_fn, &arg_abi, &param_abi)
                    .expect("fn_arg_shapes resolved a function signature");
            if !buffer.is_empty() {
                setup.push(LoweredStatement::RawGo(buffer));
            }
            GoExpression::opaque_with_deferred_evaluation(adapted, true)
        }))
    }

    /// Adapt `slice...` spread into a generic `VarArgs<fn(...)>` when the
    /// slice's element fn-shape disagrees with the variadic's element.
    pub(crate) fn try_emit_variadic_spread_adapter(
        &mut self,
        spread: &Expression,
        generic_params: Option<&[Type]>,
    ) -> Option<ValuePlan> {
        let generic_params = generic_params?;
        let raw_variadic = generic_params.last()?;
        if raw_variadic.get_name() != Some("VarArgs") {
            return None;
        }
        let variadic_inner = raw_variadic.inner()?;
        let param_fn = self
            .facts
            .resolve_to_function_type(variadic_inner.unwrap_forall())?;
        let param_ret = param_fn.get_function_ret()?;
        let param_abi = self
            .classify_direct_emission(param_ret)
            .unwrap_or_else(|| self.value_return_abi(param_ret));

        let spread_ty = spread.get_type();
        let element_ty = spread_ty.unwrap_forall().inner()?;
        let arg_fn = self
            .facts
            .resolve_to_function_type(element_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_abi = self.classify_direct_emission(arg_ret)?;

        if param_abi == arg_abi {
            return None;
        }

        let source = self
            .lower_value(spread, ExpressionContext::value())
            .map_rendered_as_name(|setup, source_value, _| {
                GoExpression::name(self.hoist_tmp_value_statement(setup, "src", &source_value))
            });
        let source_variable = source.rendered();

        let target_element_ret = self.render_lowered_return_ty(&param_abi, arg_ret);
        let arg_fn_params = arg_fn.get_function_params().unwrap_or(&[]);
        let param_type_strs: Vec<String> = arg_fn_params
            .iter()
            .map(|p| self.go_type_string(p))
            .collect();
        let target_element_ty = format!(
            "func({}) {}",
            param_type_strs.join(", "),
            target_element_ret
        );

        let adapted = self.fresh_var(Some("adapted"));
        self.declare(&adapted);
        let loop_cb = self.fresh_var(Some("cb"));

        let mut body = String::new();
        let closure =
            emit_fn_arg_shape_adapter(self, &mut body, &loop_cb, &arg_fn, &arg_abi, &param_abi)?;
        write_line!(body, "{}[i] = {}", adapted, closure);

        Some(
            source.map_rendered_as_name(|setup, _source_value, _contains_deferred_evaluation| {
                setup.push(LoweredStatement::RawGo(format!(
                    "{} := make([]{}, len({}))\n",
                    adapted, target_element_ty, source_variable
                )));
                setup.push(LoweredStatement::RawGo(format!(
                    "for i, {} := range {} {{\n{}}}\n",
                    loop_cb, source_variable, body
                )));
                GoExpression::name(adapted)
            }),
        )
    }

    /// Resolve the source and target callback contracts at a Go call boundary.
    pub(crate) fn detect_callback_wrapper(
        &self,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<(CallableReturnAbi, CallableReturnAbi, AbiTransition)> {
        let param_fn_ty = effective_param_ty
            .and_then(|param_ty| {
                self.facts
                    .resolve_to_function_type(param_ty.unwrap_forall())
            })
            .filter(|fn_ty| {
                let Type::Function(f) = fn_ty else {
                    return false;
                };
                f.return_type.is_result()
                    || f.return_type.is_option()
                    || f.return_type.tuple_arity().is_some_and(|a| a >= 2)
            })?;

        let Type::Function(param_f) = &param_fn_ty else {
            return None;
        };
        let target = self.classify_direct_emission(&param_f.return_type)?;
        let source = if is_tagged_shape_fn_value(arg) {
            CallableReturnAbi::Tagged
        } else {
            self.resolve_callable_value(arg)
                .map(|callee| callee.abi.result)
                .unwrap_or(CallableReturnAbi::Direct)
        };
        let transition = source.transition_to(&target);
        (!matches!(transition, AbiTransition::Identity)).then_some((source, target, transition))
    }

    fn lower_callback_wrapper(
        &mut self,
        arg: &Expression,
        effective_param_ty: &Type,
        source: &CallableReturnAbi,
        target: &CallableReturnAbi,
        transition: AbiTransition,
    ) -> ValuePlan {
        let argument = match transition {
            AbiTransition::Identity => self.lower_value(arg, ExpressionContext::value()),
            _ => self.plan_operand(
                arg,
                ExpressionContext::value().with_forced_tagged_go_function(true),
            ),
        };
        argument.map_rendered_as_computed(|setup, value, contains_deferred_evaluation| {
            let result = match transition {
                AbiTransition::Identity => value,
                AbiTransition::LowerFromTagged => {
                    let param_fn_ty = self
                        .facts
                        .resolve_to_function_type(effective_param_ty.unwrap_forall())
                        .expect("callback target resolves to a fn type");
                    emit_lisette_callback_wrapper(self, setup, &value, &param_fn_ty)
                }
                AbiTransition::WrapToTagged | AbiTransition::Reencode => {
                    let arg_fn_ty = self
                        .facts
                        .resolve_to_function_type(arg.get_type().unwrap_forall())
                        .expect("callback source resolves to a fn type");
                    let mut buffer = String::new();
                    let adapted = emit_fn_arg_shape_adapter(
                        self,
                        &mut buffer,
                        &value,
                        &arg_fn_ty,
                        source,
                        target,
                    )
                    .expect("callback ABI transition has a function signature");
                    if !buffer.is_empty() {
                        setup.push(LoweredStatement::RawGo(buffer));
                    }
                    adapted
                }
                AbiTransition::Incompatible => {
                    unreachable!("type-checked callback ABIs must describe the same result")
                }
            };
            GoExpression::opaque_with_deferred_evaluation(
                result,
                contains_deferred_evaluation || !matches!(transition, AbiTransition::Identity),
            )
        })
    }

    fn argument_slot_layout(&self, parameter: &CallableParamAbi) -> ValueLayout {
        if parameter.instantiated.get_name() == Some("VarArgs") {
            let slot_type = varargs_inner_or_self(&parameter.instantiated);
            let declared_slot = parameter.declared.as_ref().map(varargs_inner_or_self);
            declared_slot.as_ref().map_or_else(
                || self.value_layout(&slot_type, parameter.origin),
                |declared| {
                    self.value_layout_with_declaration(&slot_type, parameter.origin, declared)
                },
            )
        } else {
            parameter.layout.clone()
        }
    }

    fn argument_needs_slot_bridge(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> bool {
        let physical_source = self.go_physical_expression_layout(argument);
        let source = physical_source
            .clone()
            .unwrap_or_else(|| self.value_layout(&argument.get_type(), SlotOrigin::Lisette));
        let target = self.argument_slot_layout(parameter);
        let can_forward_physical = match (&physical_source, &target) {
            (
                Some(ValueLayout::Function { layout: source, .. }),
                ValueLayout::Function { layout: target, .. },
            ) => source.return_abi == target.return_abi,
            (Some(_), _) => true,
            (None, _) => false,
        };
        can_forward_physical || !CoercionPlan::bridge(self, &source, &target).is_identity()
    }

    fn lower_go_slot_bridge(
        &mut self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> ValuePlan {
        if argument.is_none_literal() {
            return ValuePlan::evaluated_literal(
                Vec::new(),
                "nil".to_string(),
                EvaluationEffect::PureCall,
            );
        }
        let raw_source = self.go_physical_expression_layout(argument);
        let source = raw_source
            .clone()
            .unwrap_or_else(|| self.value_layout(&argument.get_type(), SlotOrigin::Lisette));
        let target = self.argument_slot_layout(parameter);
        let coercion = CoercionPlan::bridge(self, &source, &target);
        let value = if raw_source.is_some() {
            if matches!(argument.unwrap_parens(), Expression::Call { .. }) {
                self.lower_call(
                    argument,
                    Some(&argument.get_type()),
                    ExpressionContext::value(),
                )
            } else {
                self.plan_operand(argument, ExpressionContext::value())
            }
        } else {
            self.lower_value(argument, ExpressionContext::value())
        };
        value.map_rendered_as_computed(|setup, value, _contains_deferred_evaluation| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            GoExpression::opaque_with_deferred_evaluation(coerced, true)
        })
    }

    fn go_physical_expression_layout(&self, expression: &Expression) -> Option<ValueLayout> {
        if let Some(plan) = self.plan_call(expression)
            && matches!(plan.resolved.origin, CallableOrigin::GoInterop)
            && matches!(plan.resolved.abi.result, CallableReturnAbi::Direct)
        {
            return Some(plan.resolved.abi.return_layout.clone());
        }
        let callable = self.resolve_callable_value(expression)?;
        matches!(callable.origin, CallableOrigin::GoInterop).then(|| ValueLayout::Function {
            function_type: expression.get_type(),
            layout: callable.abi.function_layout(),
        })
    }

    fn lower_variadic_spread_slot_bridge(
        &mut self,
        spread: &Expression,
        parameter: Option<&CallableParamAbi>,
    ) -> Option<ValuePlan> {
        let parameter = parameter?;
        if parameter.instantiated.get_name() != Some("VarArgs") {
            return None;
        }

        let raw_source = self.go_physical_expression_layout(spread);
        let source = raw_source
            .clone()
            .unwrap_or_else(|| self.value_layout(&spread.get_type(), SlotOrigin::Lisette));
        let target = ValueLayout::Slice {
            collection_type: spread.get_type(),
            element: Box::new(self.argument_slot_layout(parameter)),
        };
        let coercion = CoercionPlan::bridge(self, &source, &target);
        if coercion.is_identity() && raw_source.is_none() {
            return None;
        }

        let value = if raw_source.is_some() {
            self.lower_call(spread, Some(&spread.get_type()), ExpressionContext::value())
        } else {
            self.lower_value(spread, ExpressionContext::value())
        };
        Some(value.map_rendered_as_computed(|setup, value, _| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            GoExpression::opaque_with_deferred_evaluation(coerced, true)
        }))
    }
}

/// The element type of a `VarArgs<T>`, or the type itself when not variadic.
fn varargs_inner_or_self(ty: &Type) -> Type {
    if ty.get_name() == Some("VarArgs") {
        ty.inner().unwrap_or_else(|| ty.clone())
    } else {
        ty.clone()
    }
}

fn spread_needs_any_wrap(function: &Expression, spread: Option<&Expression>) -> bool {
    let Some(spread_expr) = spread else {
        return false;
    };
    let Some(variadic_element) = function.get_type().unwrap_forall().is_variadic() else {
        return false;
    };
    if !variadic_element.is_unknown() {
        return false;
    }
    spread_expr
        .get_type()
        .inner()
        .is_some_and(|t| !t.is_unknown())
}

fn would_suppress_tagged_go(callee: &ResolvedCallee<'_>, declared_param_ty: Option<&Type>) -> bool {
    let unwrapped = declared_param_ty.map(|p| p.unwrap_forall());
    callee.is_prelude_dispatch && unwrapped.is_some_and(|p| matches!(p, Type::Function(_)))
}

/// Compute the `ExpressionContext` for emitting a Direct or TaggedGoLowering
/// argument's underlying value via `emit_composite_value`.
fn direct_arg_emit_ctx<'b>(
    effective_param_ty: Option<&'b Type>,
    suppress: bool,
) -> ExpressionContext<'b> {
    let unwrapped = effective_param_ty.map(|p| p.unwrap_forall());
    let flows_to_unknown = unwrapped.is_some_and(|p| p.resolves_to_unknown());
    ExpressionContext::value()
        .with_forced_tagged_go_function(suppress)
        .with_unknown_argument_target(flows_to_unknown)
}
