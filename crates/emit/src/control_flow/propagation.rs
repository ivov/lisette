use crate::Emitter;
use crate::control_flow::fallible::{
    ConstructorKind, Fallible, FallibleEmitter, OPTION_SOME_TAG, PARTIAL_ERR_TAG, PARTIAL_OK_TAG,
    RESULT_OK_TAG,
};
use crate::types::abi::AbiShape;
use crate::types::emitter::Position;
use crate::utils::{Staged, inline_trivial_bindings, optimize_region};
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_propagate(
        &mut self,
        output: &mut String,
        expression: &Expression,
        result_var_name: Option<&str>,
    ) -> String {
        let expression_ty = expression.get_type();
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
            if go_name.contains('(') {
                self.capture_check_var(output, &go_name)
            } else {
                go_name
            }
        } else {
            let expression_string = self.emit_operand(output, expression);
            self.capture_check_var(output, &expression_string)
        };

        let result_var = result_var_name.map(|s| s.to_string()).unwrap_or_else(|| {
            let v = self.fresh_var(Some("result"));
            self.declare(&v);
            v
        });

        let err_field = if fallible.is_result() { ".ErrVal" } else { "" };

        if let Some(shape) = self.current_lowered_abi() {
            let return_ty = self
                .current_return_context
                .as_ref()
                .map(|ctx| ctx.ty.clone())
                .expect("lowered abi");
            // Option propagation: failure carries no payload, so emit a
            // shape-specific `None` return rather than an err-return.
            let lowered_failure = if fallible.is_result() {
                let err_expr = format!("{}{}", check_var, err_field);
                self.format_lowered_err_return(&shape, &return_ty, &err_expr)
            } else {
                self.format_lowered_none_return(&shape, &return_ty)
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
                fe.emit_contextual_failure(Some(&format!("{}{}", check_var, err_field)))
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

    /// Lower early-return body for an `Err`-with-payload expression.
    pub(crate) fn format_lowered_err_return(
        &mut self,
        shape: &AbiShape,
        return_ty: &Type,
        err_expr: &str,
    ) -> String {
        match shape {
            AbiShape::BareError => format!("return {}", err_expr),
            AbiShape::ResultTuple => {
                let ok_ty = self.peel_alias(return_ty).ok_type();
                let ok_ty_str = self.go_type_as_string(&ok_ty);
                format!("return *new({}), {}", ok_ty_str, err_expr)
            }
            // Partial/Tuple flow through their own paths.
            AbiShape::PartialTuple | AbiShape::Tuple { .. } => {
                unreachable!("not reached for shapes with their own emission paths")
            }
            AbiShape::CommaOk | AbiShape::NullableReturn => {
                unreachable!("Option's failure constructor `None` carries no payload")
            }
        }
    }

    /// Lower tail-return body for a success-constructor's payload value.
    pub(crate) fn format_lowered_ok_return(&mut self, shape: &AbiShape, ok_expr: &str) -> String {
        match shape {
            AbiShape::BareError => "return nil".to_string(),
            AbiShape::ResultTuple => format!("return {}, nil", ok_expr),
            AbiShape::PartialTuple | AbiShape::Tuple { .. } => {
                unreachable!("not reached for shapes with their own emission paths")
            }
            AbiShape::CommaOk => format!("return {}, true", ok_expr),
            AbiShape::NullableReturn => format!("return {}", ok_expr),
        }
    }

    /// Lower body for a bare `None` (failure constructor with no payload).
    pub(crate) fn format_lowered_none_return(
        &mut self,
        shape: &AbiShape,
        return_ty: &Type,
    ) -> String {
        match shape {
            AbiShape::CommaOk => {
                let inner = self.peel_alias(return_ty).ok_type();
                let inner_str = self.go_type_as_string(&inner);
                format!("return *new({}), false", inner_str)
            }
            AbiShape::NullableReturn => "return nil".to_string(),
            _ => unreachable!("only Option's `None` lacks a payload"),
        }
    }

    /// Destructure a Lisette tagged value into a lowered Go-tuple return.
    pub(crate) fn emit_lowered_result_return(
        &mut self,
        output: &mut String,
        result_value: &str,
        return_ty: &Type,
        shape: &AbiShape,
    ) {
        let ok_ty_str = match shape {
            AbiShape::ResultTuple | AbiShape::PartialTuple | AbiShape::CommaOk => {
                let ok_ty = self.peel_alias(return_ty).ok_type();
                Some(self.go_type_as_string(&ok_ty))
            }
            _ => None,
        };
        match shape {
            AbiShape::BareError => {
                write_line!(
                    output,
                    "if {p}.Tag == {ok} {{\nreturn nil\n}}\nreturn {p}.ErrVal",
                    p = result_value,
                    ok = RESULT_OK_TAG,
                );
            }
            AbiShape::ResultTuple => {
                let t = ok_ty_str.as_deref().unwrap();
                write_line!(
                    output,
                    "if {p}.Tag == {ok} {{\nreturn {p}.OkVal, nil\n}}\nreturn *new({t}), {p}.ErrVal",
                    p = result_value,
                    ok = RESULT_OK_TAG,
                );
            }
            AbiShape::PartialTuple => {
                let t = ok_ty_str.as_deref().unwrap();
                write_line!(
                    output,
                    "if {p}.Tag == {ok} {{\nreturn {p}.OkVal, nil\n}}\n\
                     if {p}.Tag == {err} {{\nreturn *new({t}), {p}.ErrVal\n}}\n\
                     return {p}.OkVal, {p}.ErrVal",
                    p = result_value,
                    ok = PARTIAL_OK_TAG,
                    err = PARTIAL_ERR_TAG,
                );
            }
            AbiShape::CommaOk => {
                let t = ok_ty_str.as_deref().unwrap();
                write_line!(
                    output,
                    "if {p}.Tag == {some} {{\nreturn {p}.SomeVal, true\n}}\n\
                     return *new({t}), false",
                    p = result_value,
                    some = OPTION_SOME_TAG,
                );
            }
            AbiShape::NullableReturn => {
                write_line!(
                    output,
                    "if {p}.Tag == {some} {{\nreturn {p}.SomeVal\n}}\nreturn nil",
                    p = result_value,
                    some = OPTION_SOME_TAG,
                );
            }
            AbiShape::Tuple { arity } => {
                let peeled = self.peel_alias(return_ty);
                let slot_tys = crate::types::abi::tuple_element_types(&peeled);
                let any_nullable = slot_tys.iter().any(|t| self.is_nullable_option(t));
                if !any_nullable {
                    let fields: Vec<String> = (0..*arity)
                        .map(|i| format!("{}.{}", result_value, syntax::parse::TUPLE_FIELDS[i]))
                        .collect();
                    write_line!(output, "return {}", fields.join(", "));
                    return;
                }
                let fields: Vec<String> = (0..*arity)
                    .map(|i| {
                        let raw = format!("{}.{}", result_value, syntax::parse::TUPLE_FIELDS[i]);
                        slot_tys
                            .get(i)
                            .filter(|t| self.is_nullable_option(t))
                            .map(|t| self.emit_option_unwrap_to_nullable(output, &raw, t))
                            .unwrap_or(raw)
                    })
                    .collect();
                write_line!(output, "return {}", fields.join(", "));
            }
        }
    }

    /// `Some(x)`/`None` collapse to `x`/`nil`; other Option expressions
    /// go through `emit_option_unwrap_to_nullable`.
    fn emit_nullable_slot_value(
        &mut self,
        output: &mut String,
        expression: &Expression,
        slot_ty: &Type,
    ) -> String {
        if let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
            && let Some(kind) = callee.as_option_constructor()
        {
            return match kind {
                Ok(()) => {
                    debug_assert_eq!(args.len(), 1, "Some(...) takes exactly one arg");
                    self.emit_composite_value(output, &args[0])
                }
                Err(()) => "nil".to_string(),
            };
        }
        if let Expression::Identifier { .. } = expression
            && expression.as_option_constructor() == Some(Err(()))
        {
            return "nil".to_string();
        }
        let value = self.emit_value(output, expression);
        self.emit_option_unwrap_to_nullable(output, &value, slot_ty)
    }

    /// Tail return for `PartialTuple` and `Tuple` ABIs, which need
    /// per-shape handling beyond the generic `emit_wrapped_return` path.
    pub(crate) fn try_emit_lowered_tail_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
    ) -> bool {
        let Some(shape) = self.current_lowered_abi() else {
            return false;
        };
        match shape {
            AbiShape::PartialTuple => self.emit_lowered_partial_tail(output, expression),
            AbiShape::Tuple { arity } => self.emit_lowered_tuple_tail(output, expression, arity),
            _ => false,
        }
    }

    fn emit_lowered_tuple_tail(
        &mut self,
        output: &mut String,
        expression: &Expression,
        arity: usize,
    ) -> bool {
        if let Expression::Tuple { elements, .. } = expression
            && elements.len() == arity
        {
            let return_ty = self
                .current_return_context
                .as_ref()
                .expect("lowered abi requires a return context")
                .ty
                .clone();
            let slot_tys = crate::types::abi::tuple_element_types(&self.peel_alias(&return_ty));
            let stages: Vec<Staged> = elements
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let mut setup = String::new();
                    let value = match slot_tys.get(i) {
                        Some(slot_ty) if self.is_nullable_option(slot_ty) => {
                            self.emit_nullable_slot_value(&mut setup, e, slot_ty)
                        }
                        _ => self.emit_composite_value(&mut setup, e),
                    };
                    Staged::new(setup, value, e)
                })
                .collect();
            let parts = self.sequence(output, stages, "_ret");
            write_line!(output, "return {}", parts.join(", "));
            return true;
        }

        let return_ty = self
            .current_return_context
            .as_ref()
            .expect("lowered abi requires a return context")
            .ty
            .clone();
        let value = self.emit_value(output, expression);
        let temp = self.fresh_var(Some("tup"));
        self.declare(&temp);
        write_line!(output, "{} := {}", temp, value);
        self.emit_lowered_result_return(output, &temp, &return_ty, &AbiShape::Tuple { arity });
        true
    }

    fn emit_lowered_partial_tail(&mut self, output: &mut String, expression: &Expression) -> bool {
        let return_ty = self
            .current_return_context
            .as_ref()
            .expect("lowered abi requires a return context")
            .ty
            .clone();

        if let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
            && let Some(variant) = callee.as_partial_constructor()
        {
            self.flags.needs_stdlib = true;
            match variant {
                "Ok" => {
                    let v = self.emit_composite_value(output, &args[0]);
                    write_line!(output, "return {}, nil", v);
                }
                "Err" => {
                    let e = self.emit_composite_value(output, &args[0]);
                    let ok_ty = self.peel_alias(&return_ty).ok_type();
                    let ok_ty_str = self.go_type_as_string(&ok_ty);
                    write_line!(output, "return *new({}), {}", ok_ty_str, e);
                }
                "Both" => {
                    let v = self.emit_composite_value(output, &args[0]);
                    let e = self.emit_composite_value(output, &args[1]);
                    write_line!(output, "return {}, {}", v, e);
                }
                _ => unreachable!("as_partial_constructor only returns Ok/Err/Both"),
            }
            return true;
        }

        let value = self.emit_value(output, expression);
        self.emit_lowered_result_return(output, &value, &return_ty, &AbiShape::PartialTuple);
        true
    }

    /// Assign the propagated expression to a fresh `check` temp so its
    /// `.Tag`/`.ErrVal`/etc. can be read without re-evaluating the (possibly
    /// effectful) underlying call.
    fn capture_check_var(&mut self, output: &mut String, expression_string: &str) -> String {
        let check_var = self.fresh_var(Some("check"));
        self.declare(&check_var);
        write_line!(output, "{} := {}", check_var, expression_string);
        check_var
    }

    pub(crate) fn emit_option_result_assignment(
        &mut self,
        output: &mut String,
        target_var: &str,
        target_ty: Option<&Type>,
        expression: &Expression,
    ) {
        let ty = target_ty
            .filter(|t| t.is_option() || t.is_result())
            .cloned()
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
            .is_some_and(|ctx| ctx.ty.is_unit());

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
        } else if !self.try_emit_lowered_tail_return(output, expression)
            && !self.emit_wrapped_return(output, expression)
        {
            let expression_string =
                self.with_position(Position::Tail, |this| this.emit_value(output, expression));
            let return_ty = self
                .current_return_context
                .as_ref()
                .map(|ctx| ctx.ty.clone());
            let expression_string =
                self.apply_type_coercion(output, return_ty.as_ref(), expression, expression_string);
            write_line!(output, "return {}", expression_string);
        }
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
        let expression_ty = expression.get_type();

        let return_ty = self
            .current_return_context
            .as_ref()
            .map(|ctx| ctx.ty.clone())
            .filter(|ty| Fallible::from_type(ty).is_some())
            .unwrap_or(expression_ty);

        let Some(fallible) = Fallible::from_type(&return_ty) else {
            return false;
        };

        self.flags.needs_stdlib = true;

        let force_tagged = self
            .current_return_context
            .as_ref()
            .is_some_and(|ctx| ctx.force_tagged);
        let lowered = if force_tagged {
            None
        } else {
            self.classify_direct_emission(&return_ty)
        };

        if let Expression::Identifier { .. } = expression
            && fallible.classify_constructor(expression) == Some(ConstructorKind::Failure)
        {
            // Only `None` reaches here — `Err` always has a payload.
            if let Some(shape) = lowered.as_ref() {
                let line = self.format_lowered_none_return(shape, &return_ty);
                write_line!(output, "{}", line);
            } else {
                let mut fe = FallibleEmitter::new(self, &fallible);
                let failure = fe.emit_failure(None);
                write_line!(output, "return {}", failure);
            }
            return true;
        }

        if matches!(expression, Expression::Call { .. }) {
            self.emit_wrapped_call_return(
                output,
                expression,
                &fallible,
                &return_ty,
                lowered.as_ref(),
            );
            return true;
        }

        if matches!(expression, Expression::If { .. } | Expression::Match { .. }) {
            self.emit_wrapped_branching_return(
                output,
                expression,
                &fallible,
                &return_ty,
                lowered.as_ref(),
            );
            return true;
        }

        let value = self.emit_value(output, expression);
        if let Some(shape) = lowered {
            // The destructure references the value multiple times (`.Tag`,
            // `.OkVal`, `.ErrVal` etc.); hoist to avoid re-evaluating.
            let temp = self.fresh_var(Some("v"));
            self.declare(&temp);
            write_line!(output, "{} := {}", temp, value);
            self.emit_lowered_result_return(output, &temp, &return_ty, &shape);
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
        fallible: &Fallible,
        return_ty: &Type,
        lowered: Option<&AbiShape>,
    ) {
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
                            let _ = self.emit_composite_value(output, &args[0]);
                        }
                        String::new()
                    } else if args.is_empty() {
                        // `Some` with no payload wouldn't typecheck; only
                        // possible when Ok type is unit and we still need a
                        // value for the tuple (`Some(())` under CommaOk).
                        "struct{}{}".to_string()
                    } else {
                        self.emit_composite_value(output, &args[0])
                    };
                    let line = self.format_lowered_ok_return(shape, &ok_arg);
                    write_line!(output, "{}", line);
                } else {
                    let arg = self.emit_composite_value(output, &args[0]);
                    let mut fe = FallibleEmitter::new(self, fallible);
                    let success = fe.emit_success(&arg);
                    write_line!(output, "return {}", success);
                }
            }
            Some(ConstructorKind::Failure) => {
                if let Some(shape) = lowered {
                    if args.is_empty() {
                        // `None` under lowered Option (CommaOk/NullableReturn).
                        let line = self.format_lowered_none_return(shape, return_ty);
                        write_line!(output, "{}", line);
                    } else {
                        let err_expr = self.emit_composite_value(output, &args[0]);
                        let line = self.format_lowered_err_return(shape, return_ty, &err_expr);
                        write_line!(output, "{}", line);
                    }
                } else {
                    let failure = if fallible.is_result() {
                        let arg = self.emit_composite_value(output, &args[0]);
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
            let call = self.emit_call(output, expression, None);
            write_line!(output, "return {}", call);
            return;
        }
        if let Some(strategy) = self.resolve_go_call_strategy(expression) {
            let result_var = self.emit_go_wrapped_call(output, expression, &strategy, return_ty);
            if let Some(shape) = lowered {
                self.emit_lowered_result_return(output, &result_var, return_ty, shape);
            } else {
                write_line!(output, "return {}", result_var);
            }
            return;
        }
        if let Some(shape) = lowered {
            let value = self.emit_value(output, expression);
            let temp = self.fresh_var(Some("v"));
            self.declare(&temp);
            write_line!(output, "{} := {}", temp, value);
            self.emit_lowered_result_return(output, &temp, return_ty, shape);
            return;
        }
        let call = self.emit_call(output, expression, None);
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
                && let Some(strategy) = self.classify_go_return_type(return_type, &[])
            {
                use crate::GoCallStrategy as G;
                return match (strategy, enclosing_shape) {
                    (G::Result, AbiShape::ResultTuple | AbiShape::BareError)
                    | (G::Partial, AbiShape::PartialTuple)
                    | (G::CommaOk, AbiShape::CommaOk)
                    | (G::NullableReturn, AbiShape::NullableReturn) => true,
                    (G::Tuple { arity: a }, AbiShape::Tuple { arity: b }) => a == *b,
                    _ => false,
                };
            }
        }
        if let Some(callee_shape) = self.classify_callee_abi(callee) {
            return callee_shape == *enclosing_shape;
        }
        false
    }

    /// Lowered ABI: push the return to each branch leaf so `Some(42)`
    /// collapses to `return 42, true` directly. Tagged ABI keeps the
    /// materialise-then-return shape so `optimize_region` can inline.
    fn emit_wrapped_branching_return(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fallible: &Fallible,
        return_ty: &Type,
        lowered: Option<&AbiShape>,
    ) {
        if lowered.is_some() {
            let saved_target_ty = self.assign_target_ty.replace(return_ty.clone());
            self.with_position(Position::Tail, |this| {
                this.emit_branching_directly(output, expression);
            });
            self.assign_target_ty = saved_target_ty;
            return;
        }

        let temp_var = self.fresh_var(None);
        self.declare(&temp_var);
        let full_ty = {
            let mut fe = FallibleEmitter::new(self, fallible);
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
    }

    pub(crate) fn emit_try_block(
        &mut self,
        output: &mut String,
        items: &[Expression],
        ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

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
        let closure_body_start = output.len();

        // The IIFE's signature is the tagged `Result`, so its body must too.
        let saved_return_context = self
            .current_return_context
            .replace(crate::ReturnContext::tagged(effective_ty.clone()));

        self.with_fresh_scope(|emitter| {
            emitter.emit_try_body(output, items, &fallible);
        });

        self.current_return_context = saved_return_context;

        inline_trivial_bindings(output, closure_body_start);
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
        self.current_return_context
            .as_ref()
            .map(|ctx| ctx.ty.clone())
            .filter(|ty| Fallible::from_type(ty).is_some())
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
        self.emit_try_tail(output, last, fallible);
    }

    fn emit_try_tail(&mut self, output: &mut String, last: &Expression, fallible: &Fallible) {
        if last.diverges().is_some() || last.get_type().is_never() {
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }

        let is_statement_only = matches!(
            last,
            Expression::Let { .. }
                | Expression::Const { .. }
                | Expression::Assignment { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::For { .. }
                | Expression::Loop { .. }
        );
        let is_unit_call =
            last.get_type().is_unit() && matches!(last.unwrap_parens(), Expression::Call { .. });
        if is_statement_only || is_unit_call {
            // Statement-only tails and unit calls can't be used as values.
            // Emit as statement, then return Ok(unit).
            self.emit_statement(output, last);
            self.emit_try_unit_return(output, fallible);
            return;
        }

        let final_expression = self.emit_value(output, last);
        if final_expression.is_empty() {
            self.emit_try_unit_return(output, fallible);
        } else {
            self.emit_try_success_return(output, &final_expression, fallible);
        }
    }

    fn emit_try_unit_return(&mut self, output: &mut String, fallible: &Fallible) {
        let unit_val = self.zero_value(fallible.ok_ty());
        self.emit_try_success_return(output, &unit_val, fallible);
    }

    fn emit_try_success_return(&mut self, output: &mut String, value: &str, fallible: &Fallible) {
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

        let saved_return_context = self
            .current_return_context
            .replace(crate::ReturnContext::new(fallible.ok_ty().clone()));

        self.with_fresh_scope(|emitter| {
            emitter.emit_recover_body(output, items, &fallible);
        });

        self.current_return_context = saved_return_context;

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
            let zero = self.zero_value(fallible.ok_ty());
            write_line!(output, "return {}", zero);
            return;
        };
        for item in rest {
            self.emit_statement(output, item);
        }
        self.emit_recover_tail(output, last, fallible);
    }

    fn emit_recover_tail(&mut self, output: &mut String, last: &Expression, fallible: &Fallible) {
        let item_ty = last.get_type();
        if item_ty.is_never() {
            self.emit_statement(output, last);
            if !Self::is_go_never(last) {
                output.push_str("panic(\"unreachable\")\n");
            }
            return;
        }
        if item_ty.is_unit() || item_ty.is_variable() {
            self.emit_statement(output, last);
            let zero = self.zero_value(fallible.ok_ty());
            write_line!(output, "return {}", zero);
            return;
        }
        let expression = self.emit_value(output, last);
        write_line!(output, "return {}", expression);
    }
}
