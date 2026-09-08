use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::transition;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::calls::go_interop::{LoweredCall, non_nil, unexpected_nil_error};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible};
use crate::definitions::functions::is_go_never;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{Definition, LoweredBlock, LoweredStatement, PlacePlan, assign, define};
use crate::plan::values::GoExpression;
use crate::state::scope::PairStatusKind;
use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Clone, Copy)]
struct WrappedReturnInfo<'a> {
    fallible: &'a Fallible,
    return_ty: &'a Type,
    lowered: Option<&'a CallableReturnAbi>,
}

pub(crate) fn plain_return(value: GoExpression) -> LoweredStatement {
    LoweredStatement::Return(vec![value])
}

impl Planner<'_> {
    /// Lower `?` into structured IR plus the ok-access value. `result_var_name`:
    /// `None` returns `check.OkVal`, `Some("_")` discards, `Some(name)` binds
    /// `name := check.OkVal`.
    pub(crate) fn lower_propagate(
        &mut self,
        expression: &Expression,
        result_var_name: Option<&str>,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let expression_ty = self.facts.peel_alias(&expression.get_type());
        let fallible = Fallible::from_type(&expression_ty)
            .expect("lower_propagate called on non-Result/Option type");

        let mut statements = Vec::new();

        // `Err(...)?` / `None?` literal already emits its own return.
        if let Some(var_name) = result_var_name
            && let Some(head) = self.try_lower_error_constructor(expression, &fallible)
        {
            statements.extend(head);
            self.declare_zero_for_dead_path(&mut statements, var_name, &fallible);
            return (statements, GoExpression::empty());
        }

        if let Some(fused) = self.try_lower_fused_propagate(expression, &fallible, result_var_name)
        {
            return fused;
        }

        let (check_setup, check) = self.hoist_propagate_check_var(expression);
        statements.extend(check_setup);
        statements.push(self.build_propagate_failure_check(&check, &fallible));

        let ok_access = GoExpression::selector(check, fallible.ok_field().to_string());
        let value = match result_var_name {
            None => ok_access,
            Some("_") => GoExpression::name("_".to_string()),
            Some(name) => {
                statements.push(self.bind_propagate_ok(name, ok_access));
                GoExpression::name(name.to_string())
            }
        };
        (statements, value)
    }

    /// `Err(...)?` and `None?` already emitted `return ...`. Declare the
    /// binding with a zero value so any dead code below that references it
    /// stays well-typed in Go.
    fn declare_zero_for_dead_path(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        var_name: &str,
        fallible: &Fallible,
    ) {
        if var_name == "_" {
            return;
        }
        let inner_ty = fallible.ok_ty();
        let zero = self.zero_value_expression(inner_ty);
        if self.is_declared(var_name) {
            statements.push(assign(GoExpression::name(var_name.to_string()), zero));
        } else {
            // Declared so the dead-path binding stays in scope for later references.
            let go_ty = self.use_go_type(inner_ty);
            statements.push(LoweredStatement::VarDecl {
                name: var_name.to_string(),
                go_type: go_ty,
                value: Some(zero),
            });
            self.declare(var_name);
        }
    }

    fn hoist_propagate_check_var(
        &mut self,
        expression: &Expression,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let plan = self.plan_operand(expression, ExpressionContext::value());
        let requires_capture =
            !matches!(expression, Expression::Identifier { .. }) || plan.expression.does_work();
        let (mut setup, value) = plan.into_parts();
        if requires_capture {
            let check = self.hoist_tmp_value_statement(&mut setup, "check", value);
            (setup, GoExpression::name(check))
        } else {
            (setup, value)
        }
    }

    /// The `if check.Tag != <success> { return <failure> }` failure guard.
    fn build_propagate_failure_check(
        &mut self,
        check: &GoExpression,
        fallible: &Fallible,
    ) -> LoweredStatement {
        let err_expr = if fallible.is_result() {
            GoExpression::selector(check.clone(), "ErrVal".to_string())
        } else {
            check.clone()
        };
        let (setup, values) = self.propagate_failure_values(fallible, err_expr);
        transition::tag_check(
            GoExpression::binary(
                GoExpression::selector(check.clone(), "Tag".to_string()),
                "!=",
                GoExpression::generated(GeneratedPackage::Prelude, fallible.success_tag()),
            ),
            setup,
            values,
        )
    }

    fn propagate_failure_values(
        &mut self,
        fallible: &Fallible,
        err_expr: GoExpression,
    ) -> (Vec<LoweredStatement>, Vec<GoExpression>) {
        let mut setup = Vec::new();
        let error = fallible
            .is_result()
            .then(|| self.convert_error_to_return_context(&mut setup, err_expr, fallible));
        let values = self.failure_return_values(fallible, error);
        (setup, values)
    }

    /// Fuse `call()?` on a lowered-ABI callee into a direct failure check
    /// (`if err != nil` / `if !ok`), skipping the tagged round trip.
    fn try_lower_fused_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        result_var_name: Option<&str>,
    ) -> Option<(Vec<LoweredStatement>, GoExpression)> {
        let LoweredCall {
            call,
            wraps,
            shape,
            ok_ty,
            nil_guard,
            payload_bridge,
            ..
        } = self.lowered_call(expression)?;
        let comma_ok = match shape {
            CallableReturnAbi::Result {
                payload: PayloadLayout::Packed,
            } => {
                if ok_ty.is_unit() {
                    return None;
                }
                false
            }
            CallableReturnAbi::BareError => false,
            CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
                payload: PayloadLayout::Packed,
            }) => true,
            _ => return None,
        };
        let has_value_slot = !matches!(shape, CallableReturnAbi::BareError);
        let return_ctx = self.return_ctx();
        let has_fallible_return = return_ctx.lowered_shape().is_some()
            || return_ctx
                .ty()
                .is_some_and(|ty| Fallible::from_type(ty).is_some());
        if !has_fallible_return {
            return None;
        }

        let (mut statements, call) = self
            .lower_call(call, None, ExpressionContext::value())
            .into_parts();
        let want_value = !matches!(result_var_name, Some("_"));
        let let_slot = result_var_name.filter(|name| {
            has_value_slot && *name != "_" && payload_bridge.is_none() && !self.is_declared(name)
        });
        let value_var =
            (has_value_slot && (want_value || nil_guard.is_some())).then(|| match let_slot {
                Some(name) => {
                    self.declare(name);
                    name.to_string()
                }
                None => self.fresh_pair_value(),
            });
        let status_kind = if comma_ok {
            PairStatusKind::Ok
        } else {
            PairStatusKind::Error
        };
        let (message_setup, wraps) = self.prepare_wrap_messages(&wraps, true);
        let opens_if = value_var.is_none() && message_setup.is_empty();
        let outcome_var = self.pair_status(None, status_kind, opens_if);
        let outcome = || GoExpression::name(outcome_var.clone());
        let binding = Definition {
            names: match &value_var {
                Some(value) => vec![value.clone(), outcome_var.clone()],
                None if has_value_slot => vec!["_".to_string(), outcome_var.clone()],
                None => vec![outcome_var.clone()],
            },
            value: call,
        };
        let initializer = if opens_if {
            Some(binding)
        } else {
            statements.push(LoweredStatement::Define(binding));
            None
        };
        statements.extend(message_setup);
        let guarded_value =
            || GoExpression::name(value_var.clone().expect("nil guard requires the value var"));

        if comma_ok {
            let failure_condition = match nil_guard {
                Some(guard) => GoExpression::binary(
                    GoExpression::unary("!", outcome()),
                    "||",
                    guard.is_nil(guarded_value()),
                ),
                None => GoExpression::unary("!", outcome()),
            };
            let (failure_setup, failure_values) =
                self.propagate_failure_values(fallible, outcome());
            statements.push(transition::tag_check_with_initializer(
                initializer,
                failure_condition,
                failure_setup,
                failure_values,
            ));
        } else {
            let error = self.wrap_error(&wraps, outcome());
            let (failure_setup, failure_values) = self.propagate_failure_values(fallible, error);
            statements.push(transition::tag_check_with_initializer(
                initializer,
                non_nil(outcome()),
                failure_setup,
                failure_values,
            ));
            if let Some(guard) = nil_guard {
                let error = self.wrap_error(&wraps, unexpected_nil_error());
                let (nil_setup, nil_failure) = self.propagate_failure_values(fallible, error);
                statements.push(transition::tag_check(
                    guard.is_nil(guarded_value()),
                    nil_setup,
                    nil_failure,
                ));
            }
        }

        let ok_value = value_var.map(|val| {
            let val = GoExpression::name(val);
            match &payload_bridge {
                Some(bridge) if want_value => self.plan_layout_bridge(&mut statements, val, bridge),
                _ => val,
            }
        });
        let unit = || GoExpression::empty_composite("struct{}".to_string());

        let value = match result_var_name {
            None => ok_value.unwrap_or_else(unit),
            Some("_") => GoExpression::name("_".to_string()),
            Some(name) if let_slot.is_some() => GoExpression::name(name.to_string()),
            Some(name) => {
                let v = ok_value.unwrap_or_else(unit);
                statements.push(self.bind_propagate_ok(name, v));
                GoExpression::name(name.to_string())
            }
        };
        Some((statements, value))
    }

    /// Statement-position `inner?` (discards the ok value).
    pub(crate) fn lower_propagate_statement(
        &mut self,
        inner: &Expression,
    ) -> Vec<LoweredStatement> {
        self.lower_propagate(inner, Some("_")).0
    }

    fn bind_propagate_ok(&mut self, name: &str, ok_access: GoExpression) -> LoweredStatement {
        if self.is_declared(name) {
            assign(GoExpression::name(name.to_string()), ok_access)
        } else {
            self.declare(name);
            define(name.to_string(), ok_access)
        }
    }

    pub(crate) fn build_return_plan(&mut self, expression: &Expression) -> LoweredBlock {
        let return_ctx = self.return_ctx();
        let is_unit = return_ctx.ty().is_some_and(Type::is_unit);
        if is_unit {
            // Unit return: impure expressions run as a statement before the
            // bare `return`; pure ones (Unit, Identifier, Literal) emit nothing.
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
            statements.push(LoweredStatement::Return(Vec::new()));
            return LoweredBlock { statements };
        }

        if let Some(statements) = transition::try_emit_lowered_tail_return(self, expression) {
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
    ) -> Option<Vec<LoweredStatement>> {
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
            statements.push(LoweredStatement::ExpressionStatement {
                expression: call,
                diverges: true,
            });
            return Some(statements);
        }

        let lowered = return_ctx.lowered_shape();

        if let Expression::Identifier { .. } = expression
            && fallible.classify_constructor(expression) == Some(ConstructorKind::Failure)
        {
            // Only `None` reaches here. `Err` always has a payload, so an
            // identifier failure constructor must be a payload-less Option.
            statements.extend(self.lower_failure_constructor_return(&[], &fallible, &[]));
            return Some(statements);
        }

        let info = WrappedReturnInfo {
            fallible: &fallible,
            return_ty: &return_ty,
            lowered: lowered.as_ref(),
        };

        if matches!(expression, Expression::Call { .. }) {
            statements.extend(self.lower_wrapped_call_return(expression, info));
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
            let (setup, value) = self.lower_propagate(inner, None);
            statements.extend(setup);
            statements.extend(self.wrapped_value_return(value, &return_ty, lowered.as_ref()));
            return Some(statements);
        }

        let (setup, value) = self
            .lower_value(expression, ExpressionContext::value())
            .into_parts();
        statements.extend(setup);
        statements.extend(self.wrapped_value_return(value, &return_ty, lowered.as_ref()));
        Some(statements)
    }

    fn wrapped_value_return(
        &mut self,
        value: GoExpression,
        return_ty: &Type,
        lowered: Option<&CallableReturnAbi>,
    ) -> Vec<LoweredStatement> {
        let Some(shape) = lowered else {
            return vec![plain_return(value)];
        };
        // The destructure references the value multiple times (`.Tag`,
        // `.OkVal`, `.ErrVal` etc.); hoist to avoid re-evaluating.
        let mut statements = Vec::new();
        let temp = GoExpression::name(self.hoist_tmp_value_statement(&mut statements, "v", value));
        statements.extend(transition::emit_lowered_result_return(
            self, &temp, return_ty, shape,
        ));
        statements
    }

    /// Lower a return for a call whose result is wrapped in the function's
    /// Result/Option return type. Success/Failure constructors collapse
    /// directly; other calls return the call expression.
    fn lower_wrapped_call_return(
        &mut self,
        expression: &Expression,
        info: WrappedReturnInfo<'_>,
    ) -> Vec<LoweredStatement> {
        let WrappedReturnInfo {
            fallible,
            return_ty,
            lowered,
        } = info;
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
            Some(ConstructorKind::Success) => {
                self.lower_success_constructor_return(args, fallible, lowered)
            }
            Some(ConstructorKind::Failure) => {
                self.lower_failure_constructor_return(args, fallible, &[])
            }
            None => self.lower_wrapped_passthrough_return(expression, return_ty, lowered),
        }
    }

    fn lower_success_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
        lowered: Option<&CallableReturnAbi>,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        match lowered {
            Some(CallableReturnAbi::BareError) => {
                if !args.is_empty() {
                    let (setup, _) = self
                        .lower_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                }
                statements.push(transition::multi_value_return(
                    transition::lowered_ok_values(&CallableReturnAbi::BareError, Vec::new()),
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
                statements.extend(self.success_return(fallible, value, lowered));
            }
        }
        statements
    }

    fn lower_failure_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
        wraps: &[&Expression],
    ) -> Vec<LoweredStatement> {
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
            let error = self.coerce_value(&mut statements, error, &arg.get_type(), &to);
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
        lowered: Option<&CallableReturnAbi>,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered
            && matches!(
                shape,
                CallableReturnAbi::Result { .. } | CallableReturnAbi::BareError
            )
            && self
                .result_fuse_plan(expression)
                .is_some_and(|plan| plan.wraps_error())
        {
            let (setup, value) = self.lower_propagate(expression, None);
            statements.extend(setup);
            let fallible = Fallible::from_type(&self.facts.peel_alias(return_ty))
                .expect("a lowered Result shape has a fallible return type");
            statements.extend(self.success_return(&fallible, value, Some(shape)));
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
        if let Some(CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        })) = lowered
            && let Some(source) = self.comma_ok_source(expression)
            && !source.has_nil_guard()
        {
            let pair = self.bind_comma_ok_pair(expression, source, CommaOkValueSlot::Temp);
            let ok = GoExpression::name(pair.status().to_string());
            let value = GoExpression::name(
                pair.value()
                    .expect("Temp slot always captures the value")
                    .to_string(),
            );
            let mut statements = pair.statements;
            statements.push(transition::multi_value_return(vec![value, ok]));
            return statements;
        }
        if let Some(plan) = self.plan_call(expression)
            && !plan.resolved.abi.result.is_passthrough()
        {
            if let Some(shape) = lowered {
                let (setup, result_var) = self
                    .lower_go_abi_wrapped_call(expression, &plan.resolved.abi, return_ty)
                    .into_parts();
                statements.extend(setup);
                statements.extend(transition::emit_lowered_result_return(
                    self,
                    &result_var,
                    return_ty,
                    shape,
                ));
            } else {
                let abi = plan.resolved.abi.clone();
                let unbridged = self.go_tuple_result_bridges(&abi, return_ty).is_none()
                    && self.go_result_layout_bridge(&abi, return_ty).is_none()
                    && self.go_return_payload_bridge(&abi, return_ty).is_none();
                if unbridged {
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
            let temp =
                GoExpression::name(self.hoist_tmp_value_statement(&mut statements, "v", value));
            statements.extend(transition::emit_lowered_result_return(
                self, &temp, return_ty, shape,
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

    /// True when the callee already has the enclosing shape, so a tail
    /// return can forward without rewrapping.
    fn callee_matches_lowered_shape(
        &self,
        call_expression: &Expression,
        enclosing_shape: &CallableReturnAbi,
    ) -> bool {
        let Some(plan) = self.plan_call(call_expression) else {
            return false;
        };
        plan.resolved.abi.result == *enclosing_shape
    }
}
