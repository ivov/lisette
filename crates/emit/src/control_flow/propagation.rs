use crate::Planner;
use crate::ReturnContext;
use crate::abi::callable::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::abi::transition;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::calls::go_interop::{
    GoResultBridge, LoweredCall, is_nil, non_nil, unexpected_nil_error,
};
use crate::calls::unwrap_or::{MapLambda, map_lambda};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible};
use crate::definitions::functions::is_go_never;
use crate::names::go_name::{GeneratedPackage, PRELUDE_ERROR_ID};
use crate::patterns::matching::{
    OptionFusePlan, PreludeVariant, ResultFusePlan, prelude_constructor,
};
use crate::plan::bodies::{
    Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, Statement, assign,
    define,
};
use crate::plan::go_expression::BinaryOp;
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
use crate::state::scope::PairStatusKind;
use syntax::ast::Expression;
use syntax::types::Type;

pub(crate) fn plain_return(value: GoExpression) -> Statement {
    LoweredStatement::Return(vec![value]).into()
}

/// What `r?; Ok(())` at the end of a body returns in place of both items.
#[derive(Clone, Copy)]
pub(crate) enum ForwardedTail<'a> {
    /// A Go call with a bare `error` result.
    ErrorCall(&'a Expression),
    /// A tagged `Result<(), E>`.
    TaggedResult(&'a Expression),
}

#[derive(Clone, Copy)]
enum PropagateTarget<'a> {
    Value,
    Discard,
    Bind(&'a GoIdentifier),
}

impl<'a> PropagateTarget<'a> {
    fn bound_name(self) -> Option<&'a GoIdentifier> {
        match self {
            PropagateTarget::Bind(name) => Some(name),
            PropagateTarget::Value | PropagateTarget::Discard => None,
        }
    }
}

impl Planner<'_> {
    pub(crate) fn lower_propagate(
        &mut self,
        expression: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let (statements, value) = self.lower_propagate_to(expression, PropagateTarget::Value);
        (
            statements,
            value.expect("a value target yields the ok value"),
        )
    }

    /// Statement-position `inner?` (discards the ok value).
    pub(crate) fn lower_propagate_statement(&mut self, inner: &Expression) -> Vec<Statement> {
        self.lower_propagate_to(inner, PropagateTarget::Discard).0
    }

    pub(crate) fn lower_propagate_binding(
        &mut self,
        inner: &Expression,
        name: &GoIdentifier,
    ) -> Vec<Statement> {
        self.lower_propagate_to(inner, PropagateTarget::Bind(name))
            .0
    }

    fn lower_propagate_to(
        &mut self,
        expression: &Expression,
        target: PropagateTarget<'_>,
    ) -> (Vec<Statement>, Option<GoExpression>) {
        let expression_ty = self.facts.peel_alias(&expression.get_type());
        let fallible = Fallible::from_type(&expression_ty)
            .expect("lower_propagate called on non-Result/Option type");

        let mut statements = Vec::new();

        // `Err(...)?` / `None?` literal already emits its own return.
        if !matches!(target, PropagateTarget::Value)
            && let Some(head) = self.try_lower_error_constructor(expression, &fallible)
        {
            statements.extend(head);
            if let Some(name) = target.bound_name() {
                self.declare_zero_for_dead_path(&mut statements, name, &fallible);
            }
            return (statements, None);
        }

        if let Some(fused) = self.try_lower_fused_propagate(expression, &fallible, target) {
            return fused;
        }
        if let Some(fused) = self.try_lower_mapped_error_propagate(expression, &fallible, target) {
            return fused;
        }
        if let Some(fused) = self.try_lower_fused_option_propagate(expression, &fallible, target) {
            return fused;
        }

        let (check_setup, check) = self.hoist_propagate_check_var(expression);
        statements.extend(check_setup);
        statements.push(
            self.build_propagate_failure_check(&check, &fallible, &expression.get_type())
                .into(),
        );

        let ok_access = GoExpression::value_field(check, fallible.ok_field().to_string());
        let source = self.value_layout(&expression.get_type(), SlotOrigin::Lisette);
        let target_layout = self.value_layout(fallible.ok_ty(), SlotOrigin::Lisette);
        let coercion = CoercionPlan::bridge(
            self,
            source
                .payload()
                .expect("propagation requires a payload layout"),
            &target_layout,
        );
        let (setup, ok_access) = coercion.lower(self, ok_access);
        statements.extend(setup);
        let value = self.propagated_value(&mut statements, target, None, Some(ok_access));
        (statements, value)
    }

    /// `Err(...)?` and `None?` already emitted `return ...`. Declare the
    /// binding with a zero value so any dead code below that references it
    /// stays well-typed in Go.
    fn declare_zero_for_dead_path(
        &mut self,
        statements: &mut Vec<Statement>,
        var_name: &GoIdentifier,
        fallible: &Fallible,
    ) {
        let inner_ty = fallible.ok_ty();
        let zero = self.zero_value_expression(inner_ty);
        if self.is_declared(var_name.spelling()) {
            statements.push(assign(GoExpression::identifier(var_name.clone()), zero));
        } else {
            // Declared so the dead-path binding stays in scope for later references.
            let go_ty = self.use_go_type(inner_ty);
            statements.push(
                LoweredStatement::VarDecl {
                    name: var_name.clone(),
                    go_type: go_ty,
                    value: Some(zero),
                }
                .into(),
            );
            self.declare(var_name.spelling());
        }
    }

    fn hoist_propagate_check_var(
        &mut self,
        expression: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let plan = self.plan_operand(expression, ExpressionContext::value());
        let requires_capture =
            !matches!(expression, Expression::Identifier { .. }) || !plan.effects().can_duplicate();
        let (mut setup, value) = plan.into_parts();
        if requires_capture {
            let check = self.hoist_tmp_value_statement(&mut setup, "check", value);
            (setup, GoExpression::identifier(check))
        } else {
            (setup, value)
        }
    }

    /// The `if check.Tag != <success> { return <failure> }` failure guard.
    fn build_propagate_failure_check(
        &mut self,
        check: &GoExpression,
        fallible: &Fallible,
        expression_ty: &Type,
    ) -> LoweredStatement {
        let return_layout = self.value_layout(&self.return_ctx().expect_ty(), SlotOrigin::Lisette);
        let source_layout = self.value_layout(expression_ty, SlotOrigin::Lisette);
        let (setup, values) = if self.returns_tagged_type(expression_ty)
            && source_layout
                .underlying()
                .same_representation(return_layout.underlying())
        {
            (Vec::new(), vec![check.clone()])
        } else {
            let mut setup = Vec::new();
            let err_expr = if fallible.is_result() {
                let error = GoExpression::value_field(check.clone(), "ErrVal".to_string());
                let ValueLayout::Fallible { error: source, .. } = source_layout.underlying() else {
                    unreachable!("Result propagation requires an error layout")
                };
                let target = self.value_layout(source.logical_type(), SlotOrigin::Lisette);
                let (error_setup, error) =
                    CoercionPlan::bridge(self, source, &target).lower(self, error);
                setup.extend(error_setup);
                error
            } else {
                check.clone()
            };
            let (failure_setup, values) = self.propagate_failure_values(fallible, err_expr);
            setup.extend(failure_setup);
            (setup, values)
        };
        transition::tag_check(
            GoExpression::binary(
                GoExpression::value_field(check.clone(), "Tag".to_string()),
                BinaryOp::Ne,
                GoExpression::generated(GeneratedPackage::Prelude, fallible.success_tag()),
            ),
            setup,
            values,
        )
    }

    fn returns_tagged_type(&self, ty: &Type) -> bool {
        let return_ctx = self.return_ctx();
        return_ctx.lowered_shape().is_none()
            && return_ctx.ty().is_some_and(|return_ty| {
                self.facts.peel_alias(return_ty).demoted() == self.facts.peel_alias(ty).demoted()
            })
    }

    fn propagate_failure_values(
        &mut self,
        fallible: &Fallible,
        err_expr: GoExpression,
    ) -> (Vec<Statement>, Vec<GoExpression>) {
        let mut setup = Vec::new();
        let error = fallible
            .is_result()
            .then(|| self.convert_error_to_return_context(&mut setup, err_expr, fallible));
        let values = self.failure_return_values(fallible, error);
        (setup, values)
    }

    /// Fuse `call()?` on a lowered-ABI callee into a direct failure check
    /// (`if err != nil`), skipping the tagged round trip.
    fn try_lower_fused_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        let LoweredCall {
            call,
            wraps,
            shape,
            ok_ty,
            nil_guard,
            bridge,
            ..
        } = self.lowered_call(expression)?;
        let payload_bridge = match bridge {
            Some(GoResultBridge::Payload { value, error }) if error.is_identity() => Some(value),
            Some(GoResultBridge::Payload { .. }) => return None,
            _ => None,
        };
        match shape {
            LoweredReturnAbi::Result {
                payload: PayloadLayout::Packed,
            } if !ok_ty.is_unit() => {}
            LoweredReturnAbi::BareError => {}
            _ => return None,
        }
        let has_value_slot = !matches!(shape, LoweredReturnAbi::BareError);
        if !self.returns_fallible() {
            return None;
        }

        let (mut statements, call) = self
            .lower_call(call, None, ExpressionContext::value())
            .into_parts();
        let want_value = !matches!(target, PropagateTarget::Discard);
        let named = if has_value_slot && payload_bridge.is_none() {
            self.declare_propagate_target(target)
        } else {
            None
        };
        let value_var = (has_value_slot && (want_value || nil_guard.is_some())).then(|| {
            named
                .cloned()
                .unwrap_or_else(|| self.fresh_pair_identifier())
        });
        let (message_setup, wraps) = self.prepare_wrap_messages(&wraps, true);
        let opens_if = value_var.is_none() && message_setup.is_empty();
        let outcome_var = self.pair_status(None, PairStatusKind::Error, opens_if);
        let outcome = || GoExpression::identifier(outcome_var.clone());
        let binding = Definition {
            names: match &value_var {
                Some(value) => vec![value.clone(), outcome_var.clone()],
                None if has_value_slot => vec!["_".to_string().into(), outcome_var.clone()],
                None => vec![outcome_var.clone()],
            },
            value: call,
        };
        let initializer = if opens_if {
            Some(binding)
        } else {
            statements.push(LoweredStatement::Define(binding).into());
            None
        };
        statements.extend(message_setup);
        let guarded_value = || {
            GoExpression::identifier(value_var.clone().expect("nil guard requires the value var"))
        };

        let error = self.wrap_error(&wraps, outcome());
        let (failure_setup, failure_values) = self.propagate_failure_values(fallible, error);
        statements.push(
            transition::tag_check_with_initializer(
                initializer,
                non_nil(outcome()),
                failure_setup,
                failure_values,
            )
            .into(),
        );
        if let Some(guard) = nil_guard {
            let error = self.wrap_error(&wraps, unexpected_nil_error());
            let (nil_setup, nil_failure) = self.propagate_failure_values(fallible, error);
            statements.push(
                transition::tag_check(guard.is_nil(guarded_value()), nil_setup, nil_failure).into(),
            );
        }

        let ok_value = value_var.map(|val| {
            let val = GoExpression::identifier(val);
            match &payload_bridge {
                Some(bridge) if want_value => self.plan_layout_bridge(&mut statements, val, bridge),
                _ => val,
            }
        });
        let ok_value =
            ok_value.unwrap_or_else(|| GoExpression::empty_composite("struct{}".to_string()));
        let value = self.propagated_value(&mut statements, target, named, Some(ok_value));
        Some((statements, value))
    }

    /// Fuse `source?` on a native Option source into its Go test.
    fn try_lower_fused_option_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        if fallible.is_result() || !self.returns_fallible() {
            return None;
        }
        let fuse = self.option_fuse_plan(expression)?;
        let named = self.declare_propagate_target(target);
        let slot = match (named, target) {
            (Some(name), _) => CommaOkValueSlot::Named(name.clone()),
            (None, PropagateTarget::Discard) => CommaOkValueSlot::Unused,
            (None, _) => CommaOkValueSlot::Temp,
        };
        let bound = fuse.bind(self, slot);
        let failure = bound.failure_condition();
        let late_binding = bound.late_binding();
        let payload = bound.payload();
        let reads_element = !bound.binds_value();
        let mut statements = bound.statements;
        let failure_values = self.failure_return_values(fallible, None);
        statements.push(
            transition::tag_check_with_initializer(
                failure.initializer,
                failure.condition,
                Vec::new(),
                failure_values,
            )
            .into(),
        );
        statements.extend(late_binding);
        let payload = match (target, payload) {
            (PropagateTarget::Value, Some(payload)) if reads_element => {
                Some(GoExpression::identifier(self.hoist_tmp_value_statement(
                    &mut statements,
                    "elem",
                    payload,
                )))
            }
            (_, payload) => payload,
        };
        let value = self.propagated_value(&mut statements, target, named, payload);
        Some((statements, value))
    }

    /// Fuse `call.map_err(|e| ..)?`, `source.ok_or(error)?` and
    /// `source.ok_or_else(|| ..)?` into the source's Go test.
    fn try_lower_mapped_error_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        target: PropagateTarget<'_>,
    ) -> Option<(Vec<Statement>, Option<GoExpression>)> {
        if !fallible.is_result() || !self.returns_fallible() {
            return None;
        }
        enum Mapped<'a> {
            MapErr(ResultFusePlan<'a>, MapLambda<'a>),
            OkOr(OptionFusePlan<'a>, &'a Expression),
            OkOrElse(OptionFusePlan<'a>, MapLambda<'a>),
        }
        let mapped = if let Some((fuse, map)) = self.map_err_call(expression) {
            if !fuse.carries_payload() && !matches!(target, PropagateTarget::Discard) {
                return None;
            }
            Mapped::MapErr(fuse, map)
        } else if let Some((receiver, [error])) = self.prelude_method_call(expression, "ok_or") {
            Mapped::OkOr(self.option_fuse_plan(receiver)?, error)
        } else if let Some((receiver, [function])) =
            self.prelude_method_call(expression, "ok_or_else")
        {
            let map = map_lambda(function)?;
            Mapped::OkOrElse(self.option_fuse_plan(receiver)?, map)
        } else {
            return None;
        };

        let named = self.declare_propagate_target(target);
        let slot = match (named, target) {
            (Some(name), _) => CommaOkValueSlot::Named(name.clone()),
            (None, PropagateTarget::Discard) => CommaOkValueSlot::Discarded,
            (None, _) => CommaOkValueSlot::Temp,
        };

        let (mut statements, failure, payload, late_binding, mut failure_setup, error) =
            match mapped {
                Mapped::MapErr(fuse, map) => {
                    let has_nil_guard = fuse.has_nil_guard();
                    let pair = fuse.bind(self, slot, None);
                    let failure = pair.failure_condition();
                    let status = pair.status_identifier().clone();
                    let mut failure_setup = Vec::new();
                    if has_nil_guard && map.param.is_some() {
                        failure_setup.push(
                            LoweredStatement::If(IfPlan::plain(
                                is_nil(GoExpression::identifier(status.clone())),
                                LoweredBlock {
                                    statements: vec![assign(
                                        GoExpression::identifier(status.clone()),
                                        unexpected_nil_error(),
                                    )],
                                },
                                ElseArm::None,
                            ))
                            .into(),
                        );
                    }
                    let (body_setup, error) = self.with_binding_frame(|this| {
                        if let Some(param) = map.param {
                            this.scope.bind_identifier(
                                param.name,
                                param.binding.as_slice(),
                                status.clone(),
                            );
                        }
                        this.lower_composite_value(map.body, ExpressionContext::value())
                            .into_parts()
                    });
                    failure_setup.extend(body_setup);
                    let payload = pair.payload();
                    (
                        pair.statements,
                        failure,
                        payload,
                        None,
                        failure_setup,
                        error,
                    )
                }
                Mapped::OkOr(fuse, error) => {
                    let bound = fuse.bind(self, slot);
                    let failure = bound.failure_condition();
                    let late_binding = bound.late_binding();
                    let payload = bound.payload();
                    let mut statements = bound.statements;
                    let value = self.lower_composite_value(error, ExpressionContext::value());
                    let (error_setup, error) =
                        self.eager_operand(error, value, "failure").into_parts();
                    statements.extend(error_setup);
                    (
                        statements,
                        failure,
                        payload,
                        late_binding,
                        Vec::new(),
                        error,
                    )
                }
                Mapped::OkOrElse(fuse, map) => {
                    let bound = fuse.bind(self, slot);
                    let failure = bound.failure_condition();
                    let late_binding = bound.late_binding();
                    let payload = bound.payload();
                    let (failure_setup, error) = self
                        .with_binding_frame(|this| {
                            this.lower_composite_value(map.body, ExpressionContext::value())
                        })
                        .into_parts();
                    (
                        bound.statements,
                        failure,
                        payload,
                        late_binding,
                        failure_setup,
                        error,
                    )
                }
            };
        let (values_setup, failure_values) = self.propagate_failure_values(fallible, error);
        failure_setup.extend(values_setup);
        statements.push(
            transition::tag_check_with_initializer(
                failure.initializer,
                failure.condition,
                failure_setup,
                failure_values,
            )
            .into(),
        );
        statements.extend(late_binding);
        let value = self.propagated_value(&mut statements, target, named, payload);
        Some((statements, value))
    }

    /// `call.map_err(|e| ..)` on a call whose Go result can be tested in place.
    fn map_err_call<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<(ResultFusePlan<'a>, MapLambda<'a>)> {
        let (receiver, [function]) = self.prelude_method_call(expression, "map_err")? else {
            return None;
        };
        let map = map_lambda(function)?;
        Some((self.result_fuse_plan(receiver)?, map))
    }

    /// `r?; Ok(())` at the end of a body returns `r` itself, when `r` has the shape of the return.
    pub(crate) fn forwarded_tail<'a>(
        &self,
        check: &'a Expression,
        last: &Expression,
    ) -> Option<ForwardedTail<'a>> {
        let Expression::Propagate {
            expression: inner, ..
        } = check.unwrap_parens()
        else {
            return None;
        };
        let Expression::Call {
            expression: callee,
            args,
            ..
        } = last.unwrap_parens()
        else {
            return None;
        };
        if prelude_constructor(callee) != Some(PreludeVariant::Ok)
            || !matches!(args.as_slice(), [Expression::Unit { .. }])
        {
            return None;
        }
        match self.return_ctx().lowered_shape() {
            Some(LoweredReturnAbi::BareError) => self
                .forwarded_error_call(inner)
                .map(ForwardedTail::ErrorCall),
            None => self
                .forwarded_tagged_result(inner)
                .map(ForwardedTail::TaggedResult),
            Some(_) => None,
        }
    }

    /// The call returns as is, unless a concrete error type would turn nil into non-nil.
    fn forwarded_error_call<'a>(&self, inner: &'a Expression) -> Option<&'a Expression> {
        if !self.is_error_interface(&self.facts.peel_alias(self.return_ctx().ty()?).err_type()) {
            return None;
        }
        let lowered = self.lowered_call(inner)?;
        if lowered.shape != LoweredReturnAbi::BareError
            || !lowered.wraps.is_empty()
            || lowered.is_bridged()
            || !self.is_error_interface(&self.facts.peel_alias(&lowered.call.get_type()).err_type())
        {
            return None;
        }
        Some(lowered.call)
    }

    /// A tagged `Result<(), E>` returns as is when it has the return type of the function.
    fn forwarded_tagged_result<'a>(&self, inner: &'a Expression) -> Option<&'a Expression> {
        let return_ty = self.facts.peel_alias(self.return_ctx().ty()?);
        let inner_ty = self.facts.peel_alias(&inner.get_type());
        (return_ty.is_result()
            && self.facts.peel_alias(&return_ty.ok_type()).is_unit()
            && return_ty == inner_ty)
            .then_some(inner)
    }

    fn is_error_interface(&self, ty: &Type) -> bool {
        matches!(
            self.facts.peel_alias(ty),
            Type::Nominal { id, .. } if id.as_str() == PRELUDE_ERROR_ID
        )
    }

    fn returns_fallible(&self) -> bool {
        let return_ctx = self.return_ctx();
        return_ctx.lowered_shape().is_some()
            || return_ctx
                .ty()
                .is_some_and(|ty| Fallible::from_type(ty).is_some())
    }

    fn declare_propagate_target<'a>(
        &mut self,
        target: PropagateTarget<'a>,
    ) -> Option<&'a GoIdentifier> {
        let name = target
            .bound_name()
            .filter(|name| !self.is_declared(name.spelling()))?;
        self.declare(name.spelling());
        Some(name)
    }

    /// The `?` value for `target`. A bound target is assigned unless `named` already holds it.
    fn propagated_value(
        &mut self,
        statements: &mut Vec<Statement>,
        target: PropagateTarget<'_>,
        named: Option<&GoIdentifier>,
        payload: Option<GoExpression>,
    ) -> Option<GoExpression> {
        let payload = || payload.expect("a propagated source carries its payload");
        match target {
            PropagateTarget::Value => Some(payload()),
            PropagateTarget::Discard => None,
            PropagateTarget::Bind(_) if named.is_some() => None,
            PropagateTarget::Bind(name) => {
                let ok = payload();
                statements.push(self.bind_propagate_ok(name, ok));
                None
            }
        }
    }

    fn bind_propagate_ok(&mut self, name: &GoIdentifier, ok_access: GoExpression) -> Statement {
        if self.is_declared(name.spelling()) {
            assign(GoExpression::identifier(name.clone()), ok_access)
        } else {
            self.declare(name.spelling());
            define(name.clone(), ok_access)
        }
    }

    pub(crate) fn build_return_plan(&mut self, expression: &Expression) -> LoweredBlock {
        let return_ctx = self.return_ctx();
        let is_unit = return_ctx
            .ty()
            .is_some_and(|ty| self.facts.peel_alias(ty).is_unit());
        if is_unit {
            // Unit expressions still run for their effects. The enclosing
            // physical signature decides whether `return` carries unit data.
            let is_pure = matches!(
                expression,
                Expression::Unit { .. }
                    | Expression::Identifier { .. }
                    | Expression::Literal { .. }
            );
            let mut statements = Vec::new();
            if !is_pure {
                statements.push(self.lower_statement(expression));
            }
            statements.push(if matches!(return_ctx, ReturnContext::Void(_)) {
                LoweredStatement::Return(Vec::new()).into()
            } else {
                plain_return(GoExpression::empty_composite("struct{}".to_string()))
            });
            return LoweredBlock { statements };
        }

        if let Some(statements) = transition::try_emit_layout_return(self, expression) {
            return LoweredBlock { statements };
        }

        if let Some(statements) = self.lower_wrapped_return(expression) {
            return LoweredBlock { statements };
        }

        let (mut statements, value) = self
            .lower_value(expression, ExpressionContext::value())
            .into_parts();
        let value = self.apply_type_coercion(&mut statements, return_ctx.ty(), expression, value);
        statements.push(plain_return(value));
        LoweredBlock { statements }
    }

    /// Lower a Result/Option-wrapped return into structured statement IR.
    ///
    /// Returns `None` only when the return type is NOT Result/Option
    /// (`Fallible::from_type` returns `None`); the caller then emits a plain
    /// return. Once a Result/Option return type is identified this is
    /// exhaustive: every path yields the wrapped-return statements.
    pub(crate) fn lower_wrapped_return(
        &mut self,
        expression: &Expression,
    ) -> Option<Vec<Statement>> {
        let expression_ty = self.facts.peel_alias(&expression.get_type());
        let return_ctx = self.return_ctx();

        let return_ty = return_ctx
            .ty()
            .filter(|ty| Fallible::from_type(ty).is_some())
            .cloned()
            .unwrap_or(expression_ty);

        let fallible = Fallible::from_type(&return_ty)?;

        let mut statements = Vec::new();

        if is_go_never(expression) {
            let (setup, call) = self
                .lower_call(expression, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.push(
                LoweredStatement::ExpressionStatement {
                    expression: call,
                    diverges: true,
                }
                .into(),
            );
            return Some(statements);
        }

        if fallible.classify_constructor(expression) == Some(ConstructorKind::Failure) {
            // Only `None` reaches here, since `Err` always has a payload.
            statements.extend(self.lower_failure_constructor_return(&[], &fallible, &[]));
            return Some(statements);
        }

        if matches!(expression, Expression::Call { .. }) {
            statements.extend(self.lower_wrapped_call_return(expression, &fallible, &return_ty));
            return Some(statements);
        }

        if matches!(
            expression,
            Expression::If { .. } | Expression::IfLet { .. } | Expression::Match { .. }
        ) {
            let block = self.lower_branching_to_block(expression, &PlacePlan::Return);
            statements.extend(block.statements);
            return Some(statements);
        }

        if let Expression::Propagate {
            expression: inner, ..
        } = expression
        {
            let (setup, value) = self.lower_propagate(inner);
            statements.extend(setup);
            statements.extend(self.wrapped_value_return(value, &return_ty));
            return Some(statements);
        }

        let (setup, value) = self
            .lower_value(expression, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        statements.extend(self.wrapped_value_return(value, &return_ty));
        Some(statements)
    }

    fn wrapped_value_return(&mut self, value: GoExpression, return_ty: &Type) -> Vec<Statement> {
        let Some(shape) = self.return_ctx().lowered_shape() else {
            return vec![plain_return(value)];
        };
        transition::emit_lowered_result_return(self, value, "v", return_ty, &shape)
    }

    /// Lower a return for a call whose result is wrapped in the function's
    /// Result/Option return type. Success/Failure constructors collapse
    /// directly; other calls return the call expression.
    fn lower_wrapped_call_return(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let Expression::Call {
            expression: call_expression,
            args,
            ..
        } = expression
        else {
            unreachable!("lower_wrapped_call_return requires a Call expression");
        };
        let (inner, wraps) = self.peel_wrap_err(expression);
        if let Expression::Call {
            expression: inner_callee,
            args: inner_args,
            ..
        } = inner
            && !wraps.is_empty()
            && fallible.classify_constructor(inner_callee) == Some(ConstructorKind::Failure)
        {
            return self.lower_failure_constructor_return(inner_args, fallible, &wraps);
        }
        match fallible.classify_constructor(call_expression) {
            Some(ConstructorKind::Success) => self.lower_success_constructor_return(args, fallible),
            Some(ConstructorKind::Failure) => {
                self.lower_failure_constructor_return(args, fallible, &[])
            }
            None => self.lower_wrapped_passthrough_return(expression, return_ty),
        }
    }

    fn lower_success_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        match self.return_ctx().lowered_shape().as_ref() {
            Some(LoweredReturnAbi::BareError) => {
                if !args.is_empty() {
                    let (setup, _) = self
                        .lower_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                }
                statements.push(transition::multi_value_return(
                    transition::lowered_ok_values(&LoweredReturnAbi::BareError, Vec::new()),
                ));
            }
            Some(shape) if args.is_empty() => {
                statements.push(transition::multi_value_return(
                    transition::lowered_ok_values(
                        shape,
                        vec![GoExpression::empty_composite("struct{}".to_string())],
                    ),
                ));
            }
            Some(shape)
                if shape.has_flattened_payload()
                    && let Expression::Tuple { elements, .. } = args[0].unwrap_parens() =>
            {
                let (setup, parts) =
                    transition::lowered_tuple_literal_values(self, elements, fallible.ok_ty());
                statements.extend(setup);
                statements.push(transition::multi_value_return(
                    transition::lowered_ok_values(shape, parts),
                ));
            }
            _ => {
                let (setup, value) = self
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                let value = self.apply_type_coercion(
                    &mut statements,
                    Some(fallible.ok_ty()),
                    &args[0],
                    value,
                );
                statements.extend(self.success_return(fallible, value));
            }
        }
        statements
    }

    fn lower_failure_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
        wraps: &[&Expression],
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        let error = args.first().map(|arg| {
            let error = self.lower_composite_value(arg, ExpressionContext::value());
            let (message_setup, wraps) = self.prepare_wrap_messages(wraps, true);
            let error = if message_setup.is_empty() {
                error
            } else {
                self.eager_operand(arg, error, "err")
            };
            let (setup, error) = error.into_parts();
            statements.extend(setup);
            let to = self
                .contextual_err_ty(fallible)
                .expect("Result must have error type");
            let error = self.apply_type_coercion(&mut statements, Some(&to), arg, error);
            statements.extend(message_setup);
            self.wrap_error(&wraps, error)
        });
        statements.push(self.failure_return(fallible, error));
        statements
    }

    /// Tail return for a non-constructor call.
    fn lower_wrapped_passthrough_return(
        &mut self,
        expression: &Expression,
        return_ty: &Type,
    ) -> Vec<Statement> {
        let lowered = self.return_ctx().lowered_shape();
        let lowered = lowered.as_ref();
        let mut statements = Vec::new();
        if let Some(LoweredReturnAbi::Option(_)) = lowered
            && self.facts.peel_alias(return_ty).demoted()
                == self.facts.peel_alias(&expression.get_type()).demoted()
            && matches!(
                self.option_fuse_plan(expression),
                Some(OptionFusePlan::Found { .. })
            )
        {
            let (setup, value) = self.lower_propagate(expression);
            statements.extend(setup);
            let fallible = Fallible::from_type(&self.facts.peel_alias(return_ty))
                .expect("an Option return has a fallible type");
            statements.extend(self.success_return(&fallible, value));
            return statements;
        }
        if matches!(
            lowered,
            Some(LoweredReturnAbi::Result { .. } | LoweredReturnAbi::BareError)
        ) && (self
            .result_fuse_plan(expression)
            .is_some_and(|plan| plan.wraps_error())
            || self
                .map_err_call(expression)
                .is_some_and(|(plan, _)| plan.carries_payload()))
        {
            let (setup, value) = self.lower_propagate(expression);
            statements.extend(setup);
            let fallible = Fallible::from_type(&self.facts.peel_alias(return_ty))
                .expect("a lowered Result shape has a fallible return type");
            statements.extend(self.success_return(&fallible, value));
            return statements;
        }
        if let Some(shape) = lowered
            && self.callee_matches_lowered_shape(expression, shape)
        {
            let (setup, call) = self
                .lower_call(expression, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.push(plain_return(call));
            return statements;
        }
        if let Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        })) = lowered
            && let Some(source) = self.comma_ok_source(expression)
            && !source.has_nil_guard()
        {
            let pair = self.bind_comma_ok_pair(expression, source, CommaOkValueSlot::Temp);
            let ok = GoExpression::identifier(pair.status_identifier().clone());
            let value = GoExpression::identifier(
                pair.payload_identifier()
                    .expect("Temp slot always captures the value")
                    .clone(),
            );
            let mut statements = pair.statements;
            statements.push(transition::multi_value_return(vec![value, ok]));
            return statements;
        }
        if let Some(plan) = self.plan_call(expression)
            && plan.resolved.abi.result.is_lowered()
        {
            if let Some(shape) = lowered {
                let (setup, result_var) = self
                    .lower_go_abi_wrapped_call(expression, &plan.resolved.abi, return_ty)
                    .into_parts();
                statements.extend(setup);
                statements.extend(transition::emit_lowered_result_return(
                    self, result_var, "res", return_ty, shape,
                ));
            } else {
                let abi = plan.resolved.abi.clone();
                if self.go_result_bridge(&abi, return_ty).is_none() {
                    let (setup, call) = self
                        .lower_call(expression, None, ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                    statements.extend(self.lower_abi_to_tagged_return(
                        call,
                        &abi.result,
                        return_ty,
                    ));
                } else {
                    let (setup, result_var) = self
                        .lower_go_abi_wrapped_call(expression, &abi, return_ty)
                        .into_parts();
                    statements.extend(setup);
                    statements.push(plain_return(result_var));
                }
            }
            return statements;
        }
        if let Some(shape) = lowered {
            let (setup, value) = self
                .lower_value(expression, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.extend(transition::emit_lowered_result_return(
                self, value, "v", return_ty, shape,
            ));
            return statements;
        }
        let (setup, call) = self
            .lower_call(expression, None, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        statements.push(plain_return(call));
        statements
    }

    /// True when the callee already has the enclosing shape, so a tail return forwards it.
    fn callee_matches_lowered_shape(
        &self,
        call_expression: &Expression,
        enclosing_shape: &LoweredReturnAbi,
    ) -> bool {
        let Some(plan) = self.plan_call(call_expression) else {
            return false;
        };
        plan.resolved.abi.result.lowered() == Some(enclosing_shape)
            && self
                .go_result_bridge(&plan.resolved.abi, &call_expression.get_type())
                .is_none()
    }
}
