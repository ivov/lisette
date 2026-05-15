use crate::Emitter;
use crate::ReturnContext;
use crate::control_flow::fallible::{ConstructorKind, Fallible, FallibleEmitter};
use crate::expressions::context::ExpressionContext;
use crate::placement::BodyPlace;
use crate::types::abi::AbiShape;
use crate::types::abi_transition;
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Clone, Copy)]
struct WrappedReturnInfo<'a> {
    fallible: &'a Fallible,
    return_ty: &'a Type,
    lowered: Option<&'a AbiShape>,
    return_ctx: &'a ReturnContext,
}

impl Emitter<'_> {
    pub(crate) fn emit_propagate(
        &mut self,
        output: &mut String,
        expression: &Expression,
        result_var_name: Option<&str>,
        return_ctx: &ReturnContext,
    ) -> String {
        let expression_ty = expression.get_type();
        let fallible = Fallible::from_type(&expression_ty)
            .expect("emit_propagate called on non-Result/Option type");

        if let Some(var_name) = result_var_name
            && let Some(result) =
                self.try_emit_error_constructor(output, expression, &fallible, return_ctx)
        {
            // Direct failure constructor (e.g. Err(...)? or None?) already emitted
            // `return ...`. Declare the binding variable so any dead code after
            // this point that references it doesn't produce "undefined" in Go.
            if var_name != "_" {
                let inner_ty = fallible.ok_ty();
                let (zero, effects) = self.zero_value(inner_ty);
                self.requirements.apply_effects(&effects);
                if self.is_declared(var_name) {
                    write_line!(output, "{} = {}", var_name, zero);
                } else {
                    let go_ty = self.go_type_as_string(inner_ty);
                    write_line!(output, "var {} {} = {}", var_name, go_ty, zero);
                    self.declare(var_name);
                }
            }
            return result;
        }

        self.requirements.require_stdlib();
        let check_var = if let Expression::Identifier { value, ty, .. } = expression {
            let go_name = self.emit_identifier(value, ty, ExpressionContext::value());
            if go_name.contains('(') {
                self.hoist_tmp_value(output, "check", &go_name)
            } else {
                go_name
            }
        } else {
            let expression_string =
                self.emit_operand(output, expression, ExpressionContext::value());
            self.hoist_tmp_value(output, "check", &expression_string)
        };

        let (result_var, result_var_pre_declared) = match result_var_name {
            Some(name) => (name.to_string(), self.is_declared(name)),
            None => {
                let v = self.fresh_var(Some("result"));
                self.declare(&v);
                (v, false)
            }
        };

        let err_field = if fallible.is_result() { ".ErrVal" } else { "" };

        if let Some(shape) = return_ctx.lowered_shape() {
            let return_ty = return_ctx.expect_ty();
            // Option propagation: failure carries no payload, so emit a
            // shape-specific `None` return rather than an err-return.
            let lowered_failure = if fallible.is_result() {
                let err_expr = format!("{}{}", check_var, err_field);
                abi_transition::format_lowered_err_return(self, &shape, &return_ty, &err_expr)
            } else {
                abi_transition::format_lowered_none_return(self, &shape, &return_ty)
            };
            write_line!(
                output,
                "if {}.Tag != {} {{\n{}\n}}",
                check_var,
                fallible.success_tag(),
                lowered_failure
            );
        } else {
            let err_return = {
                let mut fe = FallibleEmitter::new(self, &fallible);
                fe.emit_contextual_failure(Some(&format!("{}{}", check_var, err_field)), return_ctx)
            };
            write_line!(
                output,
                "if {}.Tag != {} {{\nreturn {}\n}}",
                check_var,
                fallible.success_tag(),
                err_return
            );
        }

        if result_var != "_" {
            let op = if result_var_pre_declared { "=" } else { ":=" };
            write_line!(
                output,
                "{} {} {}.{}",
                result_var,
                op,
                check_var,
                fallible.ok_field()
            );
        }

        result_var
    }
    pub(crate) fn emit_propagate_to_let(
        &mut self,
        output: &mut String,
        var_name: &str,
        expression: &Expression,
        return_ctx: &ReturnContext,
    ) {
        let Expression::Propagate { expression, .. } = expression else {
            return;
        };
        self.emit_propagate(output, expression, Some(var_name), return_ctx);
    }

    pub(crate) fn emit_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        return_ctx: &ReturnContext,
    ) {
        let is_unit = return_ctx.ty().is_some_and(Type::is_unit);

        if is_unit {
            let is_pure = matches!(
                expression,
                Expression::Unit { .. }
                    | Expression::Identifier { .. }
                    | Expression::Literal { .. }
            );
            if !is_pure {
                self.emit_statement(output, expression);
            }
            output.push_str("return\n");
        } else if !abi_transition::try_emit_lowered_tail_return(
            self, output, expression, return_ctx,
        ) && !self.emit_wrapped_return(output, expression, return_ctx)
        {
            let expression_string = self.emit_value(output, expression, ExpressionContext::value());
            let return_ty = return_ctx.ty();
            let expression_string =
                self.apply_type_coercion(output, return_ty, expression, expression_string);
            write_line!(output, "return {}", expression_string);
        }
    }

    /// Emit a return statement with Result/Option wrapping if applicable.
    ///
    /// Returns `false` only when the return type is NOT Result/Option (i.e., Fallible::from_type
    /// returns None). Once a Result/Option return type is identified, this function is exhaustive:
    /// all code paths emit the return and return `true`. The non-Result/Option case is handled
    /// by the caller emitting a plain return.
    pub(crate) fn emit_wrapped_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        return_ctx: &ReturnContext,
    ) -> bool {
        let expression_ty = expression.get_type();

        let return_ty = return_ctx
            .ty()
            .filter(|ty| Fallible::from_type(ty).is_some())
            .cloned()
            .unwrap_or(expression_ty);

        let Some(fallible) = Fallible::from_type(&return_ty) else {
            return false;
        };

        if Self::is_go_never(expression) {
            let call_str = self.emit_call(output, expression, None, ExpressionContext::value());
            write_line!(output, "{}", call_str);
            return true;
        }

        let lowered = return_ctx.lowered_shape();

        if let Expression::Identifier { .. } = expression
            && fallible.classify_constructor(expression) == Some(ConstructorKind::Failure)
        {
            // Only `None` reaches here — `Err` always has a payload.
            if let Some(shape) = lowered.as_ref() {
                let line = abi_transition::format_lowered_none_return(self, shape, &return_ty);
                write_line!(output, "{}", line);
            } else {
                self.requirements.require_stdlib();
                let mut fe = FallibleEmitter::new(self, &fallible);
                let failure = fe.emit_failure(None);
                write_line!(output, "return {}", failure);
            }
            return true;
        }

        let info = WrappedReturnInfo {
            fallible: &fallible,
            return_ty: &return_ty,
            lowered: lowered.as_ref(),
            return_ctx,
        };

        if matches!(expression, Expression::Call { .. }) {
            self.emit_wrapped_call_return(output, expression, info);
            return true;
        }

        if matches!(expression, Expression::If { .. } | Expression::Match { .. }) {
            self.emit_wrapped_branching_return(output, expression, info);
            return true;
        }

        let value = self.emit_value(output, expression, ExpressionContext::value());
        if let Some(shape) = lowered {
            // The destructure references the value multiple times (`.Tag`,
            // `.OkVal`, `.ErrVal` etc.); hoist to avoid re-evaluating.
            let temp = self.hoist_tmp_value(output, "v", &value);
            abi_transition::emit_lowered_result_return(self, output, &temp, &return_ty, &shape);
        } else {
            write_line!(output, "return {}", value);
        }
        true
    }

    /// Emit a return for a call whose result is wrapped in the function's
    /// Result/Option return type. Success/Failure constructors collapse
    /// directly; other calls emit normally and return the call expression.
    fn emit_wrapped_call_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        info: WrappedReturnInfo<'_>,
    ) {
        let WrappedReturnInfo {
            fallible,
            return_ty,
            lowered,
            return_ctx: _,
        } = info;
        let Expression::Call {
            expression: call_expression,
            args,
            ..
        } = expression
        else {
            unreachable!("emit_wrapped_call_return requires a Call expression");
        };
        match fallible.classify_constructor(call_expression) {
            Some(ConstructorKind::Success) => {
                if let Some(shape) = lowered {
                    let ok_arg = if matches!(shape, AbiShape::BareError) {
                        // Unit Ok — emit args[0] for side effects, then drop.
                        if !args.is_empty() {
                            let _ = self.emit_composite_value(
                                output,
                                &args[0],
                                ExpressionContext::value(),
                            );
                        }
                        String::new()
                    } else if args.is_empty() {
                        // `Some` with no payload wouldn't typecheck; only
                        // possible when Ok type is unit and we still need a
                        // value for the tuple (`Some(())` under CommaOk).
                        "struct{}{}".to_string()
                    } else {
                        self.emit_composite_value(output, &args[0], ExpressionContext::value())
                    };
                    let line = abi_transition::format_lowered_ok_return(shape, &ok_arg);
                    write_line!(output, "{}", line);
                } else {
                    let arg =
                        self.emit_composite_value(output, &args[0], ExpressionContext::value());
                    let mut fe = FallibleEmitter::new(self, fallible);
                    let success = fe.emit_success(&arg);
                    write_line!(output, "return {}", success);
                }
            }
            Some(ConstructorKind::Failure) => {
                if let Some(shape) = lowered {
                    if args.is_empty() {
                        // `None` under lowered Option (CommaOk/NullableReturn).
                        let line =
                            abi_transition::format_lowered_none_return(self, shape, return_ty);
                        write_line!(output, "{}", line);
                    } else {
                        let err_expr =
                            self.emit_composite_value(output, &args[0], ExpressionContext::value());
                        let line = abi_transition::format_lowered_err_return(
                            self, shape, return_ty, &err_expr,
                        );
                        write_line!(output, "{}", line);
                    }
                } else {
                    let failure = if fallible.is_result() {
                        let arg =
                            self.emit_composite_value(output, &args[0], ExpressionContext::value());
                        let mut fe = FallibleEmitter::new(self, fallible);
                        fe.emit_failure(Some(&arg))
                    } else {
                        let mut fe = FallibleEmitter::new(self, fallible);
                        fe.emit_failure(None)
                    };
                    write_line!(output, "return {}", failure);
                }
            }
            None => self.emit_wrapped_passthrough_return(
                output,
                expression,
                call_expression,
                return_ty,
                lowered,
            ),
        }
    }

    /// Tail return for a non-constructor call.
    fn emit_wrapped_passthrough_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        call_expression: &Expression,
        return_ty: &Type,
        lowered: Option<&AbiShape>,
    ) {
        if let Some(shape) = lowered
            && self.callee_matches_lowered_shape(call_expression, shape)
        {
            let call = self.emit_call(output, expression, None, ExpressionContext::value());
            write_line!(output, "return {}", call);
            return;
        }
        if let Some(strategy) = self.resolve_go_call_strategy(expression) {
            let result_var = self.emit_go_wrapped_call(output, expression, &strategy, return_ty);
            if let Some(shape) = lowered {
                abi_transition::emit_lowered_result_return(
                    self,
                    output,
                    &result_var,
                    return_ty,
                    shape,
                );
            } else {
                write_line!(output, "return {}", result_var);
            }
            return;
        }
        if let Some(shape) = lowered {
            let value = self.emit_value(output, expression, ExpressionContext::value());
            let temp = self.hoist_tmp_value(output, "v", &value);
            abi_transition::emit_lowered_result_return(self, output, &temp, return_ty, shape);
            return;
        }
        let call = self.emit_call(output, expression, None, ExpressionContext::value());
        write_line!(output, "return {}", call);
    }

    /// True when the callee's natural multi-return matches the enclosing
    /// shape, so a tail return can forward without rewrapping.
    fn callee_matches_lowered_shape(
        &self,
        callee: &Expression,
        enclosing_shape: &AbiShape,
    ) -> bool {
        let inner = callee.unwrap_parens();
        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
            && Self::is_go_receiver(receiver)
        {
            let callee_ty = callee.get_type();
            if let Type::Function { return_type, .. } = callee_ty.unwrap_forall()
                && let Some(strategy) = self.facts.classify_go_return_type(return_type, &[])
            {
                return enclosing_shape.matches_go_strategy(&strategy);
            }
        }
        if let Some(callee_shape) = self.classify_callee_abi(callee) {
            return callee_shape == *enclosing_shape;
        }
        false
    }

    /// Lowered ABI pushes the return into each branch leaf; tagged ABI
    /// builds a temp and returns it.
    fn emit_wrapped_branching_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        info: WrappedReturnInfo<'_>,
    ) {
        let WrappedReturnInfo {
            fallible,
            return_ty,
            lowered,
            return_ctx,
        } = info;
        if lowered.is_some() {
            self.emit_branching_directly(output, expression, &BodyPlace::Return(return_ctx));
            return;
        }

        let temp_var = self.fresh_var(None);
        self.declare(&temp_var);
        let full_ty = {
            let mut fe = FallibleEmitter::new(self, fallible);
            fe.full_type_string()
        };

        write_line!(output, "var {} {}", temp_var, full_ty);

        self.emit_branching_directly(
            output,
            expression,
            &BodyPlace::Assign {
                var: temp_var.clone(),
                target_ty: Some(return_ty.clone()),
            },
        );

        write_line!(output, "return {}", temp_var);
    }

    pub(crate) fn emit_try_block(
        &mut self,
        output: &mut String,
        items: &[Expression],
        ty: &Type,
    ) -> String {
        self.requirements.require_stdlib();

        let effective_ty = self.resolve_fallible_block_type(items, ty);
        let fallible = Fallible::from_type(&effective_ty)
            .expect("`try` block must have Result or Option type");

        let result_var = self.fresh_var(Some("tryResult"));
        self.declare(&result_var);
        let full_ty = {
            let mut fe = FallibleEmitter::new(self, &fallible);
            fe.full_type_string()
        };

        write_line!(output, "{} := func() {} {{", result_var, full_ty);

        self.with_scope_return_context_fallback(ReturnContext::TaggedBlock(effective_ty), |this| {
            this.with_fresh_scope(|emitter| {
                emitter.emit_try_body(output, items, &fallible);
            });
        });

        output.push_str("}()\n");

        result_var
    }

    /// Prefer the function's return context type when the block's own ok_ty
    /// is a type variable (e.g. `Result[any, ...]` when tail is a statement),
    /// or when the tail is Never-typed (ok_ty resolves to unit/Never because
    /// nothing constrains it).
    fn resolve_fallible_block_type(&self, items: &[Expression], ty: &Type) -> Type {
        let tail_is_never = items.last().is_some_and(|last| {
            let t = last.get_type();
            t.is_never() || last.diverges().is_some()
        });
        let base = Fallible::from_type(ty);
        let needs_return_context = tail_is_never
            || base
                .as_ref()
                .is_some_and(|f| f.ok_ty().is_variable() || f.ok_ty().is_never());
        if !needs_return_context {
            return ty.clone();
        }
        self.scope_return_context_fallback()
            .ty()
            .filter(|ty| Fallible::from_type(ty).is_some())
            .cloned()
            .unwrap_or_else(|| ty.clone())
    }

    fn emit_try_body(&mut self, output: &mut String, items: &[Expression], fallible: &Fallible) {
        let Some((last, rest)) = items.split_last() else {
            self.emit_try_unit_return(output, fallible);
            return;
        };
        for item in rest {
            self.emit_statement(output, item);
        }
        self.emit_to_place(
            output,
            last,
            crate::placement::ValuePlace::FallibleSuccess(fallible),
        );
    }

    pub(crate) fn emit_try_unit_return(&mut self, output: &mut String, fallible: &Fallible) {
        let (unit_val, effects) = self.zero_value(fallible.ok_ty());
        self.requirements.apply_effects(&effects);
        self.emit_try_success_return(output, &unit_val, fallible);
    }

    pub(crate) fn emit_try_success_return(
        &mut self,
        output: &mut String,
        value: &str,
        fallible: &Fallible,
    ) {
        let ok_return = {
            let mut fe = FallibleEmitter::new(self, fallible);
            fe.emit_success(value)
        };
        write_line!(output, "return {}", ok_return);
    }

    /// Optimizes `Err(...)?)` and `None?` by emitting a direct return.
    /// Returns `Some(String::new())` if handled, `None` otherwise.
    fn try_emit_error_constructor(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fallible: &Fallible,
        return_ctx: &ReturnContext,
    ) -> Option<String> {
        let err_arg = match expression {
            Expression::Call {
                expression: func,
                args,
                ..
            } => {
                if fallible.classify_constructor(func) != Some(ConstructorKind::Failure) {
                    return None;
                }
                if !args.is_empty() {
                    Some(self.emit_value(output, &args[0], ExpressionContext::value()))
                } else {
                    Some(String::new())
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(expression) != Some(ConstructorKind::Failure) {
                    return None;
                }
                Some(String::new())
            }
            _ => return None,
        };

        self.requirements.require_stdlib();
        let err_return = {
            let mut fe = FallibleEmitter::new(self, fallible);
            fe.emit_contextual_failure(err_arg.as_deref(), return_ctx)
        };

        write_line!(output, "return {}", err_return);
        Some(String::new())
    }

    pub(crate) fn emit_recover_block(
        &mut self,
        output: &mut String,
        items: &[Expression],
        ty: &Type,
    ) -> String {
        self.requirements.require_stdlib();

        let effective_ty = self.resolve_fallible_block_type(items, ty);
        let fallible = Fallible::from_type(&effective_ty)
            .expect("recover block type must be Result<T, PanicValue>");

        let result_var = self.fresh_var(Some("recoverResult"));
        self.declare(&result_var);
        let inner_ty_str = self.go_type_as_string(fallible.ok_ty());

        write_line!(
            output,
            "{} := lisette.RecoverBlock(func() {} {{",
            result_var,
            inner_ty_str
        );

        let body_return_ctx = self.return_context_for_type(fallible.ok_ty().clone());
        self.with_scope_return_context_fallback(body_return_ctx, |this| {
            this.with_fresh_scope(|emitter| {
                emitter.emit_recover_body(output, items, &fallible);
            });
        });

        output.push_str("})\n");
        result_var
    }

    fn emit_recover_body(
        &mut self,
        output: &mut String,
        items: &[Expression],
        fallible: &Fallible,
    ) {
        let Some((last, rest)) = items.split_last() else {
            self.emit_zero_return(output, fallible.ok_ty());
            return;
        };
        for item in rest {
            self.emit_statement(output, item);
        }
        self.emit_to_place(
            output,
            last,
            crate::placement::ValuePlace::RecoverSuccess(fallible),
        );
    }
}
