use crate::abi::{is_closure_literal, is_tagged_shape_fn_value};
use crate::calls::dispatch::{
    CallArgShape, all_type_params_inferrable, callee_is_go_builtin, go_builtin_name,
    is_prelude_variant_constructor,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::EcoString;
use syntax::types::FunctionParameter;

use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableParamAbi, CallableReturnAbi};
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::transition::{emit_fn_arg_shape_adapter, emit_lisette_callback_wrapper};
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::LaterStages;
use crate::expressions::staging::{SpreadSequenceOptions, VariadicCombine};
use crate::names::generics::extract_type_mapping;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign, define,
};
use crate::plan::calls::{ArgumentPlan, CallPlan, CallableOrigin, ResolvedCallee};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{
    CaptureBoundary, ConstantKind, EvaluationEffect, GoExpression, SequencedValues, ValuePlan,
};
use syntax::ast::{Expression, Literal, ResolvedCallTypeArguments};
use syntax::types::Type;

struct CallTypeArgsRequest<'e, 'c> {
    function: &'e Expression,
    callee: &'e ResolvedCallee<'c>,
    type_args: ResolvedCallTypeArguments<'e>,
    call_ty: Option<&'e Type>,
    arg_shape: CallArgShape,
    ctx: ExpressionContext<'e>,
}

fn builtin_constant(builtin: &str, values: &[GoExpression]) -> Option<ConstantKind> {
    let mut kinds = values.iter().map(GoExpression::constant_kind);
    let first = kinds.next()??;
    let joined = kinds.try_fold(first, |joined, kind| {
        let kind = kind?;
        joined.join(kind).or((joined == kind).then_some(kind))
    })?;
    match builtin {
        "min" | "max" => Some(joined),
        "complex" => Some(ConstantKind::Complex),
        "real" | "imag" => Some(ConstantKind::Float),
        _ => None,
    }
}

fn go_builtin_conversion(type_args: &str) -> Option<String> {
    let inner = type_args.strip_prefix('[')?.strip_suffix(']')?;
    (!inner.is_empty() && !inner.contains(',')).then(|| inner.to_string())
}

struct FnArgShapes {
    param_abi: CallableReturnAbi,
    param_origin: SlotOrigin,
    arg_fn: Type,
    arg_abi: CallableReturnAbi,
}

struct CallArgsContext<'plan, 'facts> {
    plan: &'plan CallPlan<'facts>,
    /// Suppresses the Go-fn identity short-circuit on fn-typed params
    /// dispatching into prelude generic helpers (e.g. `OptionAndThen`).
    spread: Option<&'plan Expression>,
    wrap_spread_to_any: bool,
    combine_variadic: Option<VariadicCombine>,
    capture_boundary: CaptureBoundary,
    retired_receiver: Option<&'plan Expression>,
    callee_is_builtin: bool,
    callee_pins_type_args: bool,
    receiver_binding: Option<(Type, Type)>,
}

fn receiver_type_binding(
    callee_expression: &Expression,
    callee: &ResolvedCallee<'_>,
) -> Option<(Type, Type)> {
    if callee.receiver_offset != 1 {
        return None;
    }
    let Expression::DotAccess {
        expression: receiver,
        ..
    } = callee_expression.unwrap_parens()
    else {
        return None;
    };
    let declared = callee
        .declared_type()?
        .unwrap_forall()
        .get_function_params()?
        .first()?
        .ty
        .clone();
    Some((declared, receiver.get_type().strip_refs()))
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

fn is_errors_new_callee(function: &Expression) -> bool {
    let Expression::DotAccess {
        expression, member, ..
    } = function
    else {
        return false;
    };
    member == "New" && expression.get_type().as_import_namespace() == Some("go:errors")
}

enum FmtArgument {
    Sprintf,
    Sprint,
}

fn classify_fmt_argument(planner: &Planner, expression: &Expression) -> Option<FmtArgument> {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => planner
            .format_string_lowers_to_sprintf(parts)
            .then_some(FmtArgument::Sprintf),
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => match (
            callee.unwrap_parens().as_dotted_path().as_deref(),
            args.as_slice(),
            spread,
        ) {
            (Some("fmt.Sprintf"), _, _) => Some(FmtArgument::Sprintf),
            (Some("fmt.Sprint"), [_], None) => Some(FmtArgument::Sprint),
            _ => None,
        },
        _ => None,
    }
}

enum FmtPrint {
    Print,
    Println,
}

impl FmtPrint {
    fn from_callee(callee: &str) -> Option<Self> {
        match callee {
            "fmt.Print" => Some(Self::Print),
            "fmt.Println" => Some(Self::Println),
            _ => None,
        }
    }
}

/// The arguments of `callee(...)` when the expression is a call to that name.
fn call_arguments_of<'n>(
    expression: &'n GoExpressionNode,
    callee: &str,
) -> Option<&'n [GoExpressionNode]> {
    let GoExpressionNode::Call {
        callee: called,
        arguments,
    } = expression
    else {
        return None;
    };
    (called.print() == callee).then_some(arguments.as_slice())
}

/// Collapse redundant fmt wrappers:
/// - `fmt.Print{ln}(fmt.Sprintf(...))` → `fmt.Printf(..., "\n")`
/// - `fmt.Print{ln}(fmt.Sprint(x))` → `fmt.Print{ln}(x)`
fn collapse_fmt_print(
    planner: &Planner,
    callee: &GoExpression,
    args: &[Expression],
    arguments: &[GoExpression],
) -> Option<GoExpression> {
    let print = FmtPrint::from_callee(callee.as_str())?;
    let ([arg_expression], [argument]) = (args, arguments) else {
        return None;
    };

    match classify_fmt_argument(planner, arg_expression)? {
        FmtArgument::Sprintf => {
            let mut inner = call_arguments_of(argument.node(), "fmt.Sprintf")?.to_vec();
            if let FmtPrint::Println = print {
                let Some(GoExpressionNode::Literal(format)) = inner.first_mut() else {
                    return None;
                };
                let close_quote = find_go_string_literal_close(format)?;
                format.insert_str(close_quote, "\\n");
            }
            Some(GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Printf"),
                inner.into_iter().map(GoExpression::from_node).collect(),
            ))
        }
        FmtArgument::Sprint => {
            let inner = call_arguments_of(argument.node(), "fmt.Sprint")?;
            Some(GoExpression::call(
                callee.clone(),
                inner.iter().cloned().map(GoExpression::from_node).collect(),
            ))
        }
    }
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
            type_arguments,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("lower_regular_call requires a Call expression");
        };
        let function = callee.unwrap_parens();
        let spread = spread.as_deref();
        let resolved_type_args = type_arguments
            .resolved_types()
            .expect("emission requires checked call type arguments");

        if let Some(plan) =
            self.collapse_errors_new_format_arg(function, args, spread, expression_ctx)
        {
            return plan;
        }

        if let Some(go_name) = self.get_callee_go_name(function).map(str::to_string) {
            let arg_ctx = match (expression_ctx.retired_receiver(), args.len()) {
                (Some(retired), 1) if self.callee_lowers_to_type_construction(function) => {
                    ExpressionContext::value().with_retired_receiver(retired)
                }
                _ => ExpressionContext::value(),
            };
            let stages: Vec<ValuePlan> =
                args.iter().map(|a| self.plan_operand(a, arg_ctx)).collect();
            let wrap_to_any = spread_needs_any_wrap(&self.facts, function, spread);
            let combine = call_plan.variadic_combine(0);
            let sequenced = self.sequence_with_spread_values(
                stages,
                spread,
                None,
                SpreadSequenceOptions {
                    wrap_to_any,
                    combine,
                    boundary: expression_ctx.capture_boundary(),
                },
            );
            let effect = self.regular_call_effect(function, sequenced.effect);
            let expression = GoExpression::call(GoExpression::name(go_name), sequenced.values);
            return if self.callee_lowers_to_type_construction(function) {
                ValuePlan::observable_call(sequenced.setup, expression, effect)
            } else {
                ValuePlan::plain_call(sequenced.setup, expression, effect)
            };
        }

        let callee_staged = self.plan_operand(function, expression_ctx.callee());
        let callee_effect = callee_staged.evaluation.effect;
        let ValuePlan {
            mut setup,
            expression: mut callee,
            ..
        } = callee_staged;

        if function.deref_inner().is_some() {
            callee = GoExpression::parenthesized(callee);
        }

        let mut type_args_string = self.resolve_call_type_args(CallTypeArgsRequest {
            function,
            callee: &call_plan.resolved,
            type_args: resolved_type_args,
            call_ty,
            arg_shape: CallArgShape {
                value_count: args.len(),
                has_spread: spread.is_some(),
            },
            ctx: expression_ctx,
        });
        let builtin_conversion = callee_is_go_builtin(function)
            .then(|| go_builtin_conversion(&type_args_string))
            .flatten();
        if builtin_conversion.is_some() {
            type_args_string = String::new();
        }
        if !type_args_string.is_empty() {
            callee = callee.without_instantiation();
        }

        let args_ctx = CallArgsContext {
            plan: call_plan,
            spread,
            wrap_spread_to_any: spread_needs_any_wrap(&self.facts, function, spread),
            combine_variadic: call_plan.variadic_combine(0),
            capture_boundary: expression_ctx.capture_boundary(),
            retired_receiver: (args.len() == 1
                && self.callee_lowers_to_type_construction(function))
            .then(|| expression_ctx.retired_receiver())
            .flatten(),
            callee_is_builtin: callee_is_go_builtin(function),
            callee_pins_type_args: !type_args_string.is_empty(),
            receiver_binding: receiver_type_binding(function, &call_plan.resolved),
        };
        let sequenced_args = self.emit_call_args(args, &args_ctx);
        let args_effect = sequenced_args.effect;
        let constant_result = go_builtin_name(function)
            .filter(|_| builtin_conversion.is_none())
            .and_then(|builtin| builtin_constant(builtin, &sequenced_args.values));
        let SequencedValues {
            setup: args_setup,
            values: arguments,
            ..
        } = sequenced_args;

        let callee_needs_pin = setup.is_empty()
            && type_args_string.is_empty()
            && LaterStages::sequenced(&args_setup, args_effect)
                .can_change(self.place_read_stability(function));
        if callee_needs_pin {
            let pinned = self.hoist_tmp_value_statement(&mut setup, "callee", callee);
            callee = GoExpression::name(pinned);
        }

        let call = match collapse_fmt_print(self, &callee, args, &arguments) {
            Some(collapsed) => collapsed,
            None => GoExpression::call(
                GoExpression::instantiation(callee, type_args_string),
                arguments,
            ),
        };
        let call = match builtin_conversion {
            Some(go_type) => GoExpression::conversion(go_type, call),
            None => call,
        };

        setup.extend(args_setup);

        let effect = self
            .regular_call_effect(function, args_effect)
            .combine(callee_effect);
        let expression = call.with_constant(constant_result);
        if self.callee_lowers_to_type_construction(function) {
            ValuePlan::computed(setup, expression, effect)
        } else {
            ValuePlan::plain_call(setup, expression, effect)
        }
    }

    /// Emit `errors.New(f"...")` as `fmt.Errorf(...)`: compiler-built format strings
    /// cannot contain `%w`, and bypassing the callee drops an unused `errors` import.
    fn collapse_errors_new_format_arg(
        &mut self,
        function: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
        expression_ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        if spread.is_some() || !is_errors_new_callee(function) {
            return None;
        }
        let [argument] = args else {
            return None;
        };
        let Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } = argument.unwrap_parens()
        else {
            return None;
        };
        if !self.format_string_lowers_to_sprintf(parts) {
            return None;
        }
        let staged = self.plan_operand(argument, ExpressionContext::value());
        let mut sequenced =
            self.sequence_values(vec![staged], expression_ctx.capture_boundary(), "arg");
        let effect = self.regular_call_effect(function, sequenced.effect);
        let value = sequenced
            .values
            .pop()
            .expect("sequenced exactly one argument");
        let call = match call_arguments_of(value.node(), "fmt.Sprintf") {
            Some(arguments) => GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                arguments
                    .iter()
                    .cloned()
                    .map(GoExpression::from_node)
                    .collect(),
            ),
            None => GoExpression::call(
                GoExpression::qualified(self.package_use_for_package("go:errors"), "New"),
                vec![value],
            ),
        };
        Some(ValuePlan::plain_call(sequenced.setup, call, effect))
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
        callee
            .declaration?
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
        let Some(Type::Forall { vars, body }) = callee.declared_type() else {
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
        let Type::Forall { body, .. } = callee.declared_type()? else {
            return None;
        };
        let mut mapping = rustc_hash::FxHashMap::default();
        extract_type_mapping(body, &callee.instantiated, &mut mapping);
        self.reconstruct_collapsed_type_args(recipe, &mapping)
    }

    fn resolve_call_type_args(&mut self, request: CallTypeArgsRequest<'_, '_>) -> String {
        let CallTypeArgsRequest {
            function,
            callee,
            type_args,
            call_ty,
            arg_shape,
            ctx,
        } = request;
        if callee_curries_receiver(callee) {
            return String::new();
        }

        let has_value_args = arg_shape.value_count > 0 || arg_shape.has_spread;
        if let Some(recipe) = self.callee_collapsed_recipe(callee) {
            if has_value_args && self.collapsed_callee_fully_inferable(callee, arg_shape) {
                return String::new();
            }
            return self
                .reconstruct_collapsed_call_type_args(callee, &recipe)
                .unwrap_or_default();
        }

        let mut type_args_string = self.format_resolved_type_args(type_args);

        let slot_ty = ctx.expected_slot_type();

        if type_args_string.is_empty()
            && let Some(inferred) =
                self.infer_return_only_type_args(function, callee.declared_type(), arg_shape)
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
        let stages: Vec<ValuePlan> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.lower_call_arg(arg, i, ctx))
            .collect();
        let mut stages = self.type_constant_arguments(stages, ctx);

        if let Some(spread) = ctx.spread
            && let Some(stage) =
                self.lower_variadic_spread_slot_bridge(spread, ctx.plan.resolved.abi.params.last())
        {
            stages.push(stage);
            let spread_index = stages.len() - 1;
            let mut sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
            self.finalize_spread_stage(
                &mut sequenced.values,
                spread_index,
                ctx.wrap_spread_to_any,
                ctx.combine_variadic.clone(),
            );
            return sequenced;
        }

        self.sequence_with_spread_values(
            stages,
            ctx.spread,
            ctx.plan
                .resolved
                .declared_type()
                .and_then(|ty| ty.unwrap_forall().get_function_params()),
            SpreadSequenceOptions {
                wrap_to_any: ctx.wrap_spread_to_any,
                combine: ctx.combine_variadic.clone(),
                boundary: ctx.capture_boundary,
            },
        )
    }

    /// Classify and lower a single call argument: dispatch is plan-driven and
    /// returns typed setup. The plain `Direct` / `TaggedGoLowering` paths produce
    /// `Define` setup; adapter and slot-bridge paths retain their own
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
                let arg_ctx = self.direct_arg_emit_ctx(param, true);
                let argument = self.lower_composite_value(arg, arg_ctx);
                argument.map_expression_as_computed(|setup, value| {
                    self.emit_lower_arg_to_tagged(setup, value, target)
                })
            }
            ArgumentPlan::Direct => self.lower_direct_arg(arg, ctx, param, declared_param_ty),
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
            && let Some((source, target, transition)) = self.detect_callback_wrapper(arg, param)
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

    pub(super) fn convert_inferred_constants(
        &mut self,
        stages: Vec<ValuePlan>,
        slots: &[Option<(&Type, &Type)>],
        convertible: &[bool],
        vars: &[EcoString],
        receiver: Option<(&Type, &Type)>,
    ) -> Vec<ValuePlan> {
        let mut bound: HashSet<String> = HashSet::default();
        if let Some((declared, instantiated)) = receiver {
            let mut mapping: HashMap<String, Type> = HashMap::default();
            extract_type_mapping(declared, instantiated, &mut mapping);
            bound.extend(mapping.into_keys());
        }
        for (stage, slot) in stages.iter().zip(slots) {
            if stage.expression.constant_kind().is_some() {
                continue;
            }
            if let Some((declared, instantiated)) = slot {
                let mut mapping: HashMap<String, Type> = HashMap::default();
                extract_type_mapping(declared, instantiated, &mut mapping);
                bound.extend(mapping.into_keys());
            }
        }
        stages
            .into_iter()
            .zip(slots)
            .zip(convertible)
            .map(|((stage, slot), convertible)| {
                let Some((declared, instantiated)) = slot else {
                    return stage;
                };
                let constant = stage.expression.constant_kind();
                if !convertible || constant.is_none() {
                    return stage;
                }
                let mut mapping: HashMap<String, Type> = HashMap::default();
                extract_type_mapping(declared, instantiated, &mut mapping);
                let inferred = mapping.keys().any(|name| {
                    (vars.is_empty() || vars.iter().any(|var| var == name)) && !bound.contains(name)
                });
                if !inferred {
                    return stage;
                }
                let slot_ty = varargs_inner_or_self(instantiated);
                match self.constant_needs_go_type(constant, &slot_ty) {
                    Some(go_type) => stage.conversion(go_type),
                    None => stage,
                }
            })
            .collect()
    }

    fn type_constant_arguments(
        &mut self,
        stages: Vec<ValuePlan>,
        ctx: &CallArgsContext<'_, '_>,
    ) -> Vec<ValuePlan> {
        if ctx.callee_is_builtin || ctx.callee_pins_type_args {
            return stages;
        }
        let abi = &ctx.plan.resolved.abi;
        let vars: Vec<EcoString> = match ctx.plan.resolved.declared_type() {
            Some(Type::Forall { vars, .. }) => vars.clone(),
            _ => Vec::new(),
        };
        let slots: Vec<Option<(&Type, &Type)>> = (0..stages.len())
            .map(|index| {
                abi.param(index).and_then(|param| {
                    param
                        .declared
                        .as_ref()
                        .map(|declared| (declared, &param.instantiated))
                })
            })
            .collect();
        let convertible: Vec<bool> = (0..stages.len())
            .map(|index| matches!(ctx.plan.arguments.get(index), Some(ArgumentPlan::Direct)))
            .collect();
        let receiver = ctx
            .receiver_binding
            .as_ref()
            .map(|(declared, instantiated)| (declared, instantiated));
        self.convert_inferred_constants(stages, &slots, &convertible, &vars, receiver)
    }

    fn lower_direct_arg(
        &mut self,
        arg: &Expression,
        ctx: &CallArgsContext<'_, '_>,
        param: Option<&CallableParamAbi>,
        declared_param_ty: Option<&Type>,
    ) -> ValuePlan {
        let suppress = would_suppress_tagged_go(&ctx.plan.resolved, declared_param_ty);
        let mut arg_ctx = self.direct_arg_emit_ctx(param, suppress);
        if let Some(retired) = ctx.retired_receiver {
            arg_ctx = arg_ctx.with_retired_receiver(retired);
        }
        let argument = self.lower_composite_value(arg, arg_ctx);
        let Some(target) = param.map(|param| &param.instantiated) else {
            return argument;
        };
        let coercion = CoercionPlan::internal(self, &arg.get_type(), target);
        if coercion.is_identity() {
            return argument;
        }
        argument.map_expression_as_computed(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced
        })
    }

    /// Context for a `Direct` or `TaggedGoLowering` argument.
    fn direct_arg_emit_ctx<'b>(
        &self,
        param: Option<&CallableParamAbi>,
        suppress: bool,
    ) -> ExpressionContext<'b> {
        let origin = param.map_or(SlotOrigin::Lisette, |param| {
            self.function_type_origin(&param.instantiated, param.origin)
        });
        let flows_to_unknown = param.is_some_and(|param| {
            self.facts
                .resolves_to_unknown(param.instantiated.unwrap_forall())
        });
        ExpressionContext::value()
            .with_function_slot_origin(origin)
            .with_forced_tagged_go_function(suppress)
            .with_unknown_argument_target(flows_to_unknown)
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
    fn fn_arg_shapes(&self, arg: &Expression, raw_param_ty: &Type) -> Option<FnArgShapes> {
        let variadic_inner = if raw_param_ty.get_name() == Some("VarArgs") {
            raw_param_ty.inner()
        } else {
            None
        };
        let param_ty = variadic_inner.as_ref().unwrap_or(raw_param_ty);
        let param_fn = self
            .facts
            .resolve_to_function_type(param_ty.unwrap_forall())?;
        let param_ret = param_fn.get_function_ret()?;
        let param_origin = self.function_type_origin(param_ty, SlotOrigin::Lisette);
        let param_abi = self.slot_return_abi(param_ret, param_origin);

        let arg_ty = arg.get_type();
        let arg_fn = self
            .facts
            .resolve_to_function_type(arg_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_origin = if is_closure_literal(arg) || self.is_go_callable(arg) {
            param_origin
        } else {
            self.function_type_origin(&arg_ty, SlotOrigin::Lisette)
        };
        let arg_abi = self.classify_slot_emission(arg_ret, arg_origin)?;

        Some(FnArgShapes {
            param_abi,
            param_origin,
            arg_fn,
            arg_abi,
        })
    }

    fn detect_lowered_fn_arg_shape(
        &self,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<()> {
        if is_tagged_shape_fn_value(arg) {
            return None;
        }
        let raw_param_ty = generic_param_ty?;
        let shapes = self.fn_arg_shapes(arg, raw_param_ty)?;
        if shapes.param_abi == shapes.arg_abi {
            return None;
        }
        Some(())
    }

    fn lower_adapt_lowered_fn_arg_shape(
        &mut self,
        arg: &Expression,
        generic_param_ty: &Type,
    ) -> Option<ValuePlan> {
        let FnArgShapes {
            param_abi,
            param_origin,
            arg_fn,
            arg_abi,
        } = self.fn_arg_shapes(arg, generic_param_ty)?;
        let argument = self.lower_value(
            arg,
            ExpressionContext::value().with_function_slot_origin(param_origin),
        );
        Some(argument.map_expression_as_computed(|setup, value| {
            emit_fn_arg_shape_adapter(self, setup, value, &arg_fn, &arg_abi, &param_abi)
                .expect("fn_arg_shapes resolved a function signature")
        }))
    }

    /// Adapt `slice...` spread into a generic `VarArgs<fn(...)>` when the
    /// slice's element fn-shape disagrees with the variadic's element.
    pub(crate) fn try_emit_variadic_spread_adapter(
        &mut self,
        spread: &Expression,
        generic_params: Option<&[FunctionParameter]>,
    ) -> Option<ValuePlan> {
        let generic_params = generic_params?;
        let raw_variadic = generic_params.last()?;
        if raw_variadic.ty.get_name() != Some("VarArgs") {
            return None;
        }
        let variadic_inner = raw_variadic.ty.inner()?;
        let param_fn = self
            .facts
            .resolve_to_function_type(variadic_inner.unwrap_forall())?;
        let param_ret = param_fn.get_function_ret()?;
        let param_origin = self.function_type_origin(&variadic_inner, SlotOrigin::Lisette);
        let param_abi = self.slot_return_abi(param_ret, param_origin);

        let spread_ty = spread.get_type();
        let element_ty = spread_ty.unwrap_forall().inner()?;
        let arg_fn = self
            .facts
            .resolve_to_function_type(element_ty.unwrap_forall())?;
        let arg_ret = arg_fn.get_function_ret()?;
        let arg_origin = self.function_type_origin(&element_ty, SlotOrigin::Lisette);
        let arg_abi = self.classify_slot_emission(arg_ret, arg_origin)?;

        if param_abi == arg_abi {
            return None;
        }

        let source = self
            .lower_value(spread, ExpressionContext::value())
            .map_expression_as_name(|setup, source_value| {
                GoExpression::name(self.hoist_tmp_value_statement(setup, "src", source_value))
            });
        let source_variable = source.expression.clone();

        let target_element_ret = self.render_lowered_return_ty(&param_abi, arg_ret);
        let arg_fn_params = arg_fn.get_function_params().unwrap_or(&[]);
        let param_type_strs: Vec<String> = arg_fn_params
            .iter()
            .map(|param| self.use_go_type(&param.ty))
            .collect();
        let target_element_ty = format!(
            "func({}) {}",
            param_type_strs.join(", "),
            target_element_ret
        );

        let adapted = self.fresh_var(Some("adapted"));
        self.declare(&adapted);
        let loop_cb = self.fresh_var(Some("cb"));

        let mut body = Vec::new();
        let closure = emit_fn_arg_shape_adapter(
            self,
            &mut body,
            GoExpression::name(loop_cb.clone()),
            &arg_fn,
            &arg_abi,
            &param_abi,
        )?;
        body.push(assign(
            GoExpression::index(
                GoExpression::name(adapted.clone()),
                GoExpression::name("i".to_string()),
            ),
            closure,
        ));

        Some(source.map_expression_as_name(|setup, _source_value| {
            setup.push(define(
                adapted.clone(),
                GoExpression::call(
                    GoExpression::name("make".to_string()),
                    vec![
                        GoExpression::type_name(format!("[]{target_element_ty}")),
                        GoExpression::call(
                            GoExpression::name("len".to_string()),
                            vec![source_variable.clone()],
                        ),
                    ],
                ),
            ));
            setup.push(LoweredStatement::Loop(LoopPlan {
                prologue: Vec::new(),
                kind: LoopKind::Generated { label: None },
                header: LoopHeader::Range {
                    key: Some("i".to_string()),
                    value: Some(loop_cb),
                    iterable: source_variable,
                },
                body: LoweredBlock { statements: body },
            }));
            GoExpression::name(adapted)
        }))
    }

    /// Resolve the source and target callback contracts at a Go call boundary.
    /// A closure literal compiles to the slot's shape and needs no adapter.
    fn detect_callback_wrapper(
        &self,
        arg: &Expression,
        param: Option<&CallableParamAbi>,
    ) -> Option<(CallableReturnAbi, CallableReturnAbi, AbiTransition)> {
        if is_closure_literal(arg) {
            return None;
        }
        let param = param?;
        let param_fn_ty = self
            .facts
            .resolve_to_function_type(param.instantiated.unwrap_forall())
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
        let target = self.classify_slot_emission(&param_f.return_type, param.origin)?;
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
        argument.map_expression_as_computed(|setup, value| match transition {
            AbiTransition::Identity => value,
            AbiTransition::LowerFromTagged => {
                let param_fn_ty = self
                    .facts
                    .resolve_to_function_type(effective_param_ty.unwrap_forall())
                    .expect("callback target resolves to a fn type");
                emit_lisette_callback_wrapper(self, setup, value, &param_fn_ty)
            }
            AbiTransition::WrapToTagged | AbiTransition::Reencode => {
                let arg_fn_ty = self
                    .facts
                    .resolve_to_function_type(arg.get_type().unwrap_forall())
                    .expect("callback source resolves to a fn type");
                emit_fn_arg_shape_adapter(self, setup, value, &arg_fn_ty, source, target)
                    .expect("callback ABI transition has a function signature")
            }
            AbiTransition::Incompatible => {
                unreachable!("type-checked callback ABIs must describe the same result")
            }
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

    fn argument_source_layout(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> ValueLayout {
        let origin = if is_closure_literal(argument) {
            self.function_type_origin(&parameter.instantiated, parameter.origin)
        } else {
            SlotOrigin::Lisette
        };
        self.value_layout(&argument.get_type(), origin)
    }

    fn argument_needs_slot_bridge(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> bool {
        let physical_source = self.go_physical_expression_layout(argument);
        let source = physical_source
            .clone()
            .unwrap_or_else(|| self.argument_source_layout(argument, parameter));
        let target = self.argument_slot_layout(parameter);
        let can_forward_physical = match (&physical_source, &target) {
            (
                Some(ValueLayout::Function { layout: source, .. }),
                ValueLayout::Function { layout: target, .. },
            ) => {
                // `lower_value` re-encodes a Go function value's return itself.
                if source.return_abi != target.return_abi {
                    return false;
                }
                true
            }
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
        let target = self.argument_slot_layout(parameter);
        if let Some(literal) = self.lower_option_literal_into_layout(argument, &target) {
            return literal;
        }
        let raw_source = self.go_physical_expression_layout(argument);
        let source = raw_source
            .clone()
            .unwrap_or_else(|| self.argument_source_layout(argument, parameter));
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
        value.map_expression_as_computed(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced
        })
    }

    fn go_physical_expression_layout(&self, expression: &Expression) -> Option<ValueLayout> {
        if self.call_target_is_go(expression)
            && let Some(plan) = self.plan_call(expression)
            && matches!(plan.resolved.abi.result, CallableReturnAbi::Direct)
        {
            return Some(plan.resolved.abi.return_layout.clone());
        }
        if !self.is_go_callable(expression) {
            return None;
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
        Some(value.map_expression_as_computed(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced
        }))
    }
}

/// The single Go type inside a rendered `[T]` list, which becomes a conversion
/// around a builtin call. `None` for an empty or multi-parameter list.
fn callee_curries_receiver(callee: &ResolvedCallee<'_>) -> bool {
    let Some(Type::Forall { body, .. }) = callee.declared_type() else {
        return false;
    };
    let Type::Function(declared_fn) = body.as_ref() else {
        return false;
    };
    callee
        .instantiated
        .as_function_type()
        .is_some_and(|instantiated_fn| instantiated_fn.params.len() < declared_fn.params.len())
}

/// The element type of a `VarArgs<T>`, or the type itself when not variadic.
fn varargs_inner_or_self(ty: &Type) -> Type {
    if ty.get_name() == Some("VarArgs") {
        ty.inner().unwrap_or_else(|| ty.clone())
    } else {
        ty.clone()
    }
}

fn spread_needs_any_wrap(
    facts: &crate::EmitFacts<'_>,
    function: &Expression,
    spread: Option<&Expression>,
) -> bool {
    let Some(spread_expr) = spread else {
        return false;
    };
    let Some(function_ty) = facts.resolve_to_function_type(&function.get_type()) else {
        return false;
    };
    let Some(variadic_element) = function_ty.is_variadic() else {
        return false;
    };
    if !facts.resolves_to_unknown(&variadic_element) {
        return false;
    }
    spread_expr
        .get_type()
        .inner()
        .is_some_and(|ty| !facts.resolves_to_unknown(&ty))
}

fn would_suppress_tagged_go(callee: &ResolvedCallee<'_>, declared_param_ty: Option<&Type>) -> bool {
    let unwrapped = declared_param_ty.map(|p| p.unwrap_forall());
    callee.is_prelude_dispatch && unwrapped.is_some_and(|p| matches!(p, Type::Function(_)))
}
