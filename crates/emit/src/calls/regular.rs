use crate::abi::is_tagged_shape_fn_value;
use crate::calls::dispatch::{
    CallArgShape, all_type_params_inferrable, is_prelude_variant_constructor,
};

use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableParamAbi, CallableReturnAbi};
use crate::abi::coercion::{Coercion, CoercionDirection, OptionShape, classify_option_shape};
use crate::abi::transition::{emit_fn_arg_shape_adapter, emit_lisette_callback_wrapper};
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::expressions::staging::VariadicCombine;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::{ArgumentPlan, CallPlan, CallableOrigin, ResolvedCallee};
use crate::utils::{contains_call, reads_mutable_operand};
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
    ) -> (Vec<LoweredStatement>, String) {
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
            let stages: Vec<StagedExpression> = args
                .iter()
                .map(|a| self.stage_operand(a, ExpressionContext::value()))
                .collect();
            let wrap_to_any = spread_needs_any_wrap(function, spread);
            let combine = call_plan.variadic_combine(0);
            let (setup, args_strings) =
                self.sequence_with_spread_structured(stages, spread, wrap_to_any, "_arg", combine);
            return (setup, format!("{}({})", go_name, args_strings.join(", ")));
        }

        let callee_staged = self.stage_operand(function, expression_ctx.callee());
        let mut function_string = callee_staged.value;

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
        };
        let (args_setup, args_strings) = self.emit_call_args(args, &args_ctx);

        let mut setup = callee_staged.setup;
        let callee_needs_pin = setup.is_empty()
            && type_args_string.is_empty()
            && reads_mutable_operand(function)
            && (!args_setup.is_empty()
                || (!matches!(function, Expression::Call { .. })
                    && (args.iter().any(contains_call) || spread.is_some_and(contains_call))));
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

        let has_array_return = call_plan.has_go_array_return();
        let value = match self.wrap_go_array_return(
            &mut setup,
            has_array_return,
            &call_str,
            expression_ctx,
        ) {
            Some(wrapped) => wrapped,
            None => call_str,
        };
        (setup, value)
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

    /// Hoist a Go array-return call into a temp and reslice as `[]T`. Skipped
    /// for discarded calls.
    fn wrap_go_array_return(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        has_array_return: bool,
        call_str: &str,
        ctx: ExpressionContext<'_>,
    ) -> Option<String> {
        if !has_array_return || ctx.keeps_raw_go_array_return() {
            return None;
        }
        let temp = self.hoist_tmp_value_statement(setup, "arr", call_str);
        Some(format!("{}[:]", temp))
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
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let stages: Vec<StagedExpression> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let (setup, value) = self.lower_call_arg(arg, i, ctx);
                self.staged_from_typed_setup(setup, value, arg)
            })
            .collect();

        self.sequence_args_with_spread_adapter(
            stages,
            ctx.spread,
            ctx.plan
                .resolved
                .declared
                .as_ref()
                .and_then(|ty| ty.unwrap_forall().get_function_params()),
            ctx.wrap_spread_to_any,
            ctx.combine_variadic.clone(),
        )
    }

    /// Classify and lower a single call argument: dispatch is plan-driven and
    /// returns typed setup. The plain `Direct` / `TaggedGoLowering` paths produce
    /// typed `TempBind` setup; the remaining adapter paths (`GoCallbackAdapter`,
    /// `LoweredFnShapeAdapter`, `NullableCoercion`, `GoPointerUnwrap`) capture
    /// their string emission as a single `RawGo` until each is individually
    /// converted.
    fn lower_call_arg(
        &mut self,
        arg: &Expression,
        index: usize,
        ctx: &CallArgsContext<'_, '_>,
    ) -> (Vec<LoweredStatement>, String) {
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
            ArgumentPlan::NullableCoercion => {
                let arg_ty = arg.get_type();
                self.lower_unwrap_go_nullable_arg(arg, &arg_ty)
            }
            ArgumentPlan::GoPointerUnwrap => self.lower_go_pointer_param_unwrap(
                arg,
                effective_param_ty.expect("GoPointerUnwrap requires effective_param_ty"),
            ),
            ArgumentPlan::TaggedGoLowering => {
                let target =
                    effective_param_ty.expect("TaggedGoLowering requires effective_param_ty");
                let arg_ctx = direct_arg_emit_ctx(Some(target), true);
                let (mut setup, value) = self.lower_composite_value(arg, arg_ctx).into_parts();
                let lowered = self.emit_lower_arg_to_tagged(&mut setup, &value, target);
                (setup, lowered)
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
        if self
            .detect_nullable_coercion(arg, effective_param_ty)
            .is_some()
        {
            return ArgumentPlan::NullableCoercion;
        }
        if matches!(callee.origin, CallableOrigin::GoInterop)
            && self
                .detect_go_pointer_param_unwrap(arg, effective_param_ty)
                .is_some()
        {
            return ArgumentPlan::GoPointerUnwrap;
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
    ) -> (Vec<LoweredStatement>, String) {
        let suppress = would_suppress_tagged_go(&ctx.plan.resolved, declared_param_ty);
        let arg_ctx = direct_arg_emit_ctx(effective_param_ty, suppress);
        let (mut setup, value) = self.lower_composite_value(arg, arg_ctx).into_parts();
        let final_value = match effective_param_ty {
            Some(target) => {
                let coercion =
                    Coercion::resolve(self, &arg.get_type(), target, CoercionDirection::Internal);
                let (coercion_setup, coerced) = coercion.lower(self, value);
                setup.extend(coercion_setup);
                coerced
            }
            None => value,
        };
        (setup, final_value)
    }

    /// Adapt a lowered-return fn arg when its shape disagrees with the
    /// callee's generic-param shape.
    pub(crate) fn try_adapt_lowered_fn_arg_shape(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<String> {
        self.detect_lowered_fn_arg_shape(arg, generic_param_ty)?;
        let (adapt_setup, value) =
            self.lower_adapt_lowered_fn_arg_shape(arg, generic_param_ty.unwrap())?;
        setup.extend(adapt_setup);
        Some(value)
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
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let (param_abi, arg_fn, arg_abi) = self.fn_arg_shapes(arg, generic_param_ty)?;
        let (mut setup, value) = self
            .lower_value(arg, ExpressionContext::value())
            .into_parts();
        let mut buffer = String::new();
        let adapted =
            emit_fn_arg_shape_adapter(self, &mut buffer, &value, &arg_fn, &arg_abi, &param_abi)?;
        if !buffer.is_empty() {
            setup.push(LoweredStatement::RawGo(buffer));
        }
        Some((setup, adapted))
    }

    /// Adapt `slice...` spread into a generic `VarArgs<fn(...)>` when the
    /// slice's element fn-shape disagrees with the variadic's element.
    pub(crate) fn try_emit_variadic_spread_adapter(
        &mut self,
        spread: &Expression,
        generic_params: Option<&[Type]>,
    ) -> Option<StagedExpression> {
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

        let (mut setup, src_value) = self
            .lower_value(spread, ExpressionContext::value())
            .into_parts();
        let src_var = self.hoist_tmp_value_statement(&mut setup, "src", &src_value);

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

        setup.push(LoweredStatement::RawGo(format!(
            "{} := make([]{}, len({}))\n",
            adapted, target_element_ty, src_var
        )));
        setup.push(LoweredStatement::RawGo(format!(
            "for i, {} := range {} {{\n{}}}\n",
            loop_cb, src_var, body
        )));

        Some(self.staged_from_typed_setup(setup, adapted, spread))
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
        Some((source, target, transition))
    }

    fn lower_callback_wrapper(
        &mut self,
        arg: &Expression,
        effective_param_ty: &Type,
        source: &CallableReturnAbi,
        target: &CallableReturnAbi,
        transition: AbiTransition,
    ) -> (Vec<LoweredStatement>, String) {
        let (mut setup, value) = match transition {
            AbiTransition::Identity => self
                .lower_value(arg, ExpressionContext::value())
                .into_parts(),
            _ => self
                .plan_operand(
                    arg,
                    ExpressionContext::value().with_forced_tagged_go_function(true),
                )
                .into_parts(),
        };
        let result = match transition {
            AbiTransition::Identity => value,
            AbiTransition::LowerFromTagged => {
                let param_fn_ty = self
                    .facts
                    .resolve_to_function_type(effective_param_ty.unwrap_forall())
                    .expect("callback target resolves to a fn type");
                emit_lisette_callback_wrapper(self, &mut setup, &value, &param_fn_ty)
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
        (setup, result)
    }

    /// Detect whether `arg`/`param_ty` form a Go pointer-param unwrap pair.
    /// Bridges a Lisette `Option<T>` argument to Go's nil-accepting form when
    /// the param and arg agree on an Option shape Go expresses as `*T`:
    /// either both `Nullable` (`Option<Ref<T>>`) or both `PointerBridged`
    /// (`Option<scalar>` produced by bindgen's `nilable_param` config).
    /// Pure: no emission, callable from the planning layer.
    pub(crate) fn detect_go_pointer_param_unwrap(
        &self,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<()> {
        let param_ty = effective_param_ty?;
        let arg_ty = arg.get_type();
        match (
            classify_option_shape(self, param_ty),
            classify_option_shape(self, &arg_ty),
        ) {
            (OptionShape::Nullable, OptionShape::Nullable)
            | (OptionShape::PointerBridged, OptionShape::PointerBridged) => Some(()),
            _ => None,
        }
    }

    fn lower_go_pointer_param_unwrap(
        &mut self,
        arg: &Expression,
        param_ty: &Type,
    ) -> (Vec<LoweredStatement>, String) {
        if arg.is_none_literal() {
            return (Vec::new(), "nil".to_string());
        }
        let arg_ty = arg.get_type();
        let (mut setup, value) = self
            .lower_value(arg, ExpressionContext::value())
            .into_parts();
        let coercion = Coercion::resolve(self, &arg_ty, param_ty, CoercionDirection::ToGoBoundary);
        let (coercion_setup, coerced) = coercion.lower(self, value);
        setup.extend(coercion_setup);
        (setup, coerced)
    }

    /// Detect a nullable `Option` argument flowing into a Go-imported
    /// interface param. Pure: no emission, callable from the planning layer.
    pub(crate) fn detect_nullable_coercion(
        &self,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<()> {
        let param_ty = effective_param_ty?;
        let arg_ty = arg.get_type();
        let check_ty = varargs_inner_or_self(param_ty);

        if !matches!(classify_option_shape(self, &arg_ty), OptionShape::Nullable) {
            return None;
        }
        self.facts
            .as_interface(&check_ty)
            .is_some_and(|id| go_name::is_go_import(&id))
            .then_some(())
    }

    fn lower_unwrap_go_nullable_arg(
        &mut self,
        arg: &Expression,
        arg_ty: &Type,
    ) -> (Vec<LoweredStatement>, String) {
        if arg.is_none_literal() {
            return (Vec::new(), "nil".to_string());
        }
        let (mut setup, value) = self
            .lower_value(arg, ExpressionContext::value())
            .into_parts();
        let coercion = Coercion::resolve(self, arg_ty, arg_ty, CoercionDirection::ToGoBoundary);
        let (coercion_setup, coerced) = coercion.lower(self, value);
        setup.extend(coercion_setup);
        (setup, coerced)
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
