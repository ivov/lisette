use crate::Planner;
use crate::Renderer;
use crate::abi::callable::{CallableReturnAbi, PayloadLayout};
use crate::abi::transition;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FalliblePlanner};
use crate::definitions::functions::is_go_never;
use crate::plan::bodies::{
    AssignForm, AssignPlan, LoweredBlock, LoweredStatement, PlacePlan, ReturnForm,
    ReturnStatementPlan,
};
use crate::plan::calls::CallableOrigin;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Clone, Copy)]
struct WrappedReturnInfo<'a> {
    fallible: &'a Fallible,
    return_ty: &'a Type,
    lowered: Option<&'a CallableReturnAbi>,
}

pub(crate) fn plain_return(value: String) -> LoweredStatement {
    LoweredStatement::Return(ReturnStatementPlan {
        form: ReturnForm::Plain {
            value: ValuePlan::opaque(value),
        },
    })
}

fn simple_assign(target: String, value: String) -> LoweredStatement {
    LoweredStatement::Assign(AssignPlan {
        form: AssignForm::Simple {
            target_capture: Vec::new(),
            target_str: target,
            value: ValuePlan::opaque(value),
        },
    })
}

impl Planner<'_> {
    /// Lower `?` into structured IR plus the ok-access value. `result_var_name`:
    /// `None` returns `check.OkVal`, `Some("_")` discards, `Some(name)` binds
    /// `name := check.OkVal`.
    pub(crate) fn lower_propagate(
        &mut self,
        expression: &Expression,
        result_var_name: Option<&str>,
    ) -> (Vec<LoweredStatement>, String) {
        let expression_ty = expression.get_type();
        let fallible = Fallible::from_type(&expression_ty)
            .expect("lower_propagate called on non-Result/Option type");

        let mut statements = Vec::new();

        // `Err(...)?` / `None?` literal already emits its own return.
        if let Some(var_name) = result_var_name
            && let Some(head) = self.try_lower_error_constructor(expression, &fallible)
        {
            statements.extend(head);
            self.declare_zero_for_dead_path(&mut statements, var_name, &fallible);
            return (statements, String::new());
        }

        if let Some(fused) =
            self.try_lower_fused_go_propagate(expression, &fallible, result_var_name)
        {
            return fused;
        }

        self.require_stdlib();
        let (check_setup, check_var) = self.hoist_propagate_check_var(expression);
        statements.extend(check_setup);
        statements.push(self.build_propagate_failure_check(&check_var, &fallible));

        let ok_access = format!("{}.{}", check_var, fallible.ok_field());
        let value = match result_var_name {
            None => ok_access,
            Some("_") => "_".to_string(),
            Some(name) => {
                statements.push(self.bind_propagate_ok(name, &ok_access));
                name.to_string()
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
        let (zero, packages) = self.zero_value(inner_ty);
        self.require_packages(&packages);
        if self.is_declared(var_name) {
            statements.push(simple_assign(var_name.to_string(), zero));
        } else {
            // Declared so the dead-path binding stays in scope for later references.
            let go_ty = self.go_type_string(inner_ty);
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
    ) -> (Vec<LoweredStatement>, String) {
        let plan = self.stage_operand(expression, ExpressionContext::value());
        let requires_capture = !matches!(expression, Expression::Identifier { .. })
            || plan.expression.contains_deferred_evaluation();
        let (mut setup, value) = plan.into_parts();
        if requires_capture {
            let check = self.hoist_tmp_value_statement(&mut setup, "check", &value);
            (setup, check)
        } else {
            (setup, value)
        }
    }

    /// The `if check.Tag != <success> { return <failure> }` failure guard.
    fn build_propagate_failure_check(
        &mut self,
        check_var: &str,
        fallible: &Fallible,
    ) -> LoweredStatement {
        let err_field = if fallible.is_result() { ".ErrVal" } else { "" };
        let success_tag = fallible.success_tag();
        let err_expr = format!("{}{}", check_var, err_field);
        let values = self.propagate_failure_values(fallible, &err_expr);
        transition::tag_check(format!("{}.Tag != {}", check_var, success_tag), values)
    }

    fn propagate_failure_values(&mut self, fallible: &Fallible, err_expr: &str) -> Vec<String> {
        let return_ctx = self.return_ctx();
        if let Some(shape) = return_ctx.lowered_shape() {
            let return_ty = return_ctx.expect_ty();
            // Option propagation: failure carries no payload, so return a
            // shape-specific `None` rather than an err-return.
            if fallible.is_result() {
                transition::lowered_err_values(self, &shape, &return_ty, err_expr)
            } else {
                transition::lowered_none_values(self, &shape, &return_ty)
            }
        } else {
            let mut fe = FalliblePlanner::new(self, fallible);
            vec![fe.emit_contextual_failure(Some(err_expr))]
        }
    }

    /// Fuse `go_call()?` into `v, err := call(); if err != nil { return ... }`,
    /// skipping the `lisette.Result`.
    fn try_lower_fused_go_propagate(
        &mut self,
        expression: &Expression,
        fallible: &Fallible,
        result_var_name: Option<&str>,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let plan = self.plan_call(expression)?;
        if !matches!(plan.resolved.origin, CallableOrigin::GoInterop)
            || !matches!(
                plan.resolved.abi.result,
                CallableReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                    ..
                }
            )
        {
            return None;
        }
        let ok_ty = self.facts.peel_alias(&expression.get_type()).ok_type();
        if ok_ty.is_unit() || matches!(self.facts.peel_alias(&ok_ty), Type::Tuple(_)) {
            return None;
        }
        if self
            .go_return_payload_bridge(&plan.resolved.abi, &expression.get_type())
            .is_some()
        {
            return None;
        }
        let return_ctx = self.return_ctx();
        let has_fallible_return = return_ctx.lowered_shape().is_some()
            || return_ctx
                .ty()
                .is_some_and(|ty| Fallible::from_type(ty).is_some());
        if !has_fallible_return {
            return None;
        }
        let nil_guard = self.result_nil_guard(&ok_ty);

        let want_value = !matches!(result_var_name, Some("_"));
        let val_var = (want_value || nil_guard.is_some()).then(|| {
            let v = self.fresh_var(Some("ret"));
            self.declare(&v);
            v
        });
        let err_var = self.fresh_var(Some("ret"));
        self.declare(&err_var);

        let (mut statements, call_str) = self
            .lower_call(expression, None, ExpressionContext::value())
            .into_parts();
        let bind_line = match &val_var {
            Some(v) => format!("{}, {} := {}\n", v, err_var, call_str),
            None => format!("_, {} := {}\n", err_var, call_str),
        };
        statements.push(LoweredStatement::RawGo(bind_line));

        let failure_values = self.propagate_failure_values(fallible, &err_var);
        statements.push(transition::tag_check(
            format!("{} != nil", err_var),
            failure_values,
        ));

        if let Some(guard) = nil_guard {
            let val = val_var
                .as_deref()
                .expect("nil guard requires the value var");
            if guard.is_interface() {
                self.require_stdlib();
            }
            self.require_errors();
            let nil_failure =
                self.propagate_failure_values(fallible, "errors.New(\"unexpected nil\")");
            statements.push(transition::tag_check(guard.is_nil(val), nil_failure));
        }

        let value = match result_var_name {
            None => val_var.expect("ok value requested when result_var_name is None"),
            Some("_") => "_".to_string(),
            Some(name) => {
                let v = val_var.expect("ok value requested for a named binding");
                statements.push(self.bind_propagate_ok(name, &v));
                name.to_string()
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

    fn bind_propagate_ok(&mut self, name: &str, ok_access: &str) -> LoweredStatement {
        if self.is_declared(name) {
            simple_assign(name.to_string(), ok_access.to_string())
        } else {
            self.declare(name);
            LoweredStatement::TempBind {
                name: name.to_string(),
                value: ok_access.to_string(),
            }
        }
    }

    /// Build a `ReturnStatementPlan`, dispatching on return shape.
    pub(crate) fn build_return_plan(&mut self, expression: &Expression) -> ReturnStatementPlan {
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
            let side_effect = if is_pure {
                None
            } else {
                let body = LoweredBlock {
                    statements: vec![self.lower_statement(expression)],
                };
                (!Renderer.renders_empty(&body)).then_some(body)
            };
            return ReturnStatementPlan {
                form: ReturnForm::Unit { side_effect },
            };
        }

        if let Some(statements) = transition::try_emit_lowered_tail_return(self, expression) {
            return ReturnStatementPlan {
                form: ReturnForm::LoweredAbi {
                    body: LoweredBlock { statements },
                },
            };
        }

        if let Some(statements) = self.lower_wrapped_return(expression) {
            return ReturnStatementPlan {
                form: ReturnForm::Wrapped {
                    body: LoweredBlock { statements },
                },
            };
        }

        let plan = self.lower_value(expression, ExpressionContext::value());
        ReturnStatementPlan {
            form: ReturnForm::Plain {
                value: plan.map_rendered_as_computed(
                    |setup, raw_value, contains_deferred_evaluation| {
                        let mut coercion_buffer = String::new();
                        let final_value = self.apply_type_coercion(
                            &mut coercion_buffer,
                            return_ctx.ty(),
                            expression,
                            raw_value,
                        );
                        if !coercion_buffer.is_empty() {
                            setup.push(LoweredStatement::RawGo(coercion_buffer));
                        }
                        GoExpression::opaque_with_deferred_evaluation(
                            final_value,
                            contains_deferred_evaluation,
                        )
                    },
                ),
            },
        }
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
        let expression_ty = expression.get_type();
        let return_ctx = self.return_ctx();

        let return_ty = return_ctx
            .ty()
            .filter(|ty| Fallible::from_type(ty).is_some())
            .cloned()
            .unwrap_or(expression_ty);

        let fallible = Fallible::from_type(&return_ty)?;

        let mut statements = Vec::new();

        if is_go_never(expression) {
            let (setup, call_str) = self
                .lower_call(expression, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            // Kept as `RawGo`: this is a Go-never call (`panic(...)`) whose
            // `ends_with_diverge` must stay true; `ExpressionStatementForm::Async`
            // reports false.
            statements.push(LoweredStatement::RawGo(format!("{}\n", call_str)));
            return Some(statements);
        }

        let lowered = return_ctx.lowered_shape();

        if let Expression::Identifier { .. } = expression
            && fallible.classify_constructor(expression) == Some(ConstructorKind::Failure)
        {
            // Only `None` reaches here. `Err` always has a payload, so an
            // identifier failure constructor must be a payload-less Option.
            statements.extend(self.lower_failure_constructor_return(
                &[],
                &fallible,
                &return_ty,
                lowered.as_ref(),
            ));
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
        value: String,
        return_ty: &Type,
        lowered: Option<&CallableReturnAbi>,
    ) -> Vec<LoweredStatement> {
        let Some(shape) = lowered else {
            return vec![plain_return(value)];
        };
        // The destructure references the value multiple times (`.Tag`,
        // `.OkVal`, `.ErrVal` etc.); hoist to avoid re-evaluating.
        let mut statements = Vec::new();
        let temp = self.hoist_tmp_value_statement(&mut statements, "v", &value);
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
        match fallible.classify_constructor(call_expression) {
            Some(ConstructorKind::Success) => {
                self.lower_success_constructor_return(args, fallible, lowered)
            }
            Some(ConstructorKind::Failure) => {
                self.lower_failure_constructor_return(args, fallible, return_ty, lowered)
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
        if let Some(shape) = lowered {
            let ok_arg = if matches!(
                shape,
                CallableReturnAbi::Result {
                    bare_error: true,
                    ..
                }
            ) {
                if !args.is_empty() {
                    let (setup, _) = self
                        .lower_composite_value(&args[0], ExpressionContext::value())
                        .into_parts();
                    statements.extend(setup);
                }
                String::new()
            } else if args.is_empty() {
                "struct{}{}".to_string()
            } else {
                let (setup, value) = self
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                value
            };
            statements.push(transition::multi_value_return(
                transition::lowered_ok_values(shape, &ok_arg),
            ));
        } else {
            let (setup, arg) = self
                .lower_composite_value(&args[0], ExpressionContext::value())
                .into_parts();
            let success = {
                let mut fe = FalliblePlanner::new(self, fallible);
                fe.emit_success(&arg)
            };
            statements.extend(setup);
            statements.push(plain_return(success));
        }
        statements
    }

    fn lower_failure_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
        return_ty: &Type,
        lowered: Option<&CallableReturnAbi>,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered {
            if args.is_empty() {
                statements.push(transition::multi_value_return(
                    transition::lowered_none_values(self, shape, return_ty),
                ));
            } else {
                let (setup, err_expr) = self
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                let values = transition::lowered_err_values(self, shape, return_ty, &err_expr);
                statements.extend(setup);
                statements.push(transition::multi_value_return(values));
            }
        } else {
            let failure = if fallible.is_result() {
                let (setup, arg) = self
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                statements.extend(setup);
                let mut fe = FalliblePlanner::new(self, fallible);
                fe.emit_failure(Some(&arg))
            } else {
                let mut fe = FalliblePlanner::new(self, fallible);
                fe.emit_failure(None)
            };
            statements.push(plain_return(failure));
        }
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
            && self.callee_matches_lowered_shape(expression, shape)
        {
            let (setup, call) = self
                .lower_call(expression, None, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            statements.push(plain_return(call));
            return statements;
        }
        if let Some(plan) = self.plan_call(expression)
            && !plan.resolved.abi.result.is_passthrough()
        {
            let (setup, result_var) = self
                .lower_go_abi_wrapped_call(expression, &plan.resolved.abi, return_ty)
                .into_parts();
            statements.extend(setup);
            if let Some(shape) = lowered {
                statements.extend(transition::emit_lowered_result_return(
                    self,
                    &result_var,
                    return_ty,
                    shape,
                ));
            } else {
                statements.push(plain_return(result_var));
            }
            return statements;
        }
        if let Some(shape) = lowered {
            let (setup, value) = self
                .lower_value(expression, ExpressionContext::value())
                .into_parts();
            statements.extend(setup);
            let temp = self.hoist_tmp_value_statement(&mut statements, "v", &value);
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
