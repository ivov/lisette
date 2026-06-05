use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::abi::AbiShape;
use crate::abi::transition;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FalliblePlanner};
use crate::definitions::functions::is_go_never;
use crate::plan::bodies::{
    AssignForm, AssignPlan, LoweredBlock, LoweredStatement, PlacePlan, ReturnForm,
    ReturnStatementPlan,
};
use crate::plan::calls::{CallReturnShape, CalleePlan};
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Clone, Copy)]
struct WrappedReturnInfo<'a> {
    fallible: &'a Fallible,
    return_ty: &'a Type,
    lowered: Option<&'a AbiShape>,
}

pub(crate) fn plain_return(value: String) -> LoweredStatement {
    LoweredStatement::Return(ReturnStatementPlan {
        directive: String::new(),
        form: ReturnForm::Plain {
            value: ValuePlan::Operand(value),
        },
    })
}

fn simple_assign(target: String, value: String) -> LoweredStatement {
    LoweredStatement::Assign(AssignPlan {
        directive: String::new(),
        form: AssignForm::Simple {
            target_capture: Vec::new(),
            target_str: target,
            value: ValuePlan::Operand(value),
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
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let expression_ty = expression.get_type();
        let fallible = Fallible::from_type(&expression_ty)
            .expect("lower_propagate called on non-Result/Option type");

        let mut statements = Vec::new();

        // `Err(...)?` / `None?` literal already emits its own return.
        if let Some(var_name) = result_var_name
            && let Some(head) = self.try_lower_error_constructor(expression, &fallible, fx)
        {
            statements.extend(head);
            self.declare_zero_for_dead_path(&mut statements, var_name, &fallible, fx);
            return (statements, String::new());
        }

        fx.require_stdlib();
        let (check_setup, check_var) = self.hoist_propagate_check_var(expression, fx);
        statements.extend(check_setup);
        statements.push(self.build_propagate_failure_check(&check_var, &fallible, fx));

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

    /// String-context bridge over `lower_propagate`.
    pub(crate) fn emit_propagate(
        &mut self,
        output: &mut String,
        expression: &Expression,
        result_var_name: Option<&str>,
        fx: &mut EmitEffects,
    ) -> String {
        let (statements, value) = self.lower_propagate(expression, result_var_name, fx);
        let block = LoweredBlock { statements };
        Renderer.render_lowered_block(output, &block);
        value
    }

    /// `Err(...)?` and `None?` already emitted `return ...`. Declare the
    /// binding with a zero value so any dead code below that references it
    /// stays well-typed in Go.
    fn declare_zero_for_dead_path(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        var_name: &str,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) {
        if var_name == "_" {
            return;
        }
        let inner_ty = fallible.ok_ty();
        let (zero, effects) = self.zero_value(inner_ty);
        fx.extend(&effects);
        if self.is_declared(var_name) {
            statements.push(simple_assign(var_name.to_string(), zero));
        } else {
            // Kept as `RawGo`: a `var name ty = value` declaration reuse (`ConstPlan`)
            // would drop `name`/`ty` from `references_var`, flipping queries on
            // this dead-path binding.
            let go_ty = self.go_type_string(inner_ty, fx);
            statements.push(LoweredStatement::RawGo(format!(
                "var {} {} = {}\n",
                var_name, go_ty, zero
            )));
            self.declare(var_name);
        }
    }

    fn hoist_propagate_check_var(
        &mut self,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        if let Expression::Identifier { value, ty, .. } = expression {
            let go_name = self.emit_identifier(value, ty, ExpressionContext::value(), fx);
            if go_name.contains('(') {
                let mut setup = Vec::new();
                let check = self.hoist_tmp_value_statement(&mut setup, "check", &go_name);
                (setup, check)
            } else {
                (Vec::new(), go_name)
            }
        } else {
            let staged = self.stage_operand(expression, ExpressionContext::value(), fx);
            let mut setup = staged.setup;
            let check = self.hoist_tmp_value_statement(&mut setup, "check", &staged.value);
            (setup, check)
        }
    }

    /// The `if check.Tag != <success> { return <failure> }` failure guard.
    fn build_propagate_failure_check(
        &mut self,
        check_var: &str,
        fallible: &Fallible,
        fx: &mut EmitEffects,
    ) -> LoweredStatement {
        let err_field = if fallible.is_result() { ".ErrVal" } else { "" };
        let success_tag = fallible.success_tag();
        let return_ctx = self.return_ctx();

        let values = if let Some(shape) = return_ctx.lowered_shape() {
            let return_ty = return_ctx.expect_ty();
            // Option propagation: failure carries no payload, so return a
            // shape-specific `None` rather than an err-return.
            if fallible.is_result() {
                let err_expr = format!("{}{}", check_var, err_field);
                transition::lowered_err_values(self, &shape, &return_ty, &err_expr, fx)
            } else {
                transition::lowered_none_values(self, &shape, &return_ty, fx)
            }
        } else {
            let err_return = {
                let mut fe = FalliblePlanner::new(self, fallible, fx);
                fe.emit_contextual_failure(Some(&format!("{}{}", check_var, err_field)))
            };
            vec![err_return]
        };

        transition::tag_check(format!("{}.Tag != {}", check_var, success_tag), values)
    }

    /// Statement-position `inner?` (discards the ok value).
    pub(crate) fn lower_propagate_statement(
        &mut self,
        inner: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        self.lower_propagate(inner, Some("_"), fx).0
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

    pub(crate) fn emit_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) {
        let return_ctx = self.return_ctx();
        let is_unit = return_ctx.ty().is_some_and(Type::is_unit);

        if is_unit {
            let is_pure = matches!(
                expression,
                Expression::Unit { .. }
                    | Expression::Identifier { .. }
                    | Expression::Literal { .. }
            );
            if !is_pure {
                self.emit_statement(output, expression, fx);
            }
            output.push_str("return\n");
        } else if !transition::render_lowered_tail_return(self, output, expression, fx)
            && !self.emit_wrapped_return(output, expression, fx)
        {
            let expression_string =
                self.emit_value(output, expression, ExpressionContext::value(), fx);
            let return_ty = return_ctx.ty();
            let expression_string =
                self.apply_type_coercion(output, return_ty, expression, expression_string, fx);
            write_line!(output, "return {}", expression_string);
        }
    }

    /// Build a `ReturnStatementPlan`, dispatching on return shape.
    pub(crate) fn build_return_plan(
        &mut self,
        expression: &Expression,
        directive: String,
        fx: &mut EmitEffects,
    ) -> ReturnStatementPlan {
        let body_block = |body_text: String| LoweredBlock {
            statements: vec![LoweredStatement::RawGo(body_text)],
        };

        let return_ctx = self.return_ctx();
        let is_unit = return_ctx.ty().is_some_and(Type::is_unit);
        if is_unit {
            // Mirror emit_return's unit path: impure expressions run as a
            // statement before the bare `return`; pure ones (Unit, Identifier,
            // Literal) emit nothing.
            let is_pure = matches!(
                expression,
                Expression::Unit { .. }
                    | Expression::Identifier { .. }
                    | Expression::Literal { .. }
            );
            let side_effect = if is_pure {
                None
            } else {
                let mut buffer = String::new();
                self.emit_statement(&mut buffer, expression, fx);
                (!buffer.is_empty()).then(|| body_block(buffer))
            };
            return ReturnStatementPlan {
                directive,
                form: ReturnForm::Unit { side_effect },
            };
        }

        if let Some(statements) = transition::try_emit_lowered_tail_return(self, expression, fx) {
            return ReturnStatementPlan {
                directive,
                form: ReturnForm::LoweredAbi {
                    body: LoweredBlock { statements },
                },
            };
        }

        if let Some(statements) = self.lower_wrapped_return(expression, fx) {
            return ReturnStatementPlan {
                directive,
                form: ReturnForm::Wrapped {
                    body: LoweredBlock { statements },
                },
            };
        }

        let (mut setup, raw_value) = self.lower_value(expression, ExpressionContext::value(), fx);
        let mut coercion_buffer = String::new();
        let final_value = self.apply_type_coercion(
            &mut coercion_buffer,
            return_ctx.ty(),
            expression,
            raw_value,
            fx,
        );
        if !coercion_buffer.is_empty() {
            setup.push(LoweredStatement::RawGo(coercion_buffer));
        }
        ReturnStatementPlan {
            directive,
            form: ReturnForm::Plain {
                value: value_plan_from_statements(setup, final_value),
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
        fx: &mut EmitEffects,
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
            let (setup, call_str) =
                self.lower_call(expression, None, ExpressionContext::value(), fx);
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
                fx,
            ));
            return Some(statements);
        }

        let info = WrappedReturnInfo {
            fallible: &fallible,
            return_ty: &return_ty,
            lowered: lowered.as_ref(),
        };

        if matches!(expression, Expression::Call { .. }) {
            statements.extend(self.lower_wrapped_call_return(expression, info, fx));
            return Some(statements);
        }

        if matches!(expression, Expression::If { .. } | Expression::Match { .. }) {
            let block = self.lower_branching_to_block(expression, &PlacePlan::Return, fx);
            statements.extend(block.statements);
            return Some(statements);
        }

        if let Expression::Propagate {
            expression: inner, ..
        } = expression
        {
            let (setup, value) = self.lower_propagate(inner, None, fx);
            statements.extend(setup);
            statements.extend(self.wrapped_value_return(value, &return_ty, lowered.as_ref(), fx));
            return Some(statements);
        }

        let (setup, value) = self.lower_value(expression, ExpressionContext::value(), fx);
        statements.extend(setup);
        statements.extend(self.wrapped_value_return(value, &return_ty, lowered.as_ref(), fx));
        Some(statements)
    }

    /// String-context bridge over `lower_wrapped_return`. Returns `true`
    /// when a wrapped return was emitted.
    pub(crate) fn emit_wrapped_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> bool {
        match self.lower_wrapped_return(expression, fx) {
            Some(statements) => {
                let block = LoweredBlock { statements };
                Renderer.render_lowered_block(output, &block);
                true
            }
            None => false,
        }
    }

    fn wrapped_value_return(
        &mut self,
        value: String,
        return_ty: &Type,
        lowered: Option<&AbiShape>,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let Some(shape) = lowered else {
            return vec![plain_return(value)];
        };
        // The destructure references the value multiple times (`.Tag`,
        // `.OkVal`, `.ErrVal` etc.); hoist to avoid re-evaluating.
        let mut statements = Vec::new();
        let temp = self.hoist_tmp_value_statement(&mut statements, "v", &value);
        statements.extend(transition::emit_lowered_result_return(
            self, &temp, return_ty, shape, fx,
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
        fx: &mut EmitEffects,
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
                self.lower_success_constructor_return(args, fallible, lowered, fx)
            }
            Some(ConstructorKind::Failure) => {
                self.lower_failure_constructor_return(args, fallible, return_ty, lowered, fx)
            }
            None => self.lower_wrapped_passthrough_return(expression, return_ty, lowered, fx),
        }
    }

    fn lower_success_constructor_return(
        &mut self,
        args: &[Expression],
        fallible: &Fallible,
        lowered: Option<&AbiShape>,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered {
            let ok_arg = if matches!(shape, AbiShape::BareError) {
                if !args.is_empty() {
                    let (setup, _) =
                        self.lower_composite_value(&args[0], ExpressionContext::value(), fx);
                    statements.extend(setup);
                }
                String::new()
            } else if args.is_empty() {
                "struct{}{}".to_string()
            } else {
                let (setup, value) =
                    self.lower_composite_value(&args[0], ExpressionContext::value(), fx);
                statements.extend(setup);
                value
            };
            statements.push(transition::multi_value_return(
                transition::lowered_ok_values(shape, &ok_arg),
            ));
        } else {
            let (setup, arg) = self.lower_composite_value(&args[0], ExpressionContext::value(), fx);
            let success = {
                let mut fe = FalliblePlanner::new(self, fallible, fx);
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
        lowered: Option<&AbiShape>,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered {
            if args.is_empty() {
                statements.push(transition::multi_value_return(
                    transition::lowered_none_values(self, shape, return_ty, fx),
                ));
            } else {
                let (setup, err_expr) =
                    self.lower_composite_value(&args[0], ExpressionContext::value(), fx);
                let values = transition::lowered_err_values(self, shape, return_ty, &err_expr, fx);
                statements.extend(setup);
                statements.push(transition::multi_value_return(values));
            }
        } else {
            let failure = if fallible.is_result() {
                let (setup, arg) =
                    self.lower_composite_value(&args[0], ExpressionContext::value(), fx);
                statements.extend(setup);
                let mut fe = FalliblePlanner::new(self, fallible, fx);
                fe.emit_failure(Some(&arg))
            } else {
                let mut fe = FalliblePlanner::new(self, fallible, fx);
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
        lowered: Option<&AbiShape>,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        if let Some(shape) = lowered
            && self.callee_matches_lowered_shape(expression, shape)
        {
            let (setup, call) = self.lower_call(expression, None, ExpressionContext::value(), fx);
            statements.extend(setup);
            statements.push(plain_return(call));
            return statements;
        }
        if let Some(plan) = self.plan_call(expression)
            && let CalleePlan::GoInterop(strategy) = plan.callee
        {
            let mut setup = String::new();
            let result_var =
                self.emit_go_wrapped_call(&mut setup, expression, &strategy, return_ty, fx);
            if !setup.is_empty() {
                statements.push(LoweredStatement::RawGo(setup));
            }
            if let Some(shape) = lowered {
                statements.extend(transition::emit_lowered_result_return(
                    self,
                    &result_var,
                    return_ty,
                    shape,
                    fx,
                ));
            } else {
                statements.push(plain_return(result_var));
            }
            return statements;
        }
        if let Some(shape) = lowered {
            let (setup, value) = self.lower_value(expression, ExpressionContext::value(), fx);
            statements.extend(setup);
            let temp = self.hoist_tmp_value_statement(&mut statements, "v", &value);
            statements.extend(transition::emit_lowered_result_return(
                self, &temp, return_ty, shape, fx,
            ));
            return statements;
        }
        let (setup, call) = self.lower_call(expression, None, ExpressionContext::value(), fx);
        statements.extend(setup);
        statements.push(plain_return(call));
        statements
    }

    /// True when the callee already has the enclosing shape, so a tail
    /// return can forward without rewrapping.
    fn callee_matches_lowered_shape(
        &self,
        call_expression: &Expression,
        enclosing_shape: &AbiShape,
    ) -> bool {
        let Some(plan) = self.plan_call(call_expression) else {
            return false;
        };
        match &plan.callee {
            CalleePlan::GoInterop(strategy) => enclosing_shape.matches_go_strategy(strategy),
            _ => {
                matches!(&plan.return_shape, CallReturnShape::Lowered(shape) if shape == enclosing_shape)
            }
        }
    }
}
