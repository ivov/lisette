use crate::EmitFacts;
use crate::abi::ReturnSlot;
use crate::abi::is_closure_literal;
use crate::abi::layout::ValueLayout;
use crate::calls::split_native_receiver;
use crate::expressions::access::struct_call::plan_struct_literal;
use crate::ir::bodies::AsyncKind;
use crate::names::go_name;
use syntax::program::Adaptation;
use syntax::program::NativeOperation;

use crate::abi::bridge::GoResultBridge;
use crate::abi::callable::CallableReturnAbi;
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::FunctionLayout;
use crate::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, discard, effect_statement, expression_statement,
};
use crate::ir::go_expression::FunctionLiteralLayout;
use crate::ir::values::{CaptureBoundary, GoExpression, OperandForm, ValuePlan};
use crate::names::go_name::GeneratedPackage;
use crate::plan::calls::{CallPlan, CallableOrigin, FunctionBridge};
use crate::state::bindings::BindingValue;
use crate::state::expression::ExpressionContext;
use crate::statements::assignments::PlaceOrdering;
use crate::types::go_type::GoType;
use crate::{Planner, ReturnContext};
use syntax::ast::{Expression, IdentifierResolution, UnaryOperator};
use syntax::program::SlotOrigin;
use syntax::program::{CallOperation, DotAccessResolution};
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn plan_value(
        &mut self,
        expression: &Expression,
        context: ExpressionContext<'_>,
    ) -> ValuePlan {
        let expression = self.peel_go_slot_alias_cast(expression, context);
        if let Some(bridge) = self.function_value_bridge(expression, context) {
            return self
                .plan_operand(expression, context)
                .map_expression(|statements, value| {
                    self.plan_function_layout_bridge(
                        statements,
                        value,
                        &bridge.source,
                        &bridge.target,
                        false,
                    )
                });
        }
        self.plan_operand(expression, context)
    }

    pub(crate) fn function_value_bridge(
        &self,
        expression: &Expression,
        context: ExpressionContext<'_>,
    ) -> Option<FunctionBridge> {
        if context.is_callee() {
            return None;
        }
        let callee = self.resolve_callable_value(expression)?;
        let from_go = matches!(callee.origin, CallableOrigin::GoInterop);
        if from_go {
            if !self.is_go_callable(expression) {
                return None;
            }
        } else if context.forces_tagged_go_function()
            || is_closure_literal(expression)
            || self
                .go_slot_declaration_layout(expression, context)
                .is_some()
        {
            return None;
        }
        let origin = if from_go || callee.is_prelude_dispatch {
            context.function_slot_origin()
        } else {
            SlotOrigin::Lisette
        };
        let return_abi = if from_go {
            Some(self.go_fn_slot_abi(expression, context)?)
        } else {
            None
        };
        let source = callee.function_layout(self);
        self.callable_slot_bridge(source, &expression.get_type(), return_abi, origin)
    }

    pub(crate) fn plan_composite_value(
        &mut self,
        expression: &Expression,
        context: ExpressionContext<'_>,
    ) -> ValuePlan {
        if self.is_unit_call(expression)
            || (self.facts.resolves_to_unit(&expression.get_type())
                && matches!(expression, Expression::Block { .. }))
        {
            return self
                .plan_value(expression, context)
                .map_observable_expression(|setup, call| {
                    if !call.is_empty() {
                        setup.push(effect_statement(call));
                    }
                    GoExpression::unit()
                });
        }
        let ty = expression.get_type();
        let filtered_out = match expression {
            Expression::Function { .. } => true,
            Expression::Lambda { .. } => context
                .generic_result_slot()
                .and_then(Type::get_function_ret)
                .is_none_or(|return_type| self.facts.returns_go_void(return_type)),
            _ => false,
        };
        if !filtered_out && let Some((source, target)) = self.never_result_bridge(&ty, context) {
            return self
                .plan_value(expression, context)
                .map_expression(|statements, value| {
                    self.plan_function_layout_bridge(statements, value, &source, &target, true)
                });
        }
        self.plan_value(expression, context)
    }

    pub(crate) fn never_result_bridge(
        &self,
        function_ty: &Type,
        context: ExpressionContext<'_>,
    ) -> Option<(FunctionLayout, FunctionLayout)> {
        let slot = context.generic_result_slot()?;
        if !function_ty.get_function_ret().is_some_and(Type::is_never) {
            return None;
        }
        let return_type = slot
            .get_function_ret()
            .filter(|return_type| !self.facts.returns_go_void(return_type));
        let (source_result, target_result, return_abi) = match return_type {
            Some(return_type) => (
                self.value_layout(&Type::Never, SlotOrigin::Lisette),
                self.value_layout(return_type, SlotOrigin::Lisette),
                self.facts
                    .return_abi(return_type, ReturnSlot::FunctionExpression(context)),
            ),
            None => (
                self.value_layout(&Type::unit(), SlotOrigin::Lisette),
                self.value_layout(&Type::unit(), SlotOrigin::Lisette),
                CallableReturnAbi::Value,
            ),
        };
        let parameters = function_ty
            .get_function_params()
            .expect("a function has parameters")
            .iter()
            .map(|parameter| self.value_layout(&parameter.ty, SlotOrigin::Lisette))
            .collect::<Vec<_>>();
        let source = self.function_layout(
            parameters.clone(),
            &source_result,
            CallableReturnAbi::Void,
            None,
        );
        let target = self.function_layout(parameters, &target_result, return_abi, None);
        Some((source, target))
    }

    fn plan_call_value(
        &mut self,
        expression: &Expression,
        result_type: &Type,
        context: ExpressionContext<'_>,
    ) -> ValuePlan {
        let plan = self
            .plan_call(expression)
            .expect("plan_call yields Some for a Call expression");
        let layout_bridge = self.call_result_layout_bridge(&plan, result_type);
        let wraps_result = plan.wraps_result();

        if let Some(bridge) = layout_bridge {
            let call_type = (!wraps_result).then_some(result_type);
            let call = self.plan_call_with_plan(expression, call_type, context, plan);
            return call.map_observable_expression(|setup, call| {
                let (bridge_setup, value) = bridge.lower(self, call);
                setup.extend(bridge_setup);
                value
            });
        }

        if wraps_result {
            self.plan_go_abi_wrapped_call(
                expression,
                &plan.resolved.abi,
                plan.resolved.origin,
                result_type,
            )
        } else {
            self.plan_call_with_plan(expression, Some(result_type), context, plan)
        }
    }

    pub(crate) fn call_result_layout_bridge(
        &self,
        plan: &CallPlan<'_>,
        result_type: &Type,
    ) -> Option<CoercionPlan> {
        match self.go_result_bridge(&plan.resolved.abi, result_type)? {
            GoResultBridge::Whole(bridge) => Some(bridge),
            GoResultBridge::Tuple(_) | GoResultBridge::Payload { .. } => None,
        }
    }

    fn go_fn_slot_abi(
        &self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> Option<CallableReturnAbi> {
        let expression_ty = expression.get_type();
        let origin = self.function_type_origin(&expression_ty, ctx.function_slot_origin());
        let fn_ty = self.facts.resolve_to_function_type(&expression_ty)?;
        let f = fn_ty.as_function_type()?;
        Some(self.facts.return_abi(
            &f.return_type,
            ReturnSlot::FunctionExpression(ctx.with_function_slot_origin(origin)),
        ))
    }

    pub(crate) fn plan_operand_leaf(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        match expression {
            Expression::Literal { literal, ty, .. } => self.plan_literal(literal, ty),
            Expression::Identifier {
                value,
                ty,
                resolution,
                ..
            } => {
                let go_expression = self.plan_identifier(expression, value, resolution, ty, ctx);
                let stability = match self
                    .scope
                    .resolve_identifier_with_resolution(value, resolution)
                {
                    Some(BindingValue::InlineExpr(inline)) => inline.stability(),
                    _ => self.facts.identifier_read_stability(expression),
                };
                ValuePlan::from_identifier_expression(go_expression, stability)
            }
            Expression::Call { ty, .. } => self.plan_call_value(expression, ty, ctx),
            Expression::RawGo { text, .. } => ValuePlan::verbatim(text.clone()),
            Expression::Unit { .. } => ValuePlan::computed(Vec::new(), GoExpression::unit()),
            Expression::Lambda {
                params, body, ty, ..
            } => ValuePlan::computed(Vec::new(), self.plan_lambda(params, body, ty, ctx)),
            Expression::Function {
                params, body, ty, ..
            } => match body.definition() {
                Some(body) => {
                    ValuePlan::computed(Vec::new(), self.plan_lambda(params, body, ty, ctx))
                }
                None => ValuePlan::computed(Vec::new(), GoExpression::empty()),
            },
            Expression::If { ty, .. }
            | Expression::IfLet { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. }
            | Expression::Block { ty, .. } => self.plan_to_operand_temp(expression, ty),
            Expression::Return {
                expression: return_expression,
                ..
            } => {
                let plan = self.build_return_plan(return_expression);
                ValuePlan::computed(plan.statements, GoExpression::empty())
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                let setup = vec![
                    self.build_assignment_plan(target, value, compound_operator.as_ref())
                        .into(),
                ];
                ValuePlan::computed(setup, GoExpression::unit())
            }
            Expression::Assert { .. } => ValuePlan::computed(
                vec![self.lower_assert_statement(expression).into()],
                GoExpression::unit(),
            ),
            _ => unreachable!(
                "unexpected leaf expression in plan_operand: {:?}",
                expression
            ),
        }
    }

    pub(crate) fn coerce_elements_to_slots(
        &mut self,
        setup: &mut Vec<Statement>,
        tuple: &Expression,
        values: Vec<GoExpression>,
        slot_types: &[Type],
        lowered_return: bool,
    ) -> Vec<GoExpression> {
        let Expression::Tuple { elements, ty, .. } = tuple else {
            unreachable!("only a tuple literal has elements to coerce")
        };
        let tuple_use = self.facts.checked(tuple).value_use();
        let placed = match &tuple_use.adaptation {
            Adaptation::Tuple(adaptations)
                if !self
                    .facts
                    .same_types(&self.facts.tuple_element_types(ty), slot_types) =>
            {
                Some((adaptations, self.facts.tuple_element_types(&tuple_use.slot)))
            }
            _ => None,
        };
        if placed.is_some() {
            self.record_converted_value(tuple);
        }
        let slot_layouts = if lowered_return {
            self.lowered_tuple_slot_layouts(&Type::Tuple(slot_types.to_vec()))
        } else {
            Vec::new()
        };
        let mut coerced_elements = Vec::with_capacity(values.len());
        for (index, (element, value)) in elements.iter().zip(values).enumerate() {
            let Some(slot) = slot_types.get(index).filter(|_| {
                !matches!(
                    slot_layouts.get(index),
                    Some(ValueLayout::NullableOption { .. })
                )
            }) else {
                coerced_elements.push(value);
                continue;
            };
            let coercion = match placed
                .as_ref()
                .and_then(|(adaptations, slots)| adaptations.get(index).zip(slots.get(index)))
            {
                Some((adaptation, semantic)) => {
                    self.adapted_slot_coercion(element, adaptation, semantic, slot)
                }
                None => self.value_slot_coercion(element, slot),
            };
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced_elements.push(coerced);
        }
        coerced_elements
    }

    pub(crate) fn make_tuple_callee(&mut self, slot_types: &[Type], arity: usize) -> GoExpression {
        let callee = GoExpression::generated(
            GeneratedPackage::Prelude,
            go_name::make_tuple_constructor(arity),
        );
        if slot_types.len() != arity {
            return callee;
        }
        let slots: Vec<GoType> = slot_types.iter().map(|slot| self.go_type(slot)).collect();
        GoExpression::instantiation(callee, GoType::type_arguments(&slots))
    }

    pub(crate) fn tail_tuple_return_slots(&self, tuple: &Expression) -> Option<Vec<Type>> {
        let Expression::Tuple {
            ty: Type::Tuple(inferred),
            ..
        } = tuple
        else {
            return None;
        };
        let slots = self.facts.tuple_elements(self.return_ctx().ty()?)?;
        (slots.len() == inferred.len()).then_some(slots)
    }

    pub(crate) fn plan_tuple_value(&mut self, tuple: &Expression, in_tail: bool) -> ValuePlan {
        let Expression::Tuple { elements, ty, .. } = tuple else {
            unreachable!("plan_tuple_value lowers a tuple literal")
        };
        let inferred_slot_types: Vec<Type> = match ty {
            Type::Tuple(slots) => slots.clone(),
            _ => Vec::new(),
        };
        let slot_types = match in_tail.then(|| self.tail_tuple_return_slots(tuple)) {
            Some(Some(return_slots)) => return_slots,
            _ => inferred_slot_types,
        };

        let stages: Vec<ValuePlan> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let slot = slot_types.get(i);
                let origin = slot.map_or(SlotOrigin::Lisette, |ty| {
                    self.function_type_origin(ty, SlotOrigin::Lisette)
                });
                let element_ctx = ExpressionContext::value()
                    .with_expected_slot_type(slot)
                    .with_function_slot_origin(origin);
                self.plan_composite_value(e, element_ctx)
            })
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "v");
        let mut setup = sequenced.setup;

        let element_expressions =
            self.coerce_elements_to_slots(&mut setup, tuple, sequenced.values, &slot_types, false);
        let callee = self.make_tuple_callee(&slot_types, element_expressions.len());
        ValuePlan::computed(setup, GoExpression::pure_call(callee, element_expressions))
    }

    pub(crate) fn plan_cast(
        &mut self,
        cast: &Expression,
        expression: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let slot_origin = self.function_type_origin(ty, SlotOrigin::Lisette);
        let ctx = ctx.with_function_slot_origin(slot_origin);

        if let Some(coercion) = self.cast_operand_coercion(cast, &expression.get_type(), ty) {
            let inner = self.plan_value(expression, ctx);
            let mut converted = inner.map_expression(|setup, value| {
                let (coercion_setup, coerced) = coercion.lower(self, value);
                setup.extend(coercion_setup);
                coerced
            });
            if !converted.stability().is_stable_across_calls() {
                converted.make_observable();
            }
            return converted;
        }

        let inner = self.plan_value(expression, ctx);
        let go_type = self.go_type(ty);

        let function_bridge = self.function_slot_bridge(expression, ty);
        if !function_bridge.is_identity() {
            return inner
                .map_expression(|setup, value| {
                    let (bridge_setup, bridged) = function_bridge.lower(self, value);
                    setup.extend(bridge_setup);
                    bridged
                })
                .conversion(go_type);
        }

        if let Some(source_go_type) = self.shift_pin_go_type(expression, ty) {
            return inner.conversion(source_go_type).conversion(go_type);
        }

        if self.is_identity_conversion(expression, &inner, ty) {
            return inner;
        }

        inner.conversion(go_type)
    }

    fn is_identity_conversion(
        &self,
        expression: &Expression,
        inner: &ValuePlan,
        ty: &Type,
    ) -> bool {
        if inner.expression().constant_kind().is_some() {
            return false;
        }
        let source_ty = expression.get_type();
        if self.facts.resolve_to_function_type(&source_ty).is_some()
            || self.facts.resolve_to_function_type(ty).is_some()
        {
            return false;
        }
        self.target_go_type(&source_ty) == self.target_go_type(ty)
    }

    fn shift_pin_go_type(&mut self, expression: &Expression, target_ty: &Type) -> Option<GoType> {
        let target_is_float = self
            .facts
            .underlying_simple_kind(target_ty)
            .is_some_and(|kind| kind.is_float());
        if !target_is_float || !self.contains_untyped_constant_shift(expression) {
            return None;
        }
        let source_ty = expression.get_type();
        self.facts
            .underlying_simple_kind(&source_ty)
            .is_some_and(|kind| kind.integer_range().is_some())
            .then(|| self.go_type(&source_ty))
    }

    pub(crate) fn plan_reference(&mut self, inner: &Expression) -> ValuePlan {
        if self.is_unit_call(inner) {
            let staged = self.plan_operand(inner, ExpressionContext::value());
            return staged.map_observable_expression(|setup, staged_value| {
                if !staged_value.is_empty() {
                    setup.push(effect_statement(staged_value));
                }
                let tmp = self.hoist_tmp_value_statement(setup, "ref", GoExpression::unit());
                GoExpression::address_of(GoExpression::identifier(tmp))
            });
        }

        let unaddressable = self.is_go_unaddressable(inner);
        let inner_plan = if self
            .facts
            .resolve_to_function_type(&inner.get_type())
            .is_some()
            && (!unaddressable
                || matches!(inner, Expression::DotAccess { resolution, .. } if resolution.is_field_read()))
        {
            if matches!(inner, Expression::Identifier { .. }) {
                self.plan_operand(inner, ExpressionContext::value())
            } else {
                let mut setup = Vec::new();
                let place = self.plan_place(&mut setup, inner, PlaceOrdering::alone());
                ValuePlan::computed(setup, place)
            }
        } else {
            self.plan_value(inner, ExpressionContext::value())
        };
        inner_plan.map_observable_expression(|setup, emitted| {
            if unaddressable {
                let tmp = self.hoist_tmp_value_statement(setup, "ref", emitted);
                GoExpression::address_of(GoExpression::identifier(tmp))
            } else {
                GoExpression::address_of(emitted)
            }
        })
    }

    pub(crate) fn plan_range_value(
        &mut self,
        start: &Option<Box<Expression>>,
        end: &Option<Box<Expression>>,
        ty: &Type,
    ) -> ValuePlan {
        let type_string = self.go_type(ty);

        let mut stages: Vec<ValuePlan> = Vec::new();
        let has_start = start.is_some();
        if let Some(s) = start {
            stages.push(self.plan_operand(s, ExpressionContext::value()));
        }
        if let Some(e) = end {
            stages.push(self.plan_operand(e, ExpressionContext::value()));
        }

        if stages.is_empty() {
            return ValuePlan::computed(Vec::new(), GoExpression::unit());
        }

        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "range");
        let mut values = sequenced.values.into_iter();
        let mut fields = Vec::new();
        if has_start {
            fields.push((
                go_name::RANGE_START_FIELD.to_string(),
                values.next().expect("range has a start"),
            ));
            if let Some(end_value) = values.next() {
                fields.push((go_name::RANGE_END_FIELD.to_string(), end_value));
            }
        } else {
            fields.push((
                go_name::RANGE_END_FIELD.to_string(),
                values.next().expect("range has an end"),
            ));
        }

        let bounds: Vec<_> = start.iter().chain(end).collect();
        let value = plan_struct_literal(&type_string, fields).position_elements(|index, _| {
            bounds
                .get(index)
                .and_then(|bound| self.facts.source_position(&bound.get_span()))
        });
        ValuePlan::computed(sequenced.setup, value)
    }

    pub(crate) fn plan_async_wrapper(
        &mut self,
        kind: AsyncKind,
        expression: &Expression,
    ) -> ValuePlan {
        let async_statement = |call: GoExpression| LoweredStatement::Async { kind, call };
        let statement_plan =
            |setup: Vec<Statement>| ValuePlan::computed(setup, GoExpression::empty());
        let immediate_call = |body: LoweredBlock| {
            GoExpression::immediate_call(GoType::default(), body, FunctionLiteralLayout::MultiLine)
        };

        if let Expression::Block { .. } = expression {
            let body = self.with_isolated_function(ReturnContext::Void(Type::unit()), |planner| {
                planner.lower_block_as_body(expression)
            });
            return statement_plan(vec![async_statement(immediate_call(body)).into()]);
        }

        let mut setup: Vec<Statement> = Vec::new();
        if let Some(call) = self.try_plan_go_call_discarded(&mut setup, expression) {
            setup.push(async_statement(call).into());
            return statement_plan(setup);
        }

        let checkpoint = self.planning_snapshot();
        let plan = self.plan_value(
            expression,
            ExpressionContext::value().with_capture_boundary(CaptureBoundary::DirectDelayedCall),
        );
        if needs_iife_for_async(
            expression,
            self.facts.call_operation(expression),
            plan.expression(),
        ) {
            self.restore_planning(checkpoint);
            let (mut setup, inner) = self
                .plan_value(
                    expression,
                    ExpressionContext::value().with_capture_boundary(CaptureBoundary::Delayed),
                )
                .into_parts();
            let mut body_statements = Vec::new();
            if !inner.is_empty() {
                body_statements.push(if self.facts.resolves_to_unit(&expression.get_type()) {
                    expression_statement(inner)
                } else {
                    discard(inner)
                });
            }
            let body = LoweredBlock {
                statements: body_statements,
            };
            setup.push(async_statement(immediate_call(body)).into());
            return statement_plan(setup);
        }
        let (mut setup, inner) = plan.into_parts();
        setup.push(async_statement(inner).into());
        statement_plan(setup)
    }

    fn is_go_unaddressable(&self, expression: &Expression) -> bool {
        if self
            .facts
            .resolve_to_function_type(&expression.get_type())
            .is_some()
        {
            return match expression {
                Expression::Identifier {
                    resolution: IdentifierResolution::Definition { name, .. },
                    ..
                } => !self
                    .facts
                    .definition(name)
                    .is_some_and(|definition| definition.is_variable()),
                Expression::Identifier {
                    value, resolution, ..
                } => !matches!(
                    self.scope
                        .resolve_identifier_with_resolution(value, resolution),
                    Some(BindingValue::GoName(_))
                ),
                Expression::DotAccess {
                    resolution: DotAccessResolution::PackageMember { definition, .. },
                    ..
                } => !definition
                    .as_ref()
                    .and_then(|name| self.facts.definition(name))
                    .is_some_and(|definition| definition.is_variable()),
                Expression::DotAccess { resolution, .. } if resolution.is_field_read() => {
                    !self.receiver_is_addressable(expression)
                }
                Expression::IndexedAccess { .. }
                | Expression::Unary {
                    operator: UnaryOperator::Deref,
                    ..
                } => !self.receiver_is_addressable(expression),
                _ => true,
            };
        }
        match expression {
            Expression::Call { .. } => true,
            Expression::Identifier {
                value,
                ty,
                resolution,
                ..
            } if !matches!(ty.unwrap_forall(), Type::Function(_)) => {
                self.identifier_is_unaddressable(value, resolution, ty)
            }
            Expression::DotAccess { ty, resolution, .. }
                if !matches!(ty.unwrap_forall(), Type::Function(_)) =>
            {
                matches!(resolution, DotAccessResolution::EnumVariant { .. })
            }
            _ => false,
        }
    }

    fn identifier_is_unaddressable(
        &self,
        value: &str,
        resolution: &IdentifierResolution,
        ty: &Type,
    ) -> bool {
        match self
            .scope
            .resolve_identifier_with_resolution(value, resolution)
        {
            Some(binding) => binding.as_go_name().is_none(),
            None => self.facts.is_lisette_enum(ty),
        }
    }
}

fn needs_iife_for_async(
    expression: &Expression,
    operation: Option<&CallOperation>,
    value: &GoExpression,
) -> bool {
    let (
        Expression::Call {
            expression: callee,
            args,
            ..
        },
        Some(CallOperation::Native { kind, method }),
    ) = (expression, operation)
    else {
        return false;
    };
    let arity = split_native_receiver(callee, args).map_or(0, |(_, arguments)| arguments.len());
    !matches!(value.syntax_form(), OperandForm::Call)
        || !NativeOperation::new(*kind, method).lowers_to_plain_call(arity)
}

impl EmitFacts<'_> {
    pub(crate) fn contains_newtype_access(&self, expression: &Expression) -> bool {
        let mut current = expression;
        while let Expression::DotAccess {
            expression: inner,
            resolution,
            ..
        } = current
        {
            if matches!(
                resolution,
                DotAccessResolution::TupleStructField { is_newtype: true }
            ) {
                return true;
            }
            current = inner;
        }
        false
    }
}
