use crate::Emitter;
use crate::go::control_flow::fallible::{ConstructorKind, Fallible, FallibleEmitter};
use crate::go::types::emitter::Position;
use crate::go::utils::{inline_trivial_bindings, optimize_region};
use crate::go::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_propagate(
        &mut self,
        output: &mut String,
        expression: &Expression,
        result_var_name: Option<&str>,
    ) -> String {
        let expression_ty = expression.get_type().resolve();
        let fallible = Fallible::from_type(&expression_ty)
            .expect("emit_propagate called on non-Result/Option type");

        if let Some(var_name) = result_var_name
            && let Some(result) = self.try_emit_error_constructor(output, expression, &fallible)
        {
            // Direct failure constructor (e.g. Err(...)? or None?) already emitted
            // `return ...`. Declare the binding variable so any dead code after
            // this point that references it doesn't produce "undefined" in Go.
            if var_name != "_" {
                let inner_ty = fallible.ok_ty();
                let zero = self.zero_value(inner_ty);
                let go_ty = self.go_type_as_string(inner_ty);
                write_line!(output, "var {} {} = {}", var_name, go_ty, zero);
                self.declare(var_name);
            }
            return result;
        }

        self.flags.needs_stdlib = true;
        let check_var = if let Expression::Identifier { value, ty, .. } = expression {
            let go_name = self.emit_identifier(value, ty);
            if !go_name.contains('(') {
                go_name
            } else {
                let check_var = self.fresh_var(Some("check"));
                self.declare(&check_var);
                write_line!(output, "{} := {}", check_var, go_name);
                check_var
            }
        } else {
            let check_var = self.fresh_var(Some("check"));
            self.declare(&check_var);
            let expression_string = self.emit_operand(output, expression);
            write_line!(output, "{} := {}", check_var, expression_string);
            check_var
        };

        let result_var = result_var_name.map(|s| s.to_string()).unwrap_or_else(|| {
            let v = self.fresh_var(Some("result"));
            self.declare(&v);
            v
        });

        let err_field = if fallible.is_result() { ".ErrVal" } else { "" };
        let err_return = {
            let mut fe = FallibleEmitter::new(self, &fallible);
            fe.emit_contextual_failure(Some(&format!("{}{}", check_var, err_field)))
        };
        write_line!(
            output,
            "if {}.Tag != {} {{\nreturn {}\n}}",
            check_var,
            fallible.success_tag(),
            err_return
        );

        if result_var != "_" {
            write_line!(
                output,
                "{} := {}.{}",
                result_var,
                check_var,
                fallible.ok_field()
            );
        }

        result_var
    }

    pub(crate) fn emit_option_result_assignment(
        &mut self,
        output: &mut String,
        target_var: &str,
        expression: &Expression,
    ) {
        let target_ty = self.assign_target_ty.as_ref().map(|t| t.resolve());
        let ty = target_ty
            .filter(|t| t.is_option() || t.is_result())
            .unwrap_or_else(|| expression.get_type());
        let Some(fallible) = Fallible::from_type(&ty) else {
            let expression_string = self.emit_operand(output, expression);
            write_line!(output, "{} = {}", target_var, expression_string);
            return;
        };

        let actual_expression = if let Expression::Block { items, .. } = expression {
            if items.len() == 1 {
                &items[0]
            } else {
                expression
            }
        } else {
            expression
        };

        match actual_expression {
            Expression::Call {
                expression: callee,
                args,
                ..
            } => {
                let kind = fallible.classify_constructor(callee);

                let constructor_name = match kind {
                    Some(ConstructorKind::Success) => fallible.ok_constructor(),
                    Some(ConstructorKind::Failure) => fallible.err_constructor(),
                    None => {
                        let expression_string = self.emit_operand(output, expression);
                        write_line!(output, "{} = {}", target_var, expression_string);
                        return;
                    }
                };

                let mut fe = FallibleEmitter::new(self, &fallible);
                if kind == Some(ConstructorKind::Success)
                    || (kind == Some(ConstructorKind::Failure)
                        && fallible.err_constructor_takes_arg())
                {
                    let arg = fe.emitter.emit_composite_value(output, &args[0]);
                    let call_str = fe.format_constructor_call(constructor_name, Some(&arg));
                    write_line!(output, "{} = {}", target_var, call_str);
                } else {
                    let call_str = fe.format_constructor_call(constructor_name, None);
                    write_line!(output, "{} = {}", target_var, call_str);
                }
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(actual_expression)
                    == Some(ConstructorKind::Failure)
                {
                    let mut fe = FallibleEmitter::new(self, &fallible);
                    let call_str = fe.format_constructor_call(fallible.err_constructor(), None);
                    write_line!(output, "{} = {}", target_var, call_str);
                } else {
                    let expression_string = self.emit_operand(output, expression);
                    write_line!(output, "{} = {}", target_var, expression_string);
                }
            }
            _ => {
                self.emit_block_to_var_with_braces(output, expression, target_var, false);
            }
        }
    }

    pub(crate) fn emit_propagate_to_let(
        &mut self,
        output: &mut String,
        var_name: &str,
        expression: &Expression,
    ) {
        let Expression::Propagate { expression, .. } = expression else {
            return;
        };
        self.emit_propagate(output, expression, Some(var_name));
    }

    pub(crate) fn emit_return(&mut self, output: &mut String, expression: &Expression) {
        let is_unit = self
            .current_return_context
            .as_ref()
            .is_some_and(|ty| ty.is_unit());

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
        } else if !self.emit_wrapped_return(output, expression) {
            let expression_string =
                self.with_position(Position::Tail, |this| this.emit_value(output, expression));
            let expression_string = self.adapt_return_to_context(expression, expression_string);
            write_line!(output, "return {}", expression_string);
        }
    }

    pub(crate) fn adapt_return_to_context(
        &mut self,
        expression: &Expression,
        emitted: String,
    ) -> String {
        let Some(return_ty) = self.current_return_context.clone() else {
            return emitted;
        };
        self.maybe_wrap_as_go_interface(emitted, &expression.get_type(), &return_ty)
    }

    /// Emit a return statement with Result/Option wrapping if applicable.
    ///
    /// Returns `false` only when the return type is NOT Result/Option (i.e., Fallible::from_type
    /// returns None). Once a Result/Option return type is identified, this function is exhaustive:
    /// all code paths emit the return and return `true`. The caller (emit_last_expression) uses
    /// `Position::Tail` only for the non-Result/Option case, so the two paths are disjoint.
    pub(crate) fn emit_wrapped_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
    ) -> bool {
        let expression_ty = expression.get_type().resolve();

        let return_ty = self
            .current_return_context
            .as_ref()
            .map(|ty| ty.resolve())
            .filter(|ctx_ty| Fallible::from_type(ctx_ty).is_some())
            .unwrap_or(expression_ty);

        let Some(fallible) = Fallible::from_type(&return_ty) else {
            return false;
        };

        self.flags.needs_stdlib = true;

        if let Expression::Identifier { .. } = expression
            && fallible.classify_constructor(expression) == Some(ConstructorKind::Failure)
        {
            let mut fe = FallibleEmitter::new(self, &fallible);
            let failure = fe.emit_failure(None);
            write_line!(output, "return {}", failure);
            return true;
        }

        if let Expression::Call {
            expression: call_expression,
            args,
            ..
        } = expression
        {
            match fallible.classify_constructor(call_expression) {
                Some(ConstructorKind::Success) => {
                    let arg = self.emit_composite_value(output, &args[0]);
                    let mut fe = FallibleEmitter::new(self, &fallible);
                    let success = fe.emit_success(&arg);
                    write_line!(output, "return {}", success);
                    return true;
                }
                Some(ConstructorKind::Failure) => {
                    if fallible.is_result() {
                        let arg = self.emit_composite_value(output, &args[0]);
                        let mut fe = FallibleEmitter::new(self, &fallible);
                        let failure = fe.emit_failure(Some(&arg));
                        write_line!(output, "return {}", failure);
                    } else {
                        let mut fe = FallibleEmitter::new(self, &fallible);
                        let failure = fe.emit_failure(None);
                        write_line!(output, "return {}", failure);
                    }
                    return true;
                }
                None => {
                    if let Some(strategy) = self.resolve_go_call_strategy(expression) {
                        let result_var =
                            self.emit_go_wrapped_call(output, expression, &strategy, &return_ty);
                        write_line!(output, "return {}", result_var);
                    } else {
                        let call = self.emit_call(output, expression, None);
                        write_line!(output, "return {}", call);
                    }
                    return true;
                }
            }
        }

        if matches!(expression, Expression::If { .. } | Expression::Match { .. }) {
            let temp_var = self.fresh_var(None);
            self.declare(&temp_var);
            let full_ty = {
                let mut fe = FallibleEmitter::new(self, &fallible);
                fe.full_type_string()
            };

            let pre_len = output.len();
            write_line!(output, "var {} {}", temp_var, full_ty);

            let saved_target_ty = self.assign_target_ty.replace(return_ty.clone());

            self.with_position(Position::Assign(temp_var.clone()), |this| {
                this.emit_branching_directly(output, expression);
            });

            self.assign_target_ty = saved_target_ty;

            write_line!(output, "return {}", temp_var);
            optimize_region(output, pre_len, Some(&temp_var));
            return true;
        }

        let value = self.emit_value(output, expression);
        write_line!(output, "return {}", value);
        true
    }

    pub(crate) fn emit_try_block(
        &mut self,
        output: &mut String,
        items: &[Expression],
        ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        // Prefer the function's return context type when the try block's own ok_ty
        // is a type variable (e.g. `Result[any, ...]` when tail is a statement),
        // or when the tail is Never-typed (ok_ty resolves to unit/Never because
        // nothing constrains it).
        let base_fallible = Fallible::from_type(ty);
        let tail_is_never = items.last().is_some_and(|last| {
            let ty = last.get_type().resolve();
            ty.is_never() || last.diverges().is_some()
        });
        let needs_context_type = tail_is_never
            || base_fallible
                .as_ref()
                .is_some_and(|f| f.ok_ty().is_variable() || f.ok_ty().is_never());

        let effective_ty = if needs_context_type {
            self.current_return_context
                .as_ref()
                .filter(|ctx_ty| Fallible::from_type(ctx_ty).is_some())
                .cloned()
                .unwrap_or_else(|| ty.clone())
        } else {
            ty.clone()
        };

        let fallible = Fallible::from_type(&effective_ty)
            .expect("`try` block must have Result or Option type");

        let result_var = self.fresh_var(Some("tryResult"));
        self.declare(&result_var);
        let full_ty = {
            let mut fe = FallibleEmitter::new(self, &fallible);
            fe.full_type_string()
        };

        write_line!(output, "{} := func() {} {{", result_var, full_ty);
        let closure_body_start = output.len();

        let saved_return_context = self.current_return_context.clone();
        self.current_return_context = Some(effective_ty.clone());

        self.with_fresh_scope(|emitter| {
            if !items.is_empty() {
                let (rest, last) = items.split_at(items.len() - 1);

                for item in rest {
                    emitter.emit_statement(output, item);
                }

                if let Some(last_item) = last.first() {
                    let diverges =
                        last_item.diverges().is_some() || last_item.get_type().resolve().is_never();

                    let is_statement_only = matches!(
                        last_item,
                        Expression::Let { .. }
                            | Expression::Const { .. }
                            | Expression::Assignment { .. }
                            | Expression::While { .. }
                            | Expression::WhileLet { .. }
                            | Expression::For { .. }
                            | Expression::Loop { .. }
                    );

                    let is_unit_call = last_item.get_type().resolve().is_unit()
                        && matches!(last_item.unwrap_parens(), Expression::Call { .. });

                    if diverges {
                        emitter.emit_statement(output, last_item);
                        if !Self::is_go_never(last_item) {
                            output.push_str("panic(\"unreachable\")\n");
                        }
                    } else if is_statement_only || is_unit_call {
                        // Statement-only tails and unit calls can't be used as values.
                        // Emit as statement, then return Ok(unit).
                        emitter.emit_statement(output, last_item);
                        let unit_val = emitter.zero_value(fallible.ok_ty());
                        let unit_return = {
                            let mut fe = FallibleEmitter::new(emitter, &fallible);
                            fe.emit_success(&unit_val)
                        };
                        write_line!(output, "return {}", unit_return);
                    } else {
                        let final_expression = emitter.emit_value(output, last_item);
                        if !final_expression.is_empty() {
                            let ok_return = {
                                let mut fe = FallibleEmitter::new(emitter, &fallible);
                                fe.emit_success(&final_expression)
                            };
                            write_line!(output, "return {}", ok_return);
                        } else {
                            let unit_val = emitter.zero_value(fallible.ok_ty());
                            let unit_return = {
                                let mut fe = FallibleEmitter::new(emitter, &fallible);
                                fe.emit_success(&unit_val)
                            };
                            write_line!(output, "return {}", unit_return);
                        }
                    }
                }
            } else {
                let unit_val = emitter.zero_value(fallible.ok_ty());
                let unit_return = {
                    let mut fe = FallibleEmitter::new(emitter, &fallible);
                    fe.emit_success(&unit_val)
                };
                write_line!(output, "return {}", unit_return);
            }
        });

        self.current_return_context = saved_return_context;

        inline_trivial_bindings(output, closure_body_start);
        output.push_str("}()\n");

        result_var
    }

    /// Optimizes `Err(...)?)` and `None?` by emitting a direct return.
    /// Returns `Some(String::new())` if handled, `None` otherwise.
    fn try_emit_error_constructor(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fallible: &Fallible,
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
                    Some(self.emit_value(output, &args[0]))
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

        self.flags.needs_stdlib = true;
        let err_return = {
            let mut fe = FallibleEmitter::new(self, fallible);
            fe.emit_contextual_failure(err_arg.as_deref())
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
        self.flags.needs_stdlib = true;

        let base_fallible = Fallible::from_type(ty);
        let tail_is_never = items.last().is_some_and(|last| {
            let ty = last.get_type().resolve();
            ty.is_never() || last.diverges().is_some()
        });
        let needs_context_type = tail_is_never
            || base_fallible
                .as_ref()
                .is_some_and(|f| f.ok_ty().is_variable() || f.ok_ty().is_never());

        let effective_ty = if needs_context_type {
            self.current_return_context
                .as_ref()
                .filter(|ctx_ty| Fallible::from_type(ctx_ty).is_some())
                .cloned()
                .unwrap_or_else(|| ty.clone())
        } else {
            ty.clone()
        };

        let result_var = self.fresh_var(Some("recoverResult"));
        self.declare(&result_var);
        let fallible = Fallible::from_type(&effective_ty)
            .expect("recover block type must be Result<T, PanicValue>");
        let inner_ty_str = self.go_type_as_string(fallible.ok_ty());

        write_line!(
            output,
            "{} := lisette.RecoverBlock(func() {} {{",
            result_var,
            inner_ty_str
        );

        let saved_return_context = self.current_return_context.clone();
        self.current_return_context = Some(fallible.ok_ty().clone());

        self.with_fresh_scope(|emitter| {
            if items.is_empty() {
                let zero_val = emitter.zero_value(fallible.ok_ty());
                write_line!(output, "return {}", zero_val);
            } else {
                for (i, item) in items.iter().enumerate() {
                    if i == items.len() - 1 {
                        let item_ty = item.get_type().resolve();
                        if item_ty.is_never() {
                            emitter.emit_statement(output, item);
                            if !Self::is_go_never(item) {
                                output.push_str("panic(\"unreachable\")\n");
                            }
                        } else if item_ty.is_unit() || item_ty.is_variable() {
                            emitter.emit_statement(output, item);
                            let zero_val = emitter.zero_value(fallible.ok_ty());
                            write_line!(output, "return {}", zero_val);
                        } else {
                            let expression = emitter.emit_value(output, item);
                            write_line!(output, "return {}", expression);
                        }
                    } else {
                        emitter.emit_statement(output, item);
                    }
                }
            }
        });

        self.current_return_context = saved_return_context;

        output.push_str("})\n");
        result_var
    }
}
