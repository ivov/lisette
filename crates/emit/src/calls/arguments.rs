use crate::abi::is_closure_literal;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::EcoString;

use crate::Planner;
use crate::abi::callable::{CallableParamAbi, CallableReturnAbi, LoweredReturnAbi};
use crate::abi::coercion::{CoercionPlan, LayoutBridge, resolve_layout_bridge};
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::transition::emit_fn_arg_shape_adapter;
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::SpreadSequenceOptions;
use crate::names::generics::extract_type_mapping;
use crate::patterns::matching::{PreludeVariant, prelude_constructor};
use crate::plan::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign, define,
};
use crate::plan::calls::{
    ArgumentPlan, ArgumentSlotBridge, ArgumentValueSource, CallPlan, CallableOrigin, ResolvedCallee,
};
use crate::plan::values::{CaptureBoundary, GoExpression, SequencedValues, ValuePlan};
use syntax::ast::Expression;
use syntax::types::Type;

pub(super) struct CallArgsContext<'plan, 'facts> {
    pub(super) plan: &'plan CallPlan<'facts>,
    pub(super) spread: Option<&'plan Expression>,
    pub(super) wrap_spread_to_any: bool,
    pub(super) capture_boundary: CaptureBoundary,
    pub(super) retired_receiver: Option<&'plan Expression>,
    pub(super) callee_is_builtin: bool,
    pub(super) callee_pins_type_args: bool,
    pub(super) receiver_binding: Option<(Type, Type)>,
}

impl Planner<'_> {
    /// Lower and sequence a call's arguments, `leading` stage first.
    pub(super) fn emit_call_args(
        &mut self,
        args: &[Expression],
        ctx: &CallArgsContext<'_, '_>,
        leading: Option<ValuePlan>,
    ) -> SequencedValues {
        let stages: Vec<ValuePlan> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.lower_call_arg(arg, i, ctx))
            .collect();
        let stages = self.type_constant_arguments(stages, ctx);
        let extra_leading = usize::from(leading.is_some());
        let stages: Vec<ValuePlan> = leading.into_iter().chain(stages).collect();
        let variadic = ctx.plan.resolved.abi.variadic_param();
        let spread_stage = ctx.spread.map(|spread| {
            self.try_emit_variadic_spread_adapter(spread, variadic)
                .or_else(|| self.lower_variadic_spread_slot_bridge(spread, variadic))
                .unwrap_or_else(|| self.plan_operand(spread, ExpressionContext::value()))
        });
        self.sequence_with_spread_values(
            stages,
            spread_stage,
            SpreadSequenceOptions {
                wrap_to_any: ctx.wrap_spread_to_any,
                combine: ctx.plan.resolved.abi.variadic_combine(extra_leading),
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
        let declared_param_ty = param.and_then(|param| param.declared.as_ref());

        let plan = ctx
            .plan
            .arguments
            .get(index)
            .expect("CallPlan has one argument plan per argument");

        match plan {
            ArgumentPlan::GoCallbackAdapter { source, target } => {
                self.lower_callback_wrapper(arg, source, target)
            }
            ArgumentPlan::GoSlotBridge(bridge) => self.lower_go_slot_bridge(arg, bridge),
            ArgumentPlan::TaggedGoLowering(adapter) => {
                let arg_ctx = self.direct_arg_emit_ctx(param, true);
                let argument = self.lower_composite_value(arg, arg_ctx);
                argument.map_expression(|setup, value| {
                    self.emit_lower_arg_to_tagged(setup, value, adapter)
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
            && let Some((source, target)) = self.detect_callback_wrapper(arg, param)
        {
            return ArgumentPlan::GoCallbackAdapter { source, target };
        }
        // Prelude callbacks return tagged values even when their instantiated
        // result could use a nullable or multi-value ABI in a Lisette function.
        if would_suppress_tagged_go(callee, declared_param_ty) {
            return match self.detect_lower_arg_to_tagged(arg, effective_param_ty) {
                Some(adapter) => ArgumentPlan::TaggedGoLowering(Box::new(adapter)),
                None => ArgumentPlan::Direct,
            };
        }
        if let Some(bridge) = param.and_then(|param| self.plan_argument_slot_bridge(arg, param)) {
            return ArgumentPlan::GoSlotBridge(Box::new(bridge));
        }
        ArgumentPlan::Direct
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
        let mut bound: HashSet<String> = HashSet::default();
        if let Some((declared, instantiated)) = &ctx.receiver_binding {
            let mut mapping: HashMap<String, Type> = HashMap::default();
            extract_type_mapping(declared, instantiated, &mut mapping);
            bound.extend(mapping.into_keys());
        }
        for (stage, slot) in stages.iter().zip(&slots) {
            if stage.expression().constant_kind().is_some() {
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
            .zip(&slots)
            .enumerate()
            .map(|(index, (stage, slot))| {
                let Some((declared, instantiated)) = slot else {
                    return stage;
                };
                let constant = stage.expression().constant_kind();
                let direct = matches!(ctx.plan.arguments.get(index), Some(ArgumentPlan::Direct));
                if !direct || constant.is_none() {
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
                match self.constant_needs_go_type(constant, instantiated) {
                    Some(go_type) => stage.conversion(go_type),
                    None => stage,
                }
            })
            .collect()
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
        param: Option<&'b CallableParamAbi>,
        suppress: bool,
    ) -> ExpressionContext<'b> {
        let origin = param.map_or(SlotOrigin::Lisette, |param| {
            self.function_type_origin(&param.instantiated, param.origin)
        });
        ExpressionContext::value()
            .with_function_slot_origin(origin)
            .with_forced_tagged_go_function(suppress)
            .with_generic_result_slot(
                param.and_then(|param| param.declared.as_ref()),
                param.map(|param| &param.instantiated),
            )
    }

    fn try_emit_variadic_spread_adapter(
        &mut self,
        spread: &Expression,
        parameter: Option<&CallableParamAbi>,
    ) -> Option<ValuePlan> {
        if self.go_physical_expression_layout(spread).is_some() {
            return None;
        }
        let parameter = parameter?;
        let element_ty = spread.get_type().unwrap_forall().inner()?;
        let element = self.value_layout(&element_ty, SlotOrigin::Lisette);
        let LayoutBridge::Function {
            source: source_layout,
            target: target_layout,
        } = resolve_layout_bridge(self, &element, &parameter.layout)
        else {
            return None;
        };
        if source_layout.return_abi.lowered().is_none()
            || source_layout.return_abi == target_layout.return_abi
        {
            return None;
        }

        let source = self
            .lower_value(spread, ExpressionContext::value())
            .map_expression(|setup, source_value| {
                GoExpression::identifier(self.hoist_tmp_value_statement(setup, "src", source_value))
            });
        let source_variable = source.expression().clone();

        let target_element_ty = target_layout.go_type(self);
        let target_element_ty = self.use_rendered_go_type(target_element_ty);

        let adapted = self.fresh_local(Some("adapted"));
        self.declare(&adapted);
        let loop_cb = self.fresh_local(Some("cb"));

        let index = self.scope.helper_local("i", &self.package);
        let mut body = Vec::new();
        let closure = self.plan_function_layout_bridge(
            &mut body,
            GoExpression::identifier(loop_cb.clone()),
            &source_layout,
            &target_layout,
        );
        body.push(assign(
            GoExpression::index(
                GoExpression::identifier(adapted.clone()),
                GoExpression::identifier(index.clone()),
            ),
            closure,
        ));

        Some(source.map_expression(|setup, _source_value| {
            setup.push(define(
                adapted.clone(),
                GoExpression::call(
                    GoExpression::external_name("make".to_string()),
                    vec![
                        GoExpression::type_name(format!("[]{target_element_ty}")),
                        GoExpression::call(
                            GoExpression::external_name("len".to_string()),
                            vec![source_variable.clone()],
                        ),
                    ],
                ),
            ));
            setup.push(
                LoweredStatement::Loop(LoopPlan {
                    kind: LoopKind::Generated { label: None },
                    header: LoopHeader::Range {
                        key: Some(index),
                        value: Some(loop_cb),
                        iterable: source_variable,
                    },
                    body: LoweredBlock { statements: body },
                })
                .into(),
            );
            GoExpression::identifier(adapted)
        }))
    }

    fn detect_callback_wrapper(
        &self,
        arg: &Expression,
        param: Option<&CallableParamAbi>,
    ) -> Option<(CallableReturnAbi, LoweredReturnAbi)> {
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
        let source = self.resolve_callable_value(arg)?.abi.result;
        (matches!(source, CallableReturnAbi::Lowered(_))
            && source != CallableReturnAbi::Lowered(target.clone()))
        .then_some((source, target))
    }

    fn lower_callback_wrapper(
        &mut self,
        arg: &Expression,
        source: &CallableReturnAbi,
        target: &LoweredReturnAbi,
    ) -> ValuePlan {
        let argument = self.plan_operand(
            arg,
            ExpressionContext::value().with_forced_tagged_go_function(true),
        );
        argument.map_expression(|setup, value| {
            let arg_fn_ty = self
                .facts
                .resolve_to_function_type(arg.get_type().unwrap_forall())
                .expect("callback source resolves to a fn type");
            let target = CallableReturnAbi::Lowered(target.clone());
            emit_fn_arg_shape_adapter(self, setup, value, &arg_fn_ty, source, &target)
                .expect("callback ABI transition has a function signature")
        })
    }

    fn plan_argument_slot_bridge(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> Option<ArgumentSlotBridge> {
        let physical_source = self.go_physical_expression_layout(argument);
        if physical_source.is_none()
            && self
                .emitted_value_layout(argument, self.direct_arg_emit_ctx(Some(parameter), false))
                .same_representation(&parameter.layout)
        {
            return None;
        }
        let source = physical_source
            .clone()
            // Slot bridges lower their source in a Lisette value context;
            // only direct arguments emit a closure into the Go slot itself.
            .unwrap_or_else(|| self.emitted_value_layout(argument, ExpressionContext::value()));
        let target = parameter.layout.clone();
        let bridge = resolve_layout_bridge(self, &source, &target);
        (physical_source.is_some() || !bridge.is_identity()).then_some(ArgumentSlotBridge {
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
        if prelude_constructor(argument) == Some(PreludeVariant::None)
            && matches!(
                plan.target,
                ValueLayout::NullableOption { .. } | ValueLayout::PointerOption { .. }
            )
        {
            return ValuePlan::literal("nil".to_string());
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

    pub(crate) fn go_physical_expression_layout(
        &self,
        expression: &Expression,
    ) -> Option<ValueLayout> {
        if self.call_target_is_go(expression)
            && let Some(plan) = self.plan_call(expression)
            && matches!(plan.resolved.abi.result, CallableReturnAbi::Value)
        {
            return Some(plan.resolved.abi.return_layout.clone());
        }
        if !self.is_go_callable(expression) {
            return None;
        }
        let callable = self.resolve_callable_value(expression)?;
        matches!(callable.origin, CallableOrigin::GoInterop).then(|| ValueLayout::Function {
            function_type: expression.get_type(),
            layout: callable.abi.function_layout(self),
        })
    }

    fn lower_variadic_spread_slot_bridge(
        &mut self,
        spread: &Expression,
        parameter: Option<&CallableParamAbi>,
    ) -> Option<ValuePlan> {
        let parameter = parameter?;
        parameter.variadic.as_ref()?;

        let raw_source = self.go_physical_expression_layout(spread);
        let source = raw_source
            .clone()
            .unwrap_or_else(|| self.value_layout(&spread.get_type(), SlotOrigin::Lisette));
        let target = ValueLayout::Slice {
            collection_type: spread.get_type(),
            element: Box::new(parameter.layout.clone()),
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

fn would_suppress_tagged_go(callee: &ResolvedCallee<'_>, declared_param_ty: Option<&Type>) -> bool {
    let unwrapped = declared_param_ty.map(|p| p.unwrap_forall());
    callee.is_prelude_dispatch && unwrapped.is_some_and(|p| matches!(p, Type::Function(_)))
}
