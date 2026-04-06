use crate::Emitter;
use crate::go::control_flow::fallible::{
    Fallible, FallibleEmitter, OPTION_SOME_TAG, PARTIAL_BOTH_CTOR, PARTIAL_ERR_TAG,
    PARTIAL_OK_CTOR, PARTIAL_OK_TAG, RESULT_OK_TAG,
};
use crate::go::is_order_sensitive;
use crate::go::names::go_name;
use crate::go::utils::optimize_region;
use crate::go::write_line;
use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

use super::GoCallStrategy;

impl Emitter<'_> {
    pub(super) fn emit_go_tuple_call_wrapped(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        arity: usize,
    ) -> String {
        let Expression::Call {
            expression: callee,
            args,
            type_args,
            ty,
            span,
            ..
        } = call_expression
        else {
            unreachable!("emit_go_tuple_call_wrapped called with non-call expression");
        };

        let call_str = self.emit_call(output, callee, args, type_args, None, *span);

        let temp_vars = self.create_temp_vars("ret", arity);

        write_line!(output, "{} := {}", temp_vars.join(", "), call_str);

        self.emit_tuple_from_vars(output, &temp_vars, ty)
    }

    pub(super) fn emit_go_partial_call_wrapped(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        partial_ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        let Expression::Call {
            expression: callee,
            args,
            type_args,
            span,
            ..
        } = call_expression
        else {
            unreachable!("emit_go_partial_call_wrapped called with non-call expression");
        };

        let call_str = self.emit_call(output, callee, args, type_args, None, *span);
        self.emit_partial_wrapping(output, &call_str, partial_ty)
    }

    pub(super) fn emit_partial_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        partial_ty: &Type,
    ) -> String {
        let ok_ty = partial_ty.ok_type();
        let err_ty = partial_ty.err_type();
        let ok_ty_str = self.go_type_as_string(&ok_ty);
        let err_ty_str = self.go_type_as_string(&err_ty);
        let pkg = go_name::GO_STDLIB_PKG;

        let (err_var, val_var) = self.extract_go_returns(output, call_str, &ok_ty);

        let type_params = format!("{}, {}", ok_ty_str, err_ty_str);
        let result_ty_str = format!("{pkg}.Partial[{type_params}]");
        let result_var = self.fresh_var(Some("result"));
        self.declare(&result_var);

        write_line!(output, "var {} {}", result_var, result_ty_str);
        write_line!(output, "if {} != nil {{", err_var);
        write_line!(
            output,
            "{} = {PARTIAL_BOTH_CTOR}[{type_params}]({}, {})",
            result_var,
            val_var,
            err_var
        );
        output.push_str("} else {\n");
        write_line!(
            output,
            "{} = {PARTIAL_OK_CTOR}[{type_params}]({})",
            result_var,
            val_var
        );
        output.push_str("}\n");

        result_var
    }

    pub(super) fn emit_go_result_call_wrapped(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        result_ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        let Expression::Call {
            expression: callee,
            args,
            type_args,
            span,
            ..
        } = call_expression
        else {
            unreachable!("emit_go_result_call_wrapped called with non-call expression");
        };

        let call_str = self.emit_call(output, callee, args, type_args, None, *span);
        self.emit_result_wrapping(output, &call_str, result_ty)
    }

    pub(super) fn emit_result_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        result_ty: &Type,
    ) -> String {
        let fallible = Fallible::from_type(result_ty).expect("Result type expected");

        if fallible.ok_ty().is_unit() {
            return self.emit_unit_result_wrapping(output, call_str, &fallible);
        }

        let ok_ty = fallible.ok_ty();
        let (err_var, ok_val) = self.extract_go_returns(output, call_str, ok_ty);

        let mut fe = FallibleEmitter::new(self, &fallible);
        let result_ty_str = fe.full_type_string();
        let result_var = fe.emitter.fresh_var(Some("result"));
        fe.emitter.declare(&result_var);

        let needs_nil_guard = ok_ty.is_ref() || self.as_interface(ok_ty).is_some();

        write_line!(output, "var {} {}", result_var, result_ty_str);
        write_line!(output, "if {} != nil {{", err_var);

        let mut fe = FallibleEmitter::new(self, &fallible);
        let err_wrapper = fe.emit_failure(Some(&err_var));
        write_line!(output, "{} = {}", result_var, err_wrapper);

        if needs_nil_guard {
            self.emit_nil_guard(output, &ok_val, ok_ty, &result_var, &fallible);
        }

        output.push_str("} else {\n");

        let mut fe = FallibleEmitter::new(self, &fallible);
        let ok_wrapper = fe.emit_success(&ok_val);
        write_line!(output, "{} = {}", result_var, ok_wrapper);

        output.push_str("}\n");

        result_var
    }

    fn emit_unit_result_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        fallible: &Fallible,
    ) -> String {
        let err_var = self.fresh_var(Some("ret"));
        self.declare(&err_var);
        write_line!(output, "{} := {}", err_var, call_str);

        let mut fe = FallibleEmitter::new(self, fallible);
        let result_ty_str = fe.full_type_string();
        let result_var = fe.emitter.fresh_var(Some("result"));
        fe.emitter.declare(&result_var);

        write_line!(output, "var {} {}", result_var, result_ty_str);
        write_line!(output, "if {} != nil {{", err_var);

        let mut fe = FallibleEmitter::new(self, fallible);
        let err_wrapper = fe.emit_failure(Some(&err_var));
        write_line!(output, "{} = {}", result_var, err_wrapper);

        output.push_str("} else {\n");

        let mut fe = FallibleEmitter::new(self, fallible);
        let ok_wrapper = fe.emit_success("struct{}{}");
        write_line!(output, "{} = {}", result_var, ok_wrapper);

        output.push_str("}\n");

        result_var
    }

    /// Destructure a Go multi-return call into error and value variables.
    ///
    /// For tuple ok types, creates N+1 temp variables and rebuilds the Lisette tuple.
    /// For non-tuple ok types, creates 2 temp variables (value, error).
    fn extract_go_returns(
        &mut self,
        output: &mut String,
        call_str: &str,
        ok_ty: &Type,
    ) -> (String, String) {
        if let Type::Tuple(elements) = ok_ty {
            let tuple_arity = elements.len();
            let temp_vars = self.create_temp_vars("ret", tuple_arity + 1);
            write_line!(output, "{} := {}", temp_vars.join(", "), call_str);
            let tuple_var = self.emit_tuple_from_vars(output, &temp_vars[..tuple_arity], ok_ty);
            (temp_vars.last().unwrap().clone(), tuple_var)
        } else {
            let val_var = self.fresh_var(Some("ret"));
            self.declare(&val_var);
            let err_var = self.fresh_var(Some("ret"));
            self.declare(&err_var);
            write_line!(output, "{}, {} := {}", val_var, err_var, call_str);
            (err_var, val_var)
        }
    }

    fn emit_nil_guard(
        &mut self,
        output: &mut String,
        ok_val: &str,
        ok_ty: &Type,
        result_var: &str,
        fallible: &Fallible,
    ) {
        let nil_check = if ok_ty.is_tuple() {
            format!("{}.First", ok_val)
        } else {
            ok_val.to_string()
        };

        let is_interface = self.as_interface(ok_ty).is_some();
        if is_interface {
            write_line!(
                output,
                "}} else if lisette.IsNilInterface({}) {{",
                nil_check
            );
        } else {
            write_line!(output, "}} else if {} == nil {{", nil_check);
        }

        self.flags.needs_errors = true;
        let mut fe = FallibleEmitter::new(self, fallible);
        let nil_err = fe.emit_failure(Some("errors.New(\"unexpected nil\")"));
        write_line!(output, "{} = {}", result_var, nil_err);
    }

    pub(crate) fn classify_go_fn_value(&self, expression: &Expression) -> Option<GoCallStrategy> {
        let inner = expression.unwrap_parens();

        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
            && Self::is_go_receiver(receiver)
        {
            let fn_type = expression.get_type().resolve();
            let Type::Function { return_type, .. } = fn_type else {
                return None;
            };

            let go_hints = if let Expression::DotAccess {
                expression: receiver_expression,
                member,
                ..
            } = inner
            {
                self.go_qualified_name(receiver_expression, member)
                    .and_then(|name| self.ctx.definitions.get(name.as_str()))
                    .map(|d| d.go_hints().to_vec())
                    .unwrap_or_default()
            } else {
                vec![]
            };

            return self.classify_go_return_type(&return_type, &go_hints);
        }

        None
    }

    pub(crate) fn is_go_array_return_value(&self, expression: &Expression) -> bool {
        if let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = expression.unwrap_parens()
            && Self::is_go_receiver(receiver)
        {
            return self.has_go_array_return(receiver, member);
        }
        false
    }

    fn hoist_go_fn_if_needed(&mut self, output: &mut String, expression: &Expression) -> String {
        let go_fn_str = self.emit_operand(output, expression);

        let is_go_module_fn = matches!(
            expression.unwrap_parens(),
            Expression::DotAccess { expression, .. }
            if matches!(
                expression.get_type(),
                Type::Constructor { ref id, .. } if id.starts_with(go_name::IMPORT_GO_PREFIX)
            )
        );
        if is_go_module_fn {
            return go_fn_str;
        }

        if is_order_sensitive(expression) {
            let temp = self.fresh_var(Some("fn"));
            self.declare(&temp);
            write_line!(output, "{} := {}", temp, go_fn_str);
            temp
        } else {
            go_fn_str
        }
    }

    fn build_wrapper_params(&mut self, params: &[Type]) -> (Vec<String>, Vec<String>) {
        let mut param_strs = Vec::new();
        let mut arg_names = Vec::new();
        let last_idx = params.len().saturating_sub(1);
        for (i, param_ty) in params.iter().enumerate() {
            let name = format!("arg{}", i);
            let ty_str = self.go_type_as_string(param_ty);
            param_strs.push(format!("{} {}", name, ty_str));
            if i == last_idx && param_ty.get_name() == Some("VarArgs") {
                arg_names.push(format!("{}...", name));
            } else {
                arg_names.push(name);
            }
        }
        (param_strs, arg_names)
    }

    pub(crate) fn emit_array_return_wrapper(
        &mut self,
        output: &mut String,
        expression: &Expression,
    ) -> String {
        let fn_type = expression.get_type().resolve();
        let (params, return_type) = match fn_type {
            Type::Function {
                params,
                return_type,
                ..
            } => (params, *return_type),
            _ => return self.emit_operand(output, expression),
        };

        let go_fn_str = self.hoist_go_fn_if_needed(output, expression);
        let (param_strs, arg_names) = self.build_wrapper_params(&params);

        let ret_ty_str = self.go_type_as_string(&return_type);
        let call_str = format!("{}({})", go_fn_str, arg_names.join(", "));

        let arr_var = self.fresh_var(Some("arr"));
        self.declare(&arr_var);

        format!(
            "func({}) {} {{\n{} := {}\nreturn {}[:]\n}}",
            param_strs.join(", "),
            ret_ty_str,
            arr_var,
            call_str,
            arr_var,
        )
    }

    pub(crate) fn emit_go_fn_wrapper(
        &mut self,
        output: &mut String,
        expression: &Expression,
        strategy: &GoCallStrategy,
    ) -> String {
        self.flags.needs_stdlib = true;

        let fn_type = expression.get_type().resolve();
        let (params, return_type) = match fn_type {
            Type::Function {
                params,
                return_type,
                ..
            } => (params, *return_type),
            _ => unreachable!("expected function type"),
        };

        let go_fn_str = self.hoist_go_fn_if_needed(output, expression);
        let (param_strs, arg_names) = self.build_wrapper_params(&params);

        let ret_ty_str = self.go_type_as_string(&return_type);
        let call_str = format!("{}({})", go_fn_str, arg_names.join(", "));

        let mut body = String::new();
        let result_var = match strategy {
            GoCallStrategy::Result => self.emit_result_wrapping(&mut body, &call_str, &return_type),
            GoCallStrategy::CommaOk => {
                self.emit_comma_ok_wrapping(&mut body, &call_str, &return_type)
            }
            GoCallStrategy::NullableReturn => {
                let raw_var = self.fresh_var(Some("raw"));
                self.declare(&raw_var);
                write_line!(body, "{} := {}", raw_var, call_str);
                self.emit_nil_check_option_wrap(&mut body, &raw_var, &return_type)
            }
            GoCallStrategy::Tuple { arity } => {
                let temp_vars = self.create_temp_vars("ret", *arity);
                write_line!(body, "{} := {}", temp_vars.join(", "), call_str);
                self.emit_tuple_from_vars(&mut body, &temp_vars, &return_type)
            }
            GoCallStrategy::Partial => {
                self.emit_partial_wrapping(&mut body, &call_str, &return_type)
            }
        };

        write_line!(body, "return {}", result_var);
        optimize_region(&mut body, 0, Some(&result_var));

        format!(
            "func({}) {} {{\n{}}}",
            param_strs.join(", "),
            ret_ty_str,
            body
        )
    }

    pub(crate) fn emit_lisette_callback_wrapper(
        &mut self,
        output: &mut String,
        fn_value: &str,
        fn_type: &Type,
    ) -> String {
        let Type::Function {
            params,
            return_type,
            ..
        } = fn_type
        else {
            return fn_value.to_string();
        };

        let return_type = return_type.resolve();

        let (param_strs, arg_names) = self.build_wrapper_params(params);
        let params_str = param_strs.join(", ");

        let cb_var = self.fresh_var(Some("cb"));
        self.declare(&cb_var);
        write_line!(output, "{} := {}", cb_var, fn_value);

        let call_str = format!("{}({})", cb_var, arg_names.join(", "));

        self.flags.needs_stdlib = true;

        let (go_ret, body) = if return_type.is_result() {
            let ok_ty = return_type.ok_type();
            let err_ty = return_type.err_type();
            let err_ty_str = self.go_type_as_string(&err_ty);
            let res = self.fresh_var(Some("res"));
            self.declare(&res);

            let mut b = format!("{res} := {call_str}\n");
            if ok_ty.is_unit() {
                // Result<(), error> → func(...) error
                let ok_tag = RESULT_OK_TAG;
                write_line!(
                    b,
                    "if {res}.Tag == {ok_tag} {{\nreturn nil\n}}\nreturn {res}.ErrVal"
                );
                (err_ty_str, b)
            } else {
                // Result<T, error> → func(...) (T, error)
                let ok_ty_str = self.go_type_as_string(&ok_ty);
                let ok_tag = RESULT_OK_TAG;
                write_line!(
                    b,
                    "if {res}.Tag == {ok_tag} {{\nreturn {res}.OkVal, nil\n}}\n\
                     return *new({ok_ty_str}), {res}.ErrVal"
                );
                (format!("({ok_ty_str}, {err_ty_str})"), b)
            }
        } else if return_type.is_partial() {
            // Partial<T, error> → func(...) (T, error)
            let ok_ty = return_type.ok_type();
            let err_ty = return_type.err_type();
            let ok_ty_str = self.go_type_as_string(&ok_ty);
            let err_ty_str = self.go_type_as_string(&err_ty);
            let res = self.fresh_var(Some("res"));
            self.declare(&res);

            let b = format!(
                "{res} := {call_str}\n\
                 if {res}.Tag == {PARTIAL_OK_TAG} {{\nreturn {res}.OkVal, nil\n}}\n\
                 if {res}.Tag == {PARTIAL_ERR_TAG} {{\nreturn *new({ok_ty_str}), {res}.ErrVal\n}}\n\
                 return {res}.OkVal, {res}.ErrVal\n"
            );
            (format!("({ok_ty_str}, {err_ty_str})"), b)
        } else if return_type.is_option() {
            // Option<T> → func(...) (T, bool)
            let inner_ty_str = self.go_type_as_string(&return_type.ok_type());
            let opt = self.fresh_var(Some("opt"));
            self.declare(&opt);

            let some_tag = OPTION_SOME_TAG;
            let b = format!(
                "{opt} := {call_str}\n\
                 if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal, true\n}}\n\
                 return *new({inner_ty_str}), false\n"
            );
            (format!("({inner_ty_str}, bool)"), b)
        } else if let Some(arity) = return_type.tuple_arity()
            && arity >= 2
        {
            // Tuple<T1, T2, ...> → func(...) (T1, T2, ...)
            let tuple_params = match &return_type {
                Type::Constructor { params, .. } => params,
                _ => return fn_value.to_string(),
            };
            let ret_types: Vec<String> = tuple_params
                .iter()
                .map(|t| self.go_type_as_string(t))
                .collect();
            let tup = self.fresh_var(Some("tup"));
            self.declare(&tup);
            let fields: Vec<String> = (0..arity)
                .map(|i| format!("{tup}.{}", TUPLE_FIELDS[i]))
                .collect();
            let b = format!("{tup} := {call_str}\nreturn {}\n", fields.join(", "));
            (format!("({})", ret_types.join(", ")), b)
        } else {
            return fn_value.to_string();
        };

        format!("func({params_str}) {go_ret} {{\n{body}}}")
    }
}
