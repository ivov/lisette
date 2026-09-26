use crate::abi::{is_closure_literal, is_tagged_shape_fn_value};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::EcoString;
use syntax::types::FunctionParameter;

use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableParamAbi, CallableReturnAbi};
use crate::abi::coercion::{CoercionPlan, resolve_layout_bridge};
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::transition::{emit_fn_arg_shape_adapter, emit_lisette_callback_wrapper};
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::{SpreadSequenceOptions, VariadicCombine};
use crate::names::generics::extract_type_mapping;
use crate::plan::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign, define,
};
use crate::plan::calls::{
    ArgumentPlan, ArgumentSlotBridge, ArgumentValueSource, CallPlan, CallableOrigin,
    FunctionArgumentAdapter, ResolvedCallee,
};
use crate::plan::values::{
    CaptureBoundary, EvaluationEffect, GoExpression, SequencedValues, ValuePlan,
};
use syntax::ast::Expression;
use syntax::types::Type;

pub(super) struct CallArgsContext<'plan, 'facts> {
    pub(super) plan: &'plan CallPlan<'facts>,
    pub(super) spread: Option<&'plan Expression>,
    pub(super) wrap_spread_to_any: bool,
    pub(super) combine_variadic: Option<VariadicCombine>,
    pub(super) capture_boundary: CaptureBoundary,
    pub(super) retired_receiver: Option<&'plan Expression>,
    pub(super) callee_is_builtin: bool,
    pub(super) callee_pins_type_args: bool,
    pub(super) receiver_binding: Option<(Type, Type)>,
}

impl Planner<'_> {
    pub(super) fn emit_call_args(
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
            let mut sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
            self.finalize_spread_stage(
                &mut sequenced.values,
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

    fn lower_call_arg(
        &mut self,
        arg: &Expression,
        index: usize,
        ctx: &CallArgsContext<'_, '_>,
    ) -> ValuePlan {
        let param = ctx.plan.resolved.abi.param(index);
        let effective_param_ty = param.map(|param| &param.instantiated);
        let declared_param_ty = param.and_then(|param| param.declared.as_ref());

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
            ArgumentPlan::LoweredFnShapeAdapter(adapter) => {
                self.lower_function_argument_adapter(arg, adapter)
            }
            ArgumentPlan::GoSlotBridge(bridge) => self.lower_go_slot_bridge(arg, bridge),
            ArgumentPlan::TaggedGoLowering => {
                let target =
                    effective_param_ty.expect("TaggedGoLowering requires effective_param_ty");
                let arg_ctx = self.direct_arg_emit_ctx(param, true);
                let argument = self.lower_composite_value(arg, arg_ctx);
                argument.map_expression(|setup, value| {
                    self.emit_lower_arg_to_tagged(setup, value, target)
                })
            }
            ArgumentPlan::Direct => self.lower_direct_arg(arg, ctx, param, declared_param_ty),
        }
    }

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
        if let Some(adapter) = self.plan_function_argument_adapter(arg, declared_param_ty) {
            return ArgumentPlan::LoweredFnShapeAdapter(Box::new(adapter));
        }
        if let Some(bridge) = param.and_then(|param| self.plan_argument_slot_bridge(arg, param)) {
            return ArgumentPlan::GoSlotBridge(Box::new(bridge));
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
        argument.map_expression(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced
        })
    }

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

    pub(crate) fn try_adapt_lowered_fn_arg_shape(
        &mut self,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<ValuePlan> {
        let adapter = self.plan_function_argument_adapter(arg, generic_param_ty)?;
        Some(self.lower_function_argument_adapter(arg, &adapter))
    }

    fn plan_function_argument_adapter(
        &self,
        arg: &Expression,
        generic_param_ty: Option<&Type>,
    ) -> Option<FunctionArgumentAdapter> {
        if is_tagged_shape_fn_value(arg) {
            return None;
        }
        let raw_param_ty = generic_param_ty?;
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

        (param_abi != arg_abi).then_some(FunctionArgumentAdapter {
            source_function: arg_fn,
            source_abi: arg_abi,
            target_abi: param_abi,
            target_origin: param_origin,
        })
    }

    fn lower_function_argument_adapter(
        &mut self,
        arg: &Expression,
        adapter: &FunctionArgumentAdapter,
    ) -> ValuePlan {
        let FunctionArgumentAdapter {
            source_function,
            source_abi,
            target_abi,
            target_origin,
        } = adapter;
        let argument = self.lower_value(
            arg,
            ExpressionContext::value().with_function_slot_origin(*target_origin),
        );
        argument.map_expression(|setup, value| {
            emit_fn_arg_shape_adapter(self, setup, value, source_function, source_abi, target_abi)
                .expect("argument adapter contains a function signature")
        })
    }

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
            .map_expression(|setup, source_value| {
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

        Some(source.map_expression(|setup, _source_value| {
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

    fn detect_callback_wrapper(
        &self,
        arg: &Expression,
        param: Option<&CallableParamAbi>,
    ) -> Option<(CallableReturnAbi, CallableReturnAbi, AbiTransition)> {
        // Closures already return values in the form expected by the parameter.
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
        argument.map_expression(|setup, value| match transition {
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

    fn plan_argument_slot_bridge(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> Option<ArgumentSlotBridge> {
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
                // `lower_value` already converts the function's return values.
                if source.return_abi != target.return_abi {
                    return None;
                }
                true
            }
            (Some(_), _) => true,
            (None, _) => false,
        };
        let bridge = resolve_layout_bridge(self, &source, &target);
        (can_forward_physical || !bridge.is_identity()).then_some(ArgumentSlotBridge {
            target,
            bridge,
            source: if physical_source.is_some() {
                ArgumentValueSource::GoPhysical
            } else {
                ArgumentValueSource::Lisette
            },
        })
    }

    fn lower_go_slot_bridge(
        &mut self,
        argument: &Expression,
        plan: &ArgumentSlotBridge,
    ) -> ValuePlan {
        if argument.is_none_literal() {
            return ValuePlan::evaluated_literal(
                Vec::new(),
                "nil".to_string(),
                EvaluationEffect::PureCall,
            );
        }
        if let Some(literal) = self.lower_option_literal_into_layout(argument, &plan.target) {
            return literal;
        }
        let value = match plan.source {
            ArgumentValueSource::GoPhysical => {
                if matches!(argument.unwrap_parens(), Expression::Call { .. }) {
                    self.lower_call(
                        argument,
                        Some(&argument.get_type()),
                        ExpressionContext::value(),
                    )
                } else {
                    self.plan_operand(argument, ExpressionContext::value())
                }
            }
            ArgumentValueSource::Lisette => self.lower_value(argument, ExpressionContext::value()),
        };
        value.map_expression(|setup, value| {
            let mut coercion_setup = Vec::new();
            let coerced = self.plan_layout_bridge(&mut coercion_setup, value, &plan.bridge);
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
        Some(value.map_expression(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced
        }))
    }
}

fn varargs_inner_or_self(ty: &Type) -> Type {
    if ty.get_name() == Some("VarArgs") {
        ty.inner().unwrap_or_else(|| ty.clone())
    } else {
        ty.clone()
    }
}

fn would_suppress_tagged_go(callee: &ResolvedCallee<'_>, declared_param_ty: Option<&Type>) -> bool {
    let unwrapped = declared_param_ty.map(|p| p.unwrap_forall());
    callee.is_prelude_dispatch && unwrapped.is_some_and(|p| matches!(p, Type::Function(_)))
}
