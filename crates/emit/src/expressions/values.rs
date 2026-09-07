use crate::abi::is_tagged_shape_fn_value;
use crate::expressions::access::struct_call::emit_struct_literal;
use syntax::program::DefinitionBody;

use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::transition::emit_lisette_callback_wrapper;
use crate::context::expression::ExpressionContext;
use crate::is_order_sensitive;
use crate::plan::bodies::{LoweredBlock, LoweredStatement, assign, discard, expression_statement};
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::plan::go_expression::FunctionLiteralLayout;
use crate::plan::values::{
    CaptureBoundary, EvaluationEffect, GoExpression, OperandForm, ValuePlan,
};
use crate::state::bindings::BindingValue;
use syntax::ast::Expression;
use syntax::program::CallKind;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn lower_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if self.is_go_callable(expression)
            && let Some(callee) = self.resolve_callable_value(expression)
            && matches!(callee.origin, CallableOrigin::GoInterop)
        {
            let abi = &callee.abi.result;
            let matches_slot = self.go_fn_slot_abi(expression, ctx).as_ref() == Some(abi);
            let target_layout =
                self.value_layout(&expression.get_type(), ctx.function_slot_origin());
            let same_abi = matches_slot
                && matches!(
                    &target_layout,
                    ValueLayout::Function { layout, .. } if layout.return_abi == *abi
                );
            if !ctx.is_callee() && same_abi {
                let source_layout = ValueLayout::Function {
                    function_type: expression.get_type(),
                    layout: callee.abi.function_layout(),
                };
                let layout_coercion = CoercionPlan::bridge(self, &source_layout, &target_layout);
                if !layout_coercion.is_identity() {
                    let value = self.plan_operand(expression, ctx);
                    return value.map_expression_as_computed(|setup, value| {
                        let (bridge_setup, value) = layout_coercion.lower(self, value);
                        setup.extend(bridge_setup);
                        value.with_deferred_evaluation(true)
                    });
                }
            }
            if !matches_slot {
                let mut setup = Vec::new();
                let value = if self.go_fn_needs_lowered_tuple_adapter(expression, abi, ctx) {
                    self.emit_go_fn_lowered_tuple_adapter(&mut setup, expression)
                } else if let CallableReturnAbi::Option(OptionReturnAbi::Sentinel(value)) = abi
                    && !ctx.forces_tagged_go_function()
                {
                    self.emit_go_fn_sentinel_adapter(&mut setup, expression, *value)
                } else {
                    self.emit_go_fn_wrapper(&mut setup, expression, &callee.abi)
                };
                return ValuePlan::computed(setup, value, EvaluationEffect::Pure)
                    .stable_across_calls_if(
                        self.identifier_immune_to_calls(expression.unwrap_parens()),
                    );
            }
        }

        self.plan_operand(expression, ctx)
    }

    pub(crate) fn lower_composite_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if expression.get_type().is_unit()
            && matches!(
                expression.unwrap_parens(),
                Expression::Call { .. } | Expression::Block { .. }
            )
        {
            return self
                .lower_value(expression, ctx)
                .map_expression_as_observable_computed(|setup, call| {
                    if !call.is_empty() {
                        setup.push(expression_statement(call));
                    }
                    GoExpression::empty_composite("struct{}".to_string())
                });
        }
        self.lower_value(expression, ctx)
    }

    pub(crate) fn lower_call_value(
        &mut self,
        expression: &Expression,
        result_type: &Type,
        context: ExpressionContext<'_>,
    ) -> ValuePlan {
        let plan = self
            .plan_call(expression)
            .expect("plan_call yields Some for a Call expression");
        let layout_bridge = self.call_result_layout_bridge(&plan, result_type);
        let result_transition = plan.result_transition;

        if let Some(bridge) = layout_bridge {
            let call_type =
                matches!(result_transition, AbiTransition::Identity).then_some(result_type);
            let call = self.lower_call_with_plan(expression, call_type, context, plan);
            return call.map_expression_as_observable_computed(|setup, call| {
                let (bridge_setup, value) = bridge.lower(self, call);
                setup.extend(bridge_setup);
                value.with_deferred_evaluation(false)
            });
        }

        match result_transition {
            AbiTransition::Identity => {
                self.lower_call_with_plan(expression, Some(result_type), context, plan)
            }
            AbiTransition::WrapToTagged => {
                self.lower_go_abi_wrapped_call(expression, &plan.resolved.abi, result_type)
            }
            AbiTransition::LowerFromTagged
            | AbiTransition::Reencode
            | AbiTransition::Incompatible => {
                unreachable!("call results target their Lisette value representation")
            }
        }
    }

    pub(crate) fn call_result_layout_bridge(
        &self,
        plan: &CallPlan<'_>,
        result_type: &Type,
    ) -> Option<CoercionPlan> {
        if !matches!(plan.resolved.origin, CallableOrigin::GoInterop) {
            return None;
        }
        self.go_result_layout_bridge(&plan.resolved.abi, result_type)
    }

    /// Wrap a captured tagged-shape prelude fn ref into a lowered-ABI closure
    /// so its Go type matches what the rest of the pipeline expects.
    fn maybe_lower_tagged_fn_ref(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        ty: &Type,
        raw: GoExpression,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        if ctx.is_callee() || ctx.forces_tagged_go_function() {
            return raw;
        }
        if !is_tagged_shape_fn_value(expression) {
            return raw;
        }
        let fn_ty = ty.unwrap_forall();
        let Type::Function(f) = fn_ty else {
            return raw;
        };
        if self.classify_direct_emission(&f.return_type).is_none() {
            return raw;
        }
        emit_lisette_callback_wrapper(self, setup, raw, fn_ty)
    }

    /// Result ABI the slot expects from a Go function value, or `None` when
    /// the value is not a function. A forced-tagged slot calls through a
    /// Lisette-shaped callback, so it wants the tagged ABI, not the lowered one.
    fn go_fn_slot_abi(
        &self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> Option<CallableReturnAbi> {
        let fn_ty = expression.get_type();
        let f = fn_ty.as_function_type()?;
        Some(if ctx.forces_tagged_go_function() {
            self.value_return_abi(&f.return_type)
        } else {
            self.slot_return_abi(&f.return_type, ctx.function_slot_origin())
        })
    }

    /// True when a tuple-ok fallible Go function value must be wrapped to match a lowered slot.
    fn go_fn_needs_lowered_tuple_adapter(
        &self,
        expression: &Expression,
        source: &CallableReturnAbi,
        ctx: ExpressionContext<'_>,
    ) -> bool {
        matches!(
            (source, self.go_fn_slot_abi(expression, ctx)),
            (
                CallableReturnAbi::Result {
                    payload: PayloadLayout::Flattened,
                } | CallableReturnAbi::Partial {
                    payload: PayloadLayout::Flattened,
                },
                Some(
                    CallableReturnAbi::Result {
                        payload: PayloadLayout::Packed,
                    } | CallableReturnAbi::Partial {
                        payload: PayloadLayout::Packed,
                    }
                )
            )
        )
    }

    /// Plan a value-position leaf expression (one `plan_operand` does not lower
    /// structurally) into a `ValuePlan`.
    pub(crate) fn plan_operand_leaf(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        match expression {
            Expression::Literal { literal, ty, .. } => self.emit_literal(literal, ty),
            Expression::Identifier {
                value,
                ty,
                resolution,
                ..
            } => {
                let go_expression = self.emit_identifier(value, resolution.definition(), ty, ctx);
                let stability = self.identifier_read_stability(expression);
                let plan = ValuePlan::from_identifier_expression(go_expression, stability);
                let mut adapter_setup = Vec::new();
                let value = self.maybe_lower_tagged_fn_ref(
                    &mut adapter_setup,
                    expression,
                    ty,
                    plan.expression.clone(),
                    ctx,
                );
                if adapter_setup.is_empty() {
                    plan
                } else {
                    plan.map_expression_as_computed(|setup, identifier| {
                        setup.extend(adapter_setup);
                        value.with_deferred_evaluation(identifier.contains_deferred_evaluation())
                    })
                }
            }
            Expression::Call { ty, .. } => self.lower_call_value(expression, ty, ctx),
            Expression::RawGo { text } => ValuePlan::verbatim(text.clone()),
            Expression::Unit { .. } => ValuePlan::computed(
                Vec::new(),
                GoExpression::empty_composite("struct{}".to_string()),
                EvaluationEffect::Pure,
            ),
            Expression::Lambda {
                params, body, ty, ..
            } => ValuePlan::computed(
                Vec::new(),
                self.emit_lambda(params, body, ty, ctx),
                EvaluationEffect::Pure,
            ),
            Expression::Function {
                params, body, ty, ..
            } => match body.definition() {
                Some(body) => ValuePlan::computed(
                    Vec::new(),
                    self.emit_lambda(params, body, ty, ctx),
                    EvaluationEffect::Pure,
                ),
                None => {
                    ValuePlan::computed(Vec::new(), GoExpression::empty(), EvaluationEffect::Pure)
                }
            },
            Expression::IfLet { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. }
            | Expression::Block { ty, .. } => self.lower_to_operand_temp(expression, ty),
            Expression::Return {
                expression: return_expression,
                ..
            } => {
                let plan = self.build_return_plan(return_expression);
                ValuePlan::computed(
                    vec![LoweredStatement::Return(plan)],
                    GoExpression::empty(),
                    EvaluationEffect::Pure,
                )
            }
            Expression::Assignment { target, value, .. } => {
                let setup = self.lower_assignment_operand(target, value);
                ValuePlan::computed(
                    setup,
                    GoExpression::empty_composite("struct{}".to_string())
                        .with_deferred_evaluation(false),
                    EvaluationEffect::Pure,
                )
            }
            Expression::Assert { .. } => ValuePlan::computed(
                vec![self.lower_assert_statement(expression)],
                GoExpression::empty_composite("struct{}".to_string()),
                EvaluationEffect::Pure,
            ),
            _ => unreachable!(
                "unexpected leaf expression in plan_operand: {:?}",
                expression
            ),
        }
    }

    pub(crate) fn coerce_elements_to_slots(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        elements: &[Expression],
        values: Vec<GoExpression>,
        slot_types: &[Type],
    ) -> Vec<GoExpression> {
        let mut coerced_elements = Vec::with_capacity(values.len());
        for (index, (element, value)) in elements.iter().zip(values).enumerate() {
            let Some(slot) = slot_types.get(index) else {
                coerced_elements.push(value);
                continue;
            };
            let coercion = CoercionPlan::internal(self, &element.get_type(), slot);
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            coerced_elements.push(coerced);
        }
        coerced_elements
    }

    pub(crate) fn make_tuple_callee(&mut self, slot_types: &[Type], arity: usize) -> GoExpression {
        self.require_stdlib();
        let callee = GoExpression::name(format!("lisette.MakeTuple{}", arity));
        if slot_types.len() != arity {
            return callee;
        }
        let rendered: Vec<String> = slot_types
            .iter()
            .map(|slot| self.use_go_type(slot))
            .collect();
        GoExpression::instantiation(callee, format!("[{}]", rendered.join(", ")))
    }

    pub(crate) fn plan_tuple_value(
        &mut self,
        elements: &[Expression],
        ty: &Type,
        in_tail: bool,
    ) -> ValuePlan {
        let inferred_slot_types: Vec<Type> = match ty {
            Type::Tuple(slots) => slots.clone(),
            _ => Vec::new(),
        };
        let slot_types = match in_tail.then(|| tail_return_slots(self, &inferred_slot_types)) {
            Some(Some(return_slots)) => return_slots,
            _ => inferred_slot_types,
        };

        let stages: Vec<ValuePlan> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let element_ctx =
                    ExpressionContext::value().with_expected_slot_type(slot_types.get(i));
                self.lower_composite_value(e, element_ctx)
            })
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "v");
        let effect = sequenced.effect;
        let mut setup = sequenced.setup;

        let element_expressions =
            self.coerce_elements_to_slots(&mut setup, elements, sequenced.values, &slot_types);
        let callee = self.make_tuple_callee(&slot_types, element_expressions.len());
        ValuePlan::observable_call(
            setup,
            GoExpression::call(callee, element_expressions),
            effect,
        )
    }

    /// Plan a `cast` expression. The interface-target path resolves through
    /// a coercion (may emit setup); the primitive/named path becomes a
    /// structured `ValuePlan::Cast { go_type, inner }`. The inner value is
    /// planned first to preserve the original mutation order (inner emitted
    /// before the target type is formatted).
    pub(crate) fn plan_cast(
        &mut self,
        expression: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let slot_origin = self.function_type_origin(ty, SlotOrigin::Lisette);
        let ctx = ctx.with_function_slot_origin(slot_origin);

        if self.facts.is_interface(ty) {
            let inner = self.lower_value(expression, ctx);
            let source_ty = expression.get_type();
            let coercion = CoercionPlan::internal(self, &source_ty, ty);
            let mut converted = inner.map_expression_as_computed(|setup, value| {
                let (coercion_setup, coerced) = coercion.lower(self, value);
                setup.extend(coercion_setup);
                coerced.with_deferred_evaluation(true)
            });
            if !converted.evaluation.stability.is_stable_across_calls() {
                converted.make_observable();
            }
            return converted;
        }

        let expression = expression.unwrap_parens();
        let inner = self.lower_value(expression, ctx);
        let go_type = self.use_go_type(ty);

        let function_bridge = self.function_slot_bridge(expression, ty);
        if !function_bridge.is_identity() {
            return inner
                .map_expression_as_computed(|setup, value| {
                    let (bridge_setup, bridged) = function_bridge.lower(self, value);
                    setup.extend(bridge_setup);
                    bridged.with_deferred_evaluation(true)
                })
                .conversion(go_type);
        }

        if let Some(source_go_type) = self.shift_pin_go_type(expression, ty) {
            return inner.conversion(source_go_type).conversion(go_type);
        }

        inner.conversion(go_type)
    }

    /// `T(x)` is a primary expression, so parentheses around it add nothing.
    pub(crate) fn is_conversion_cast(&self, expression: &Expression) -> bool {
        matches!(
            expression.unwrap_parens(),
            Expression::Cast { ty, .. } if !self.facts.is_interface(ty)
        )
    }

    fn shift_pin_go_type(&mut self, expression: &Expression, target_ty: &Type) -> Option<String> {
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
            .then(|| self.use_go_type(&source_ty))
    }

    /// Plan a `&inner` reference, hoisting to a temp when the inner is
    /// Go-unaddressable.
    pub(crate) fn plan_reference(&mut self, inner: &Expression, ty: &Type) -> ValuePlan {
        if inner.get_type().is_unit() && matches!(inner.unwrap_parens(), Expression::Call { .. }) {
            let staged = self.plan_operand(inner.unwrap_parens(), ExpressionContext::value());
            return staged.map_expression_as_observable_computed(|setup, staged_value| {
                if !staged_value.is_empty() {
                    setup.push(expression_statement(staged_value));
                }
                let tmp = self.hoist_tmp_value_statement(
                    setup,
                    "ref",
                    GoExpression::empty_composite("struct{}".to_string()),
                );
                GoExpression::address_of(GoExpression::name(tmp))
            });
        }

        let inner_plan = self.lower_value(inner, ExpressionContext::value());
        inner_plan.map_expression_as_observable_computed(|setup, emitted| {
            if inner.get_type() == *ty {
                emitted
            } else if self.is_go_unaddressable(inner)
                || matches!(inner.get_type(), Type::Function(_))
            {
                let tmp = self.hoist_tmp_value_statement(setup, "ref", emitted);
                GoExpression::address_of(GoExpression::name(tmp))
            } else {
                GoExpression::address_of(emitted)
            }
        })
    }

    pub(crate) fn contains_newtype_access(&self, expression: &Expression) -> bool {
        let mut current = expression;
        while let Expression::DotAccess {
            expression: inner,
            member,
            ..
        } = current
        {
            if member.parse::<usize>().is_ok()
                && self.is_newtype_struct(&inner.get_type().strip_refs())
            {
                return true;
            }
            current = inner;
        }
        false
    }

    fn lower_assignment_operand(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Vec<LoweredStatement> {
        let go_field_layout = match target {
            Expression::DotAccess {
                expression: receiver,
                member,
                ty,
                resolution,
                ..
            } => self.field_slot_layout(
                &receiver.get_type(),
                resolution.declaring_type(),
                member,
                ty,
            ),
            _ => None,
        };
        let literal_slot = go_field_layout
            .as_ref()
            .and_then(|layout| self.lower_option_literal_into_layout(value, layout));
        let is_literal_slot = literal_slot.is_some();
        let right_hand_side = literal_slot
            .unwrap_or_else(|| self.lower_composite_value(value, ExpressionContext::value()));
        let mut setup: Vec<LoweredStatement> = Vec::new();
        let target_place = if is_order_sensitive(target) {
            self.emit_left_value_capturing(&mut setup, target, Some(&right_hand_side))
        } else {
            self.emit_left_value(&mut setup, target)
        };
        let ValuePlan {
            setup: rhs_setup,
            expression: rhs_value,
            ..
        } = right_hand_side;
        setup.extend(rhs_setup);

        if let Some(target_layout) = go_field_layout
            && !is_literal_slot
        {
            let source_layout = self.value_layout(&value.get_type(), SlotOrigin::Lisette);
            let coercion = CoercionPlan::bridge(self, &source_layout, &target_layout);
            if !coercion.is_identity() {
                let (coercion_setup, unwrapped) = coercion.lower(self, rhs_value);
                setup.extend(coercion_setup);
                setup.push(assign(target_place, unwrapped));
                return setup;
            }
        }
        setup.push(assign(target_place, rhs_value));
        setup
    }

    pub(crate) fn plan_range_value(
        &mut self,
        start: &Option<Box<Expression>>,
        end: &Option<Box<Expression>>,
        _inclusive: bool,
        ty: &Type,
    ) -> ValuePlan {
        let type_string = self.use_go_type(ty);

        let mut stages: Vec<ValuePlan> = Vec::new();
        let has_start = start.is_some();
        if let Some(s) = start {
            stages.push(self.plan_operand(s, ExpressionContext::value()));
        }
        if let Some(e) = end {
            stages.push(self.plan_operand(e, ExpressionContext::value()));
        }

        if stages.is_empty() {
            return ValuePlan::computed(
                Vec::new(),
                GoExpression::empty_composite("struct{}".to_string()),
                EvaluationEffect::Pure,
            );
        }

        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "range");
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let mut values = sequenced.values.into_iter();
        let mut fields = Vec::new();
        if has_start {
            fields.push((
                "Start".to_string(),
                values.next().expect("range has a start"),
            ));
            if let Some(end_value) = values.next() {
                fields.push(("End".to_string(), end_value));
            }
        } else {
            fields.push(("End".to_string(), values.next().expect("range has an end")));
        }

        let value = emit_struct_literal(
            &type_string,
            fields,
            ExpressionContext::value(),
            contains_deferred_evaluation,
        );
        ValuePlan::computed(sequenced.setup, value, effect)
    }

    /// Plan a `Task`/`Defer` operand.
    pub(crate) fn plan_async_wrapper(
        &mut self,
        keyword: &str,
        expression: &Expression,
    ) -> ValuePlan {
        let async_statement = |call: GoExpression| LoweredStatement::Async {
            keyword: keyword.to_string(),
            call,
        };
        let statement_plan = |setup: Vec<LoweredStatement>| {
            ValuePlan::computed(setup, GoExpression::empty(), EvaluationEffect::Pure)
        };
        let immediate_call = |body: LoweredBlock| {
            GoExpression::immediate_call(String::new(), body, FunctionLiteralLayout::MultiLine)
        };

        if let Expression::Block { .. } = expression {
            let body =
                self.with_isolated_function(|planner| planner.lower_block_as_body(expression));
            return statement_plan(vec![async_statement(immediate_call(body))]);
        }

        let mut setup: Vec<LoweredStatement> = Vec::new();
        if let Some(call) = self.emit_go_call_discarded(&mut setup, expression) {
            setup.push(async_statement(call));
            return statement_plan(setup);
        }

        let plan = self.lower_value(
            expression,
            ExpressionContext::value().with_capture_boundary(CaptureBoundary::DirectDelayedCall),
        );
        if needs_iife_for_async(expression, plan.evaluation.form) {
            let capture_boundary = if keyword == "defer" {
                CaptureBoundary::DeferSite
            } else {
                CaptureBoundary::TaskSite
            };
            let (mut setup, inner) = self
                .lower_value(
                    expression,
                    ExpressionContext::value().with_capture_boundary(capture_boundary),
                )
                .into_parts();
            let mut body_statements = Vec::new();
            if !inner.is_empty() {
                body_statements.push(if expression.get_type().is_unit() {
                    expression_statement(inner)
                } else {
                    discard(inner)
                });
            }
            let body = LoweredBlock {
                statements: body_statements,
            };
            setup.push(async_statement(immediate_call(body)));
            return statement_plan(setup);
        }
        let (mut setup, inner) = plan.into_parts();
        setup.push(async_statement(inner));
        statement_plan(setup)
    }
}

impl Planner<'_> {
    fn is_go_unaddressable(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Call { .. } => true,
            Expression::Identifier { value, ty, .. }
                if !matches!(ty.unwrap_forall(), Type::Function(_)) =>
            {
                self.identifier_is_unaddressable(value, ty)
            }
            Expression::DotAccess { expression, ty, .. }
                if !matches!(ty.unwrap_forall(), Type::Function(_)) =>
            {
                self.dot_access_is_unaddressable(expression, ty)
            }
            _ => false,
        }
    }

    fn identifier_is_unaddressable(&self, value: &str, ty: &Type) -> bool {
        match self.scope.resolve_identifier_binding(value) {
            Some(BindingValue::GoName(_) | BindingValue::GoConst(_)) => false,
            Some(BindingValue::InlineExpr(_)) => true,
            None => self.ty_is_enum(ty),
        }
    }

    fn dot_access_is_unaddressable(&self, receiver: &Expression, ty: &Type) -> bool {
        if !self.ty_is_enum(ty) {
            return false;
        }
        let Type::Nominal {
            id: receiver_id, ..
        } = &receiver.get_type()
        else {
            return false;
        };
        matches!(
            self.facts.definition(receiver_id.as_str()).map(|d| &d.body),
            Some(DefinitionBody::Enum { .. } | DefinitionBody::TypeAlias { .. })
        )
    }

    /// Whether `ty` is a nominal type whose definition is an `enum`.
    fn ty_is_enum(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        matches!(
            self.facts.definition(id.as_str()).map(|d| &d.body),
            Some(DefinitionBody::Enum { .. })
        )
    }
}

fn tail_return_slots(planner: &Planner<'_>, inferred: &[Type]) -> Option<Vec<Type>> {
    let return_ctx = planner.return_ctx();
    let Some(Type::Tuple(slots)) = return_ctx.ty() else {
        return None;
    };
    (slots.len() == inferred.len()).then(|| slots.clone())
}

fn is_native_method_call(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Call {
            call_kind: CallKind::NativeMethod(_) | CallKind::NativeMethodIdentifier(_),
            ..
        }
    )
}

fn needs_iife_for_async(expression: &Expression, form: OperandForm) -> bool {
    if !is_native_method_call(expression) {
        return false;
    }
    !matches!(form, OperandForm::Call)
}
