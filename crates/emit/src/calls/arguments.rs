use crate::EmitFacts;
use crate::abi::ReturnSlot;
use crate::abi::is_closure_literal;
use crate::abi::wrapping::NilGuard;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::EcoString;

use crate::Planner;
use crate::abi::callable::{CallableParamAbi, CallableReturnAbi};
use crate::abi::coercion::{CoercionPlan, LayoutBridge};
use crate::abi::layout::ValueLayout;
use crate::expressions::staging::{SpreadSequenceOptions, takes_prelude_callback};
use crate::ir::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign, define,
};
use crate::ir::values::{CaptureBoundary, GoExpression, SequencedValues, ValuePlan};
use crate::names::generics::extract_type_mapping;
use crate::plan::calls::{
    ArgumentPlan, ArgumentSlotBridge, ArgumentValueSource, CallPlan, CallableOrigin,
    FunctionBridge, ResolvedCallee,
};
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::program::{Adaptation, SlotOrigin};
use syntax::types::Type;

pub(super) struct CallArgsContext<'plan, 'facts> {
    pub(super) plan: &'plan CallPlan<'facts>,
    pub(super) spread: Option<&'plan Expression>,
    pub(super) capture_boundary: CaptureBoundary,
    pub(super) retired_receiver: Option<&'plan Expression>,
    pub(super) callee_is_builtin: bool,
    pub(super) callee_pins_type_args: bool,
    pub(super) receiver_binding: Option<(Type, Type)>,
}

impl CallArgsContext<'_, '_> {
    fn receiver_bound_parameters(&self) -> HashSet<String> {
        let mut mapping: HashMap<String, Type> = HashMap::default();
        if let Some((declared, instantiated)) = &self.receiver_binding {
            extract_type_mapping(declared, instantiated, &mut mapping);
        }
        mapping.into_keys().collect()
    }
}

impl Planner<'_> {
    pub(super) fn plan_call_args(
        &mut self,
        args: &[Expression],
        ctx: &CallArgsContext<'_, '_>,
        leading: Option<ValuePlan>,
    ) -> SequencedValues {
        let stages: Vec<ValuePlan> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.plan_call_arg(arg, i, ctx))
            .collect();
        let stages = self.type_constant_arguments(stages, ctx);
        let extra_leading = usize::from(leading.is_some());
        let stages: Vec<ValuePlan> = leading.into_iter().chain(stages).collect();
        let variadic = ctx.plan.resolved.abi.variadic_param();
        let spread_stage = ctx.spread.map(|spread| {
            self.try_plan_variadic_spread_adapter(spread, variadic)
                .or_else(|| self.try_plan_variadic_spread_slot_bridge(spread, variadic))
                .unwrap_or_else(|| self.plan_operand(spread, ExpressionContext::value()))
        });
        self.sequence_with_spread_values(
            stages,
            spread_stage,
            SpreadSequenceOptions {
                wrap_to_any: ctx
                    .plan
                    .resolved
                    .abi
                    .spread_wraps_to_any(&self.facts, ctx.spread),
                combine: ctx.plan.resolved.abi.variadic_combine(extra_leading),
                boundary: ctx.capture_boundary,
            },
        )
    }

    fn plan_call_arg(
        &mut self,
        argument: &Expression,
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
            ArgumentPlan::Bridge(bridge) => self.plan_go_slot_bridge(argument, bridge),
            ArgumentPlan::Direct => self.plan_direct_arg(argument, ctx, param, declared_param_ty),
        }
    }

    pub(crate) fn plan_argument(
        &self,
        argument: &Expression,
        callee: &ResolvedCallee<'_>,
        parameter: Option<&CallableParamAbi>,
    ) -> ArgumentPlan {
        let effective_param_ty = parameter.map(|parameter| &parameter.instantiated);
        let declared_param_ty = parameter.and_then(|parameter| parameter.declared.as_ref());
        if matches!(callee.origin, CallableOrigin::GoInterop)
            && let Some(bridge) = self.detect_callback_wrapper(argument, parameter)
        {
            return ArgumentPlan::Bridge(Box::new(bridge.into_argument(argument.get_type())));
        }
        if would_suppress_tagged_go(callee, declared_param_ty) {
            return match self.detect_lower_arg_to_tagged(argument, effective_param_ty) {
                Some(adapter) => {
                    ArgumentPlan::Bridge(Box::new(adapter.into_argument(argument.get_type())))
                }
                None => ArgumentPlan::Direct,
            };
        }
        if let Some(bridge) =
            parameter.and_then(|parameter| self.plan_argument_slot_bridge(argument, parameter))
        {
            return ArgumentPlan::Bridge(Box::new(bridge));
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
        let mut bound = ctx.receiver_bound_parameters();
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

    fn plan_direct_arg(
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
        let argument = self.plan_composite_value(arg, arg_ctx);
        let Some(param) = param else {
            return argument;
        };
        let coercion = CoercionPlan::checked(self, arg, Some(&param.layout));
        let receiver_binds = matches!(&param.declared, Some(Type::Parameter(name))
            if ctx.receiver_bound_parameters().contains(name.as_str()));
        let inferred = !ctx.callee_pins_type_args
            && !receiver_binds
            && self.facts.converts_for_inference(arg, param);
        if coercion.is_identity() && !inferred {
            return argument;
        }
        argument.map_expression(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            if inferred {
                GoExpression::conversion(self.go_type(&param.instantiated), coerced)
            } else {
                coerced
            }
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

    fn try_plan_variadic_spread_adapter(
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
        } = self.resolve_layout_bridge(&element, &parameter.layout)
        else {
            return None;
        };
        if source_layout.return_abi.lowered().is_none()
            || source_layout.return_abi == target_layout.return_abi
        {
            return None;
        }

        let source = self
            .plan_value(spread, ExpressionContext::value())
            .map_expression(|setup, source_value| {
                GoExpression::identifier(self.hoist_tmp_value_statement(setup, "src", source_value))
            });
        let source_variable = source.expression().clone();

        let target_element_ty = target_layout.go_type(self);

        let adapted = self.fresh_declared("adapted");
        let loop_cb = self.fresh_local(Some("cb"));

        let index = self.scope.helper_local("i", self.package);
        let mut body = Vec::new();
        let closure = self.plan_function_layout_bridge(
            &mut body,
            GoExpression::identifier(loop_cb.clone()),
            &source_layout,
            &target_layout,
            false,
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
                        GoExpression::type_name(GoType::slice_of(&target_element_ty)),
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
        argument: &Expression,
        parameter: Option<&CallableParamAbi>,
    ) -> Option<FunctionBridge> {
        if is_closure_literal(argument) {
            return None;
        }
        let parameter = parameter?;
        let param_fn_ty = self
            .facts
            .resolve_to_function_type(parameter.instantiated.unwrap_forall())?;

        let Type::Function(param_f) = &param_fn_ty else {
            return None;
        };
        let CallableReturnAbi::Lowered(target) = self.facts.return_abi(
            &param_f.return_type,
            ReturnSlot::FunctionValue(parameter.origin),
        ) else {
            return None;
        };
        let source = self
            .resolve_callable_value(argument)?
            .abi
            .function_layout(self);
        if !source.return_abi.is_lowered()
            || source.return_abi == CallableReturnAbi::Lowered(target.clone())
        {
            return None;
        }
        let layout = self.value_layout(&param_fn_ty, SlotOrigin::Lisette);
        let ValueLayout::Function { layout, .. } = layout.underlying() else {
            return None;
        };
        let target =
            layout.with_return_abi(self, CallableReturnAbi::Lowered(target), parameter.origin);
        Some(FunctionBridge { source, target })
    }

    fn plan_argument_slot_bridge(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> Option<ArgumentSlotBridge> {
        if matches!(
            self.facts.checked(argument).value_use().adaptation,
            Adaptation::ImplicitSome { .. }
        ) && matches!(
            parameter.layout.underlying(),
            ValueLayout::TaggedOption { .. }
        ) {
            return None;
        }
        if let Some(source) = self.raw_go_slot_layout(argument)
            && source.same_representation(&parameter.layout)
        {
            return Some(ArgumentSlotBridge {
                target: parameter.layout.clone(),
                bridge: LayoutBridge::Identity,
                source: ArgumentValueSource::GoSlot,
            });
        }
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
            .unwrap_or_else(|| self.emitted_value_layout(argument, ExpressionContext::value()));
        let target = parameter.layout.clone();
        let bridge = self.resolve_layout_bridge(&source, &target);
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

    fn plan_go_slot_bridge(
        &mut self,
        argument: &Expression,
        plan: &ArgumentSlotBridge,
    ) -> ValuePlan {
        if let Some(literal) = self.try_plan_option_literal_into_layout(argument, &plan.target) {
            return literal;
        }
        let value = match plan.source {
            ArgumentValueSource::GoPhysical => {
                if matches!(argument.unwrap_parens(), Expression::Call { .. }) {
                    self.plan_call_expression(
                        argument,
                        Some(&argument.get_type()),
                        ExpressionContext::value(),
                    )
                } else {
                    self.plan_operand(argument, ExpressionContext::value())
                }
            }
            ArgumentValueSource::Callable => self.plan_operand(
                argument,
                ExpressionContext::value().with_forced_tagged_go_function(true),
            ),
            ArgumentValueSource::GoSlot => self.plan_raw_nullable_field(argument),
            ArgumentValueSource::Lisette => self.plan_value(argument, ExpressionContext::value()),
        };
        value.map_expression(|setup, value| {
            let mut coercion_setup = Vec::new();
            let coerced = self.plan_layout_bridge(&mut coercion_setup, value, &plan.bridge, true);
            setup.extend(coercion_setup);
            coerced
        })
    }

    fn raw_go_slot_layout(&self, expression: &Expression) -> Option<ValueLayout> {
        let Expression::DotAccess {
            expression: base,
            member,
            ty,
            resolution,
            ..
        } = expression.unwrap_parens()
        else {
            return None;
        };
        let raw = self.field_slot_layout(
            &base.get_type(),
            resolution.declaring_type(),
            None,
            member,
            ty,
        )?;
        let ValueLayout::NullableOption { payload, .. } = raw.underlying() else {
            return None;
        };
        let passes_unchanged = !matches!(
            self.facts.nullable_option_nil_guard(ty),
            Some(NilGuard::Interface)
        ) && self
            .value_layout(ty, SlotOrigin::Lisette)
            .payload()
            .is_some_and(|lisette| lisette.same_representation(payload));
        passes_unchanged.then_some(raw)
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

    fn try_plan_variadic_spread_slot_bridge(
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
            self.plan_call_expression(spread, Some(&spread.get_type()), ExpressionContext::value())
        } else {
            self.plan_value(spread, ExpressionContext::value())
        };
        Some(value.map_expression(|setup, value| {
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced
        }))
    }
}

fn would_suppress_tagged_go(callee: &ResolvedCallee<'_>, declared_param_ty: Option<&Type>) -> bool {
    callee.is_prelude_dispatch && takes_prelude_callback(declared_param_ty)
}

impl EmitFacts<'_> {
    pub(crate) fn converts_for_inference(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> bool {
        if !matches!(parameter.declared, Some(Type::Parameter(_))) {
            return false;
        }
        let checked = self.checked(argument.unwrap_parens());
        let value_use = match argument.unwrap_parens() {
            Expression::Cast { .. } => checked.cast_operand(),
            _ => checked.value_use(),
        };
        matches!(value_use.adaptation, Adaptation::Interface(_))
    }
}
