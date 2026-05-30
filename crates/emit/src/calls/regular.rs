use crate::abi::is_tagged_shape_fn_value;
use crate::calls::dispatch::is_prelude_variant_constructor;
use crate::calls::go_interop::is_go_receiver;
use rustc_hash::FxHashSet as HashSet;

use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::ReturnContext;
use crate::abi::coercion::{Coercion, CoercionDirection, OptionShape, classify_option_shape};
use crate::abi::transition::{emit_fn_arg_shape_adapter, emit_lisette_callback_wrapper};
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::expressions::staging::VariadicCombine;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::{ArgumentPlan, CallPlan, CallbackWrapperKind, NullableCoerceKind};
use crate::utils::{contains_call, reads_mutable_operand};
use crate::write_line;
use syntax::ast::{Annotation, Expression};
use syntax::program::Definition;
use syntax::types::Type;

struct CalleeAnalysis<'a> {
    fn_param_types: Vec<Type>,
    generic_fn_param_types: Option<&'a [Type]>,
    pointer_indices: HashSet<usize>,
    is_go_call: bool,
    is_prelude_dispatch: bool,
}

struct CallArgsContext<'a> {
    fn_param_types: &'a [Type],
    generic_fn_param_types: Option<&'a [Type]>,
    pointer_indices: &'a HashSet<usize>,
    is_go_call: bool,
    /// Suppresses the Go-fn identity short-circuit on fn-typed params
    /// dispatching into prelude generic helpers (e.g. `OptionAndThen`).
    is_prelude_dispatch: bool,
    spread: Option<&'a Expression>,
    wrap_spread_to_any: bool,
    combine_variadic: Option<VariadicCombine>,
    ambient_return_ctx: Option<&'a ReturnContext>,
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

    if let Some(inner) = arg
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
        call_plan: &CallPlan,
        call_ty: Option<&Type>,
        expression_ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::Call {
            expression: callee,
            args,
            type_args,
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
                .map(|a| self.stage_operand(a, ExpressionContext::value(), fx))
                .collect();
            let wrap_to_any = spread_needs_any_wrap(function, spread);
            let combine = call_plan.variadic_combine(0);
            let (setup, args_strings) = self.sequence_with_spread_structured(
                stages,
                spread,
                wrap_to_any,
                "_arg",
                combine,
                expression_ctx.ambient_return_ctx(),
                fx,
            );
            return (setup, format!("{}({})", go_name, args_strings.join(", ")));
        }

        let callee_staged = self.stage_operand(function, expression_ctx.callee(), fx);
        let mut function_string = callee_staged.value;

        if function.deref_inner().is_some() {
            function_string = format!("({})", function_string);
        }

        let type_args_string = self.resolve_call_type_args(
            function,
            type_args,
            call_ty,
            &mut function_string,
            expression_ctx,
            fx,
        );

        let analysis = self.analyze_callee(function);
        let args_ctx = CallArgsContext {
            fn_param_types: &analysis.fn_param_types,
            generic_fn_param_types: analysis.generic_fn_param_types,
            pointer_indices: &analysis.pointer_indices,
            is_go_call: analysis.is_go_call,
            is_prelude_dispatch: analysis.is_prelude_dispatch,
            spread,
            wrap_spread_to_any: spread_needs_any_wrap(function, spread),
            combine_variadic: call_plan.variadic_combine(0),
            ambient_return_ctx: expression_ctx.ambient_return_ctx(),
        };
        let (args_setup, args_strings) = self.emit_call_args(args, &args_ctx, fx);

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

    fn analyze_callee(&mut self, function: &Expression) -> CalleeAnalysis<'a> {
        let pointer_indices = self.get_recursive_enum_pointer_indices(function);
        let fn_param_types: Vec<Type> = function
            .get_type()
            .unwrap_forall()
            .get_function_params()
            .map(<[Type]>::to_vec)
            .unwrap_or_default();
        let generic_fn_param_types = self
            .callee_definition(function)
            .and_then(|definition| definition.ty().unwrap_forall().get_function_params());
        let (is_go_call, is_prelude_dispatch) = match function.unwrap_parens() {
            Expression::DotAccess { expression, .. } => {
                let is_prelude = matches!(
                    expression.get_type().strip_refs().unwrap_forall(),
                    Type::Nominal { id, .. } if id.starts_with("prelude.")
                );
                (is_go_receiver(expression), is_prelude)
            }
            Expression::Identifier {
                qualified: Some(q), ..
            } if q.starts_with("prelude.") => (false, true),
            _ => (false, false),
        };
        CalleeAnalysis {
            fn_param_types,
            generic_fn_param_types,
            pointer_indices,
            is_go_call,
            is_prelude_dispatch,
        }
    }

    pub(crate) fn callee_definition(&self, function: &Expression) -> Option<&'a Definition> {
        match function.unwrap_parens() {
            Expression::Identifier {
                qualified: Some(q), ..
            } => self.facts.definition(q.as_str()),
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } => {
                let receiver_ty = receiver.get_type();
                if let Some(module) = receiver_ty.as_import_namespace() {
                    return self.facts.definition(&format!("{}.{}", module, member));
                }
                match receiver_ty.strip_refs() {
                    Type::Nominal { id, .. } => {
                        self.facts.definition(&format!("{}.{}", id, member))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
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

    fn resolve_call_type_args(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        function_string: &mut String,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        let mut type_args_string = self.format_type_args_from_annotations(type_args, fx);

        let slot_ty = ctx.expected_slot_type();

        if type_args_string.is_empty()
            && let Some(inferred) = self.infer_return_only_type_args(function, fx)
        {
            type_args_string = match slot_ty {
                Some(t) => self.prelude_container_type_args(t, fx).unwrap_or(inferred),
                None => inferred,
            };
        }

        if type_args_string.is_empty() && is_prelude_variant_constructor(function) {
            let mut candidate = call_ty.and_then(|t| self.prelude_container_type_args(t, fx));
            if candidate.is_none() {
                candidate = slot_ty.and_then(|t| self.prelude_container_type_args(t, fx));
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
        ctx: &CallArgsContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let mut stages: Vec<StagedExpression> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let (setup, value) = self.lower_call_arg(arg, i, ctx, fx);
                StagedExpression::from_typed_setup(setup, value, arg)
            })
            .collect();

        if let Some(spread) = ctx.spread
            && let Some(adapter_stage) =
                self.try_emit_variadic_spread_adapter(spread, ctx.generic_fn_param_types, fx)
        {
            stages.push(adapter_stage);
            let spread_index = stages.len() - 1;
            let (setup, mut values) = self.sequence_structured(stages, "_arg");
            self.finalize_spread_stage(
                &mut values,
                spread_index,
                ctx.wrap_spread_to_any,
                ctx.combine_variadic.as_ref().cloned(),
                fx,
            );
            return (setup, values);
        }

        self.sequence_with_spread_structured(
            stages,
            ctx.spread,
            ctx.wrap_spread_to_any,
            "_arg",
            ctx.combine_variadic.as_ref().cloned(),
            ctx.ambient_return_ctx,
            fx,
        )
    }

    /// Classify and lower a single call argument: dispatch is plan-driven and
    /// returns typed setup. The plain `Direct` / `RecursiveEnumPointer` /
    /// `TaggedGoLowering` paths produce typed `TempBind` setup; the remaining
    /// adapter paths (`GoCallbackAdapter`, `LoweredFnShapeAdapter`,
    /// `NullableCoercion`, `GoPointerUnwrap`) capture their string emission as
    /// a single `RawGo` until each is individually converted.
    fn lower_call_arg(
        &mut self,
        arg: &Expression,
        index: usize,
        ctx: &CallArgsContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let effective_param_ty = effective_param_type(index, ctx.fn_param_types);
        let generic_param_ty = ctx
            .generic_fn_param_types
            .and_then(|params| effective_param_type(index, params));

        let plan = self.plan_argument(arg, index, ctx, effective_param_ty, generic_param_ty);

        match plan {
            ArgumentPlan::GoCallbackAdapter(kind) => self.lower_callback_wrapper(
                arg,
                effective_param_ty.expect("GoCallbackAdapter requires effective_param_ty"),
                kind,
                fx,
            ),
            ArgumentPlan::LoweredFnShapeAdapter => self
                .lower_adapt_lowered_fn_arg_shape(
                    arg,
                    generic_param_ty.expect("LoweredFnShapeAdapter requires generic_param_ty"),
                    fx,
                )
                .expect("detect_lowered_fn_arg_shape ensures Some"),
            ArgumentPlan::NullableCoercion(kind) => self.lower_nullable_coercion(
                arg,
                effective_param_ty.expect("NullableCoercion requires effective_param_ty"),
                kind,
                fx,
            ),
            ArgumentPlan::GoPointerUnwrap => self.lower_go_pointer_param_unwrap(
                arg,
                effective_param_ty.expect("GoPointerUnwrap requires effective_param_ty"),
                fx,
            ),
            ArgumentPlan::RecursiveEnumPointer => {
                let (mut setup, value) = self.lower_value(
                    arg,
                    ExpressionContext::value().with_ambient_return_ctx_opt(ctx.ambient_return_ctx),
                    fx,
                );
                if matches!(arg, Expression::Reference { .. }) || arg.get_type().is_ref() {
                    return (setup, value);
                }
                let temp = self.hoist_tmp_value_statement(&mut setup, "ptr", &value);
                (setup, format!("&{}", temp))
            }
            ArgumentPlan::TaggedGoLowering => {
                let target =
                    effective_param_ty.expect("TaggedGoLowering requires effective_param_ty");
                let arg_ctx = direct_arg_emit_ctx(ctx, Some(target), true);
                let (mut setup, value) = self.lower_composite_value(arg, arg_ctx, fx);
                let mut buffer = String::new();
                let lowered = self.emit_lower_arg_to_tagged(&mut buffer, &value, target, fx);
                if !buffer.is_empty() {
                    setup.push(LoweredStatement::RawGo(buffer));
                }
                (setup, lowered)
            }
            ArgumentPlan::Direct => self.lower_direct_arg(arg, ctx, effective_param_ty, fx),
        }
    }

    /// Pre-plan adaptations for a single argument. Mirrors the prior
    /// `try_emit_*` chain in order; the first hit wins. Returns `Direct` for
    /// the fallback path (which still handles tagged-Go suppression inline).
    fn plan_argument(
        &self,
        arg: &Expression,
        index: usize,
        ctx: &CallArgsContext<'_>,
        effective_param_ty: Option<&Type>,
        generic_param_ty: Option<&Type>,
    ) -> ArgumentPlan {
        if ctx.is_go_call
            && let Some(kind) = self.detect_callback_wrapper(arg, effective_param_ty)
        {
            return ArgumentPlan::GoCallbackAdapter(kind);
        }
        if self
            .detect_lowered_fn_arg_shape(arg, generic_param_ty)
            .is_some()
        {
            return ArgumentPlan::LoweredFnShapeAdapter;
        }
        if let Some(kind) = self.detect_nullable_coercion(arg, effective_param_ty) {
            return ArgumentPlan::NullableCoercion(kind);
        }
        if ctx.is_go_call
            && self
                .detect_go_pointer_param_unwrap(arg, effective_param_ty)
                .is_some()
        {
            return ArgumentPlan::GoPointerUnwrap;
        }
        if ctx.pointer_indices.contains(&index) {
            return ArgumentPlan::RecursiveEnumPointer;
        }
        let suppress = would_suppress_tagged_go(ctx, effective_param_ty);
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
        ctx: &CallArgsContext<'_>,
        effective_param_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let suppress = would_suppress_tagged_go(ctx, effective_param_ty);
        let arg_ctx = direct_arg_emit_ctx(ctx, effective_param_ty, suppress);
        let (mut setup, value) = self.lower_composite_value(arg, arg_ctx, fx);
        let final_value = match effective_param_ty {
            Some(target) => {
                let coercion =
                    Coercion::resolve(self, &arg.get_type(), target, CoercionDirection::Internal);
                let (coercion_setup, coerced) = coercion.lower(self, value, fx);
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
        output: &mut String,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        self.detect_lowered_fn_arg_shape(arg, generic_param_ty)?;
        let (setup, value) =
            self.lower_adapt_lowered_fn_arg_shape(arg, generic_param_ty.unwrap(), fx)?;
        output.push_str(&Renderer.render_setup(&setup));
        Some(value)
    }

    /// Detect whether `arg`'s fn-shape disagrees with the callee's generic
    /// param shape (Lisette callback adapter trigger). Pure detection.
    pub(crate) fn detect_lowered_fn_arg_shape(
        &self,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<()> {
        if is_tagged_shape_fn_value(arg) {
            return None;
        }
        let raw_param_ty = generic_param_ty?;
        let variadic_inner = (raw_param_ty.get_name() == Some("VarArgs"))
            .then(|| raw_param_ty.inner())
            .flatten();
        let param_ty = variadic_inner.as_ref().unwrap_or(raw_param_ty);
        let param_fn = self
            .facts
            .resolve_to_function_type(param_ty.unwrap_forall())?;
        let param_ret = param_fn.get_function_ret()?;
        let param_shape = self.classify_direct_emission(param_ret);

        let arg_ty = arg.get_type();
        let arg_fn = self
            .facts
            .resolve_to_function_type(arg_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_shape = self.classify_direct_emission(arg_ret)?;

        if param_shape.as_ref() == Some(&arg_shape) {
            return None;
        }
        Some(())
    }

    fn lower_adapt_lowered_fn_arg_shape(
        &mut self,
        arg: &Expression,
        generic_param_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let raw_param_ty = generic_param_ty;
        let variadic_inner = (raw_param_ty.get_name() == Some("VarArgs"))
            .then(|| raw_param_ty.inner())
            .flatten();
        let param_ty = variadic_inner.as_ref().unwrap_or(raw_param_ty);
        let param_fn = self
            .facts
            .resolve_to_function_type(param_ty.unwrap_forall())?;
        let param_ret = param_fn.get_function_ret()?;
        let param_shape = self.classify_direct_emission(param_ret);

        let arg_ty = arg.get_type();
        let arg_fn = self
            .facts
            .resolve_to_function_type(arg_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_shape = self.classify_direct_emission(arg_ret)?;

        let (mut setup, value) = self.lower_value(arg, ExpressionContext::value(), fx);
        let mut buffer = String::new();
        let adapted = emit_fn_arg_shape_adapter(
            self,
            &mut buffer,
            &value,
            &arg_fn,
            &arg_shape,
            param_shape.as_ref(),
            fx,
        )?;
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
        fx: &mut EmitEffects,
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
        let param_shape = self.classify_direct_emission(param_ret);

        let spread_ty = spread.get_type();
        let element_ty = spread_ty.unwrap_forall().inner()?;
        let arg_fn = self
            .facts
            .resolve_to_function_type(element_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_shape = self.classify_direct_emission(arg_ret)?;

        if param_shape.as_ref() == Some(&arg_shape) {
            return None;
        }

        let mut setup = String::new();
        let src_value = self.emit_value(&mut setup, spread, ExpressionContext::value(), fx);
        let src_var = self.hoist_tmp_value(&mut setup, "src", &src_value);

        let target_element_ret = match param_shape.as_ref() {
            Some(shape) => self.render_lowered_return_ty(shape, arg_ret, fx),
            None => self.go_type_string(arg_ret, fx),
        };
        let arg_fn_params = arg_fn.get_function_params().unwrap_or(&[]);
        let param_type_strs: Vec<String> = arg_fn_params
            .iter()
            .map(|p| self.go_type_string(p, fx))
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
        let closure = emit_fn_arg_shape_adapter(
            self,
            &mut body,
            &loop_cb,
            &arg_fn,
            &arg_shape,
            param_shape.as_ref(),
            fx,
        )?;
        write_line!(body, "{}[i] = {}", adapted, closure);

        write_line!(
            setup,
            "{} := make([]{}, len({}))",
            adapted,
            target_element_ty,
            src_var
        );
        write_line!(
            setup,
            "for i, {} := range {} {{\n{}}}",
            loop_cb,
            src_var,
            body
        );

        Some(StagedExpression::new(setup, adapted, spread))
    }

    /// Detect whether a Go-call argument needs a callback wrapper. Returns
    /// `Identity` when the shapes already agree (no wrapping, just emit) and
    /// `Wrap` when the Lisette callback ABI must be wrapped for the Go param.
    pub(crate) fn detect_callback_wrapper(
        &self,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<CallbackWrapperKind> {
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

        let arg_ty = arg.get_type();
        let arg_fn_ty = self.facts.resolve_to_function_type(arg_ty.unwrap_forall());
        if let Some(Type::Function(arg_f)) = arg_fn_ty.as_ref()
            && let Type::Function(param_f) = &param_fn_ty
            && self.classify_direct_emission(&arg_f.return_type).is_some()
            && self
                .classify_direct_emission(&param_f.return_type)
                .is_some()
        {
            return Some(CallbackWrapperKind::Identity);
        }
        Some(CallbackWrapperKind::Wrap)
    }

    fn lower_callback_wrapper(
        &mut self,
        arg: &Expression,
        effective_param_ty: &Type,
        kind: CallbackWrapperKind,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let (mut setup, value) = self.lower_value(arg, ExpressionContext::value(), fx);
        let result = match kind {
            CallbackWrapperKind::Identity => value,
            CallbackWrapperKind::Wrap => {
                let param_fn_ty = self
                    .facts
                    .resolve_to_function_type(effective_param_ty.unwrap_forall())
                    .expect("Wrap kind only reached when param resolves to a fn type");
                let mut buffer = String::new();
                let wrapped =
                    emit_lisette_callback_wrapper(self, &mut buffer, &value, &param_fn_ty, fx);
                if !buffer.is_empty() {
                    setup.push(LoweredStatement::RawGo(buffer));
                }
                wrapped
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
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        if arg.is_none_literal() {
            return (Vec::new(), "nil".to_string());
        }
        let arg_ty = arg.get_type();
        let (mut setup, value) = self.lower_value(arg, ExpressionContext::value(), fx);
        let coercion = Coercion::resolve(self, &arg_ty, param_ty, CoercionDirection::ToGoBoundary);
        let (coercion_setup, coerced) = coercion.lower(self, value, fx);
        setup.extend(coercion_setup);
        (setup, coerced)
    }

    /// Detect which nullable-coercion strategy (if any) applies to this
    /// argument. Pure: no emission, callable from the planning layer.
    pub(crate) fn detect_nullable_coercion(
        &self,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<NullableCoerceKind> {
        let param_ty = effective_param_ty?;
        let arg_ty = arg.get_type();
        let check_ty = if param_ty.get_name() == Some("VarArgs") {
            param_ty.inner().unwrap_or_else(|| param_ty.clone())
        } else {
            param_ty.clone()
        };

        if arg_ty.is_option() && check_ty.resolves_to_unknown() {
            return Some(NullableCoerceKind::OptionToUnknown);
        }

        if !matches!(classify_option_shape(self, &arg_ty), OptionShape::Nullable) {
            return None;
        }
        let needs_coercion = self
            .facts
            .as_interface(&check_ty)
            .is_some_and(|id| go_name::is_go_import(&id));

        if !needs_coercion {
            return None;
        }

        Some(NullableCoerceKind::NullableInterface)
    }

    fn lower_nullable_coercion(
        &mut self,
        arg: &Expression,
        effective_param_ty: &Type,
        kind: NullableCoerceKind,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let arg_ty = arg.get_type();
        match kind {
            NullableCoerceKind::OptionToUnknown => {
                let check_ty = if effective_param_ty.get_name() == Some("VarArgs") {
                    effective_param_ty
                        .inner()
                        .unwrap_or_else(|| effective_param_ty.clone())
                } else {
                    effective_param_ty.clone()
                };
                if arg.is_none_literal() {
                    return (Vec::new(), "nil".to_string());
                }
                let (mut setup, value) = self.lower_value(arg, ExpressionContext::value(), fx);
                let coercion =
                    Coercion::resolve(self, &arg_ty, &check_ty, CoercionDirection::ToGoBoundary);
                let (coercion_setup, coerced) = coercion.lower(self, value, fx);
                setup.extend(coercion_setup);
                (setup, coerced)
            }
            NullableCoerceKind::NullableInterface => {
                self.lower_unwrap_go_nullable_arg(arg, &arg_ty, fx)
            }
        }
    }

    fn lower_unwrap_go_nullable_arg(
        &mut self,
        arg: &Expression,
        arg_ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        if arg.is_none_literal() {
            return (Vec::new(), "nil".to_string());
        }
        let (mut setup, value) = self.lower_value(arg, ExpressionContext::value(), fx);
        let coercion = Coercion::resolve(self, arg_ty, arg_ty, CoercionDirection::ToGoBoundary);
        let (coercion_setup, coerced) = coercion.lower(self, value, fx);
        setup.extend(coercion_setup);
        (setup, coerced)
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

/// True when a prelude-dispatch call's param is a function type — the
/// condition that triggers `with_forced_tagged_go_function` and gates the
/// tagged-Go lowering wrap.
fn would_suppress_tagged_go(ctx: &CallArgsContext<'_>, effective_param_ty: Option<&Type>) -> bool {
    let unwrapped = effective_param_ty.map(|p| p.unwrap_forall());
    ctx.is_prelude_dispatch && unwrapped.is_some_and(|p| matches!(p, Type::Function(_)))
}

/// Compute the `ExpressionContext` for emitting a Direct or TaggedGoLowering
/// argument's underlying value via `emit_composite_value`.
fn direct_arg_emit_ctx<'b>(
    ctx: &CallArgsContext<'b>,
    effective_param_ty: Option<&'b Type>,
    suppress: bool,
) -> ExpressionContext<'b> {
    let unwrapped = effective_param_ty.map(|p| p.unwrap_forall());
    let flows_to_unknown = unwrapped.is_some_and(|p| p.resolves_to_unknown());
    ExpressionContext::value()
        .with_forced_tagged_go_function(suppress)
        .with_unknown_argument_target(flows_to_unknown)
        .with_ambient_return_ctx_opt(ctx.ambient_return_ctx)
}

pub(crate) fn effective_param_type(index: usize, fn_param_types: &[Type]) -> Option<&Type> {
    fn_param_types.get(index).or_else(|| {
        fn_param_types
            .last()
            .filter(|t| t.get_name() == Some("VarArgs"))
    })
}
