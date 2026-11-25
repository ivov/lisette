use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::abi::is_closure_literal;
use crate::lowering::abi::return_abi;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::EcoString;

use crate::Planner;
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lowering::abi::callable::CallableParamAbi;
use crate::lowering::abi::coercion_plan::{CoercionPlan, LayoutChange};
use crate::lowering::calls::call_plan::{
    ArgumentPlan, ArgumentSlotLayoutChange, ArgumentValueSource, CallPlan, CallableOrigin,
    FunctionLayoutChange, ResolvedCallee,
};
use crate::lowering::expressions::operand_order::{SpreadSequenceOptions, takes_prelude_callback};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::{SequencedValues, ValuePlan};
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use crate::target::names::generics::extract_type_mapping;
use crate::target::names::go_name::MemberSpelling;
use crate::target::types::return_abi::CallableReturnAbi;
use syntax::ast::Expression;
use syntax::program::{Adaptation, SlotOrigin};
use syntax::types::Type;

pub(super) struct CallArgumentsContext<'plan, 'facts> {
    pub(super) plan: &'plan CallPlan<'facts>,
    pub(super) spread: Option<&'plan Expression>,
    pub(super) capture_boundary: CaptureBoundary,
    pub(super) assigned_place: Option<&'plan Expression>,
    pub(super) callee_is_builtin: bool,
    pub(super) callee_pins_type_args: bool,
    pub(super) receiver_binding: Option<(Type, Type)>,
}

impl CallArgumentsContext<'_, '_> {
    fn receiver_bound_parameters(&self) -> FxHashSet<String> {
        let mut mapping: FxHashMap<String, Type> = FxHashMap::default();
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
        ctx: &CallArgumentsContext<'_, '_>,
        leading: Option<ValuePlan>,
    ) -> SequencedValues {
        let operands: Vec<ValuePlan> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.plan_call_arg(arg, i, ctx))
            .collect();
        let operands = self.type_constant_arguments(operands, ctx);
        let extra_leading = usize::from(leading.is_some());
        let operands: Vec<ValuePlan> = leading.into_iter().chain(operands).collect();
        let variadic = ctx.plan.resolved.abi.variadic_param();
        let spread_operand = ctx.spread.map(|spread| {
            self.try_plan_variadic_spread_slot_layout_change(spread, variadic)
                .unwrap_or_else(|| self.plan_operand(spread, ExpressionContext::value()))
        });
        self.sequence_with_spread_values(
            operands,
            spread_operand,
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
        ctx: &CallArgumentsContext<'_, '_>,
    ) -> ValuePlan {
        let param = ctx.plan.resolved.abi.param(index);
        let declared_param_ty = param.and_then(|param| param.declared.as_ref());

        let plan = ctx
            .plan
            .arguments
            .get(index)
            .expect("CallPlan has one argument plan per argument");

        match plan {
            ArgumentPlan::LayoutChange(change) => self.plan_go_slot_layout_change(argument, change),
            ArgumentPlan::Direct => self.plan_direct_arg(argument, ctx, param, declared_param_ty),
        }
    }

    pub(in crate::lowering) fn plan_argument(
        &self,
        argument: &Expression,
        callee: &ResolvedCallee<'_>,
        parameter: Option<&CallableParamAbi>,
    ) -> ArgumentPlan {
        let effective_param_ty = parameter.map(|parameter| &parameter.instantiated);
        let declared_param_ty = parameter.and_then(|parameter| parameter.declared.as_ref());
        if matches!(callee.origin, CallableOrigin::GoInterop)
            && let Some(change) = self.detect_callback_wrapper(argument, parameter)
        {
            return ArgumentPlan::LayoutChange(Box::new(change.into_argument(argument.get_type())));
        }
        if would_suppress_tagged_go(callee, declared_param_ty) {
            return match self.detect_lower_arg_to_tagged(argument, effective_param_ty) {
                Some(adapter) => {
                    ArgumentPlan::LayoutChange(Box::new(adapter.into_argument(argument.get_type())))
                }
                None => ArgumentPlan::Direct,
            };
        }
        if let Some(change) = parameter
            .and_then(|parameter| self.plan_argument_slot_layout_change(argument, parameter))
        {
            return ArgumentPlan::LayoutChange(Box::new(change));
        }
        ArgumentPlan::Direct
    }

    fn type_constant_arguments(
        &self,
        operands: Vec<ValuePlan>,
        ctx: &CallArgumentsContext<'_, '_>,
    ) -> Vec<ValuePlan> {
        if ctx.callee_is_builtin || ctx.callee_pins_type_args {
            return operands;
        }
        let abi = &ctx.plan.resolved.abi;
        let vars: Vec<EcoString> = match ctx.plan.resolved.declared_type() {
            Some(Type::Forall { vars, .. }) => vars.clone(),
            _ => Vec::new(),
        };
        let slots: Vec<Option<(&Type, &Type)>> = (0..operands.len())
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
        for (operand, slot) in operands.iter().zip(&slots) {
            if operand.expression().constant_kind().is_some() {
                continue;
            }
            if let Some((declared, instantiated)) = slot {
                let mut mapping: FxHashMap<String, Type> = FxHashMap::default();
                extract_type_mapping(declared, instantiated, &mut mapping);
                bound.extend(mapping.into_keys());
            }
        }
        operands
            .into_iter()
            .zip(&slots)
            .enumerate()
            .map(|(index, (operand, slot))| {
                let Some((declared, instantiated)) = slot else {
                    return operand;
                };
                let constant = operand.expression().constant_kind();
                let direct = matches!(ctx.plan.arguments.get(index), Some(ArgumentPlan::Direct));
                if !direct || constant.is_none() {
                    return operand;
                }
                let mut mapping: FxHashMap<String, Type> = FxHashMap::default();
                extract_type_mapping(declared, instantiated, &mut mapping);
                let inferred = mapping.keys().any(|name| {
                    (vars.is_empty() || vars.iter().any(|var| var == name)) && !bound.contains(name)
                });
                if !inferred {
                    return operand;
                }
                match self.constant_needs_go_type(constant, instantiated) {
                    Some(go_type) => operand.conversion(go_type),
                    None => operand,
                }
            })
            .collect()
    }

    fn plan_direct_arg(
        &mut self,
        arg: &Expression,
        ctx: &CallArgumentsContext<'_, '_>,
        param: Option<&CallableParamAbi>,
        declared_param_ty: Option<&Type>,
    ) -> ValuePlan {
        let spelling = if would_suppress_tagged_go(&ctx.plan.resolved, declared_param_ty) {
            MemberSpelling::TaggedCounterpart
        } else {
            MemberSpelling::Plain
        };
        let mut arg_ctx = self.direct_arg_emit_ctx(param, spelling);
        if let Some(assigned) = ctx.assigned_place {
            arg_ctx = arg_ctx.with_assigned_place(assigned);
        }
        let argument = self.plan_composite_value(arg, arg_ctx);
        let Some(param) = param else {
            return self.convert_to_recorded_slot(arg, argument);
        };
        let coercion = self.slot_coercion(arg, &param.instantiated, Some(&param.layout));
        let inferred = converts_for_inference(&self.facts, arg, param, |name| {
            ctx.callee_pins_type_args || ctx.receiver_bound_parameters().contains(name)
        });
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
        spelling: MemberSpelling,
    ) -> ExpressionContext<'b> {
        let origin = param.map_or(SlotOrigin::Lisette, |param| {
            self.facts
                .function_type_origin(&param.instantiated, param.origin)
        });
        ExpressionContext::value()
            .with_function_slot_origin(origin)
            .with_go_function_spelling(spelling)
            .with_generic_result_slot(
                param.and_then(|param| param.declared.as_ref()),
                param.map(|param| &param.instantiated),
            )
    }

    fn detect_callback_wrapper(
        &self,
        argument: &Expression,
        parameter: Option<&CallableParamAbi>,
    ) -> Option<FunctionLayoutChange> {
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
        let CallableReturnAbi::Lowered(target) = return_abi(
            &self.facts,
            &param_f.return_type,
            ReturnSlot::FunctionValue(parameter.origin),
        ) else {
            return None;
        };

        let source = self
            .resolve_callable_value(argument)?
            .abi
            .function_layout(&self.facts);
        if !source.return_abi.is_lowered()
            || source.return_abi == CallableReturnAbi::Lowered(target.clone())
        {
            return None;
        }
        let layout = self.facts.value_layout(&param_fn_ty, SlotOrigin::Lisette);
        let ValueLayout::Function { layout, .. } = layout.underlying() else {
            return None;
        };
        let target = layout.with_return_abi(
            &self.facts,
            CallableReturnAbi::Lowered(target),
            parameter.origin,
        );
        Some(FunctionLayoutChange { source, target })
    }

    fn plan_argument_slot_layout_change(
        &self,
        argument: &Expression,
        parameter: &CallableParamAbi,
    ) -> Option<ArgumentSlotLayoutChange> {
        let value_use = self.facts.checked(argument).value_use();
        if matches!(
            CoercionPlan::adapted(
                self,
                &value_use.adaptation,
                &argument.get_type(),
                &value_use.slot,
                Some(&parameter.layout),
            ),
            CoercionPlan::WrapSome { .. }
        ) {
            return None;
        }
        if let Some(source) = self.raw_go_slot_layout(argument)
            && source.same_representation(&parameter.layout)
        {
            return Some(ArgumentSlotLayoutChange {
                target: parameter.layout.clone(),
                change: LayoutChange::Identity,
                source: ArgumentValueSource::GoSlot,
            });
        }
        let physical_source = self.go_physical_expression_layout(argument);
        if physical_source.is_none()
            && self
                .emitted_value_layout(
                    argument,
                    self.direct_arg_emit_ctx(Some(parameter), MemberSpelling::Plain),
                )
                .same_representation(&parameter.layout)
        {
            return None;
        }
        let source = physical_source
            .clone()
            .unwrap_or_else(|| self.emitted_value_layout(argument, ExpressionContext::value()));
        let target = parameter.layout.clone();
        let change = self.resolve_layout_change(&source, &target);
        (physical_source.is_some() || change.changes_layout()).then_some(ArgumentSlotLayoutChange {
            target,
            change,
            source: if physical_source.is_some() {
                ArgumentValueSource::GoPhysical
            } else {
                ArgumentValueSource::Lisette
            },
        })
    }

    fn plan_go_slot_layout_change(
        &mut self,
        argument: &Expression,
        plan: &ArgumentSlotLayoutChange,
    ) -> ValuePlan {
        if let Some(literal) = self.try_plan_option_literal_into_layout(argument, &plan.target) {
            return literal;
        }
        let value = match plan.source {
            ArgumentValueSource::GoPhysical => {
                if matches!(argument, Expression::Call { .. }) {
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
                ExpressionContext::value()
                    .with_go_function_spelling(MemberSpelling::TaggedCounterpart),
            ),
            ArgumentValueSource::GoSlot => self.plan_raw_nullable_field(argument),
            ArgumentValueSource::Lisette => self.plan_value(argument, ExpressionContext::value()),
        };

        let slot = &self.facts.checked(argument).value_use().slot;
        let coercion = self.slot_coercion(argument, slot, Some(&plan.target));
        value.map_expression(|setup, value| {
            let mut coercion_setup = Vec::new();
            let changed = self.plan_layout_change(&mut coercion_setup, value, &plan.change);
            let (conversion_setup, coerced) = coercion.lower(self, changed);
            setup.extend(coercion_setup);
            setup.extend(conversion_setup);
            coerced
        })
    }

    fn raw_go_slot_layout(&self, expression: &Expression) -> Option<ValueLayout> {
        let Expression::DotAccess {
            expression: base,
            member,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        let raw = self.facts.field_slot_layout(
            &base.get_type(),
            self.facts.checked(expression).field_selection(),
            member,
            ty,
        )?;

        let ValueLayout::NullableOption { payload, .. } = raw.underlying() else {
            return None;
        };
        let passes_unchanged = !matches!(self.layout_nil_guard(&raw), Some(NilGuard::Interface))
            && self
                .facts
                .value_layout(ty, SlotOrigin::Lisette)
                .payload()
                .is_some_and(|lisette| lisette.same_representation(payload));
        passes_unchanged.then_some(raw)
    }

    pub(in crate::lowering) fn go_physical_expression_layout(
        &self,
        expression: &Expression,
    ) -> Option<ValueLayout> {
        if self.call_target_is_go(expression)
            && let Some(plan) = self.plan_call(expression)
            && matches!(plan.resolved.abi.result, CallableReturnAbi::Value)
        {
            return Some(plan.resolved.abi.return_layout);
        }
        if !self.is_go_callable(expression) {
            return None;
        }
        let callable = self.resolve_callable_value(expression)?;
        matches!(callable.origin, CallableOrigin::GoInterop).then(|| ValueLayout::Function {
            function_type: expression.get_type(),
            layout: callable.function_layout(&self.facts),
        })
    }

    fn try_plan_variadic_spread_slot_layout_change(
        &mut self,
        spread: &Expression,
        parameter: Option<&CallableParamAbi>,
    ) -> Option<ValuePlan> {
        let parameter = parameter?;
        parameter.variadic.as_ref()?;

        let raw_source = self.go_physical_expression_layout(spread);
        let source = raw_source.clone().unwrap_or_else(|| {
            self.facts
                .value_layout(&spread.get_type(), SlotOrigin::Lisette)
        });
        let target = ValueLayout::Slice {
            collection_type: spread.get_type(),
            element: Box::new(parameter.layout.clone()),
        };
        let coercion = CoercionPlan::change(self, &source, &target);
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

pub(in crate::lowering) fn converts_for_inference(
    facts: &EmitFacts<'_>,
    argument: &Expression,
    parameter: &CallableParamAbi,
    pinned: impl FnOnce(&str) -> bool,
) -> bool {
    let Some(Type::Parameter(name)) = &parameter.declared else {
        return false;
    };
    if pinned(name) {
        return false;
    }
    let checked = facts.checked(argument);
    let value_use = match argument {
        Expression::Cast { .. } => checked.cast_operand(),
        _ => checked.value_use(),
    };
    matches!(value_use.adaptation, Adaptation::Interface(_))
}
