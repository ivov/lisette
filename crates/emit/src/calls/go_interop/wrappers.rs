use crate::Emitter;
use crate::control_flow::fallible::{
    Fallible, FallibleEmitter, PARTIAL_BOTH_CTOR, PARTIAL_OK_CTOR,
};
use crate::expressions::context::ExpressionContext;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

use super::GoCallStrategy;

impl Emitter<'_> {
    pub(super) fn emit_go_tuple_call_wrapped(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        arity: usize,
    ) -> String {
        let Expression::Call { ty, .. } = call_expression else {
            unreachable!("emit_go_tuple_call_wrapped called with non-call expression");
        };

        let call_str = self.emit_call(output, call_expression, None, ExpressionContext::value());

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
        self.requirements.require_stdlib();

        let call_str = self.emit_call(output, call_expression, None, ExpressionContext::value());
        self.emit_partial_wrapping(output, &call_str, partial_ty)
    }

    pub(crate) fn emit_partial_wrapping(
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
        self.requirements.require_stdlib();

        let call_str = self.emit_call(output, call_expression, None, ExpressionContext::value());
        self.emit_result_wrapping(output, &call_str, result_ty)
    }

    pub(crate) fn emit_result_wrapping(
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

        let interface_id = self.facts.as_interface(ok_ty);
        let needs_nil_guard = ok_ty.is_ref()
            || interface_id
                .as_deref()
                .is_some_and(|id| id != go_name::PRELUDE_ERROR_ID);

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
        let err_var = self.hoist_tmp_value(output, "ret", call_str);

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

        let is_interface = self.facts.is_interface(ok_ty);
        if is_interface {
            write_line!(
                output,
                "}} else if lisette.IsNilInterface({}) {{",
                nil_check
            );
        } else {
            write_line!(output, "}} else if {} == nil {{", nil_check);
        }

        self.requirements.require_errors();
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
            let fn_type = expression.get_type();
            let Type::Function { return_type, .. } = fn_type.unwrap_forall() else {
                return None;
            };
            let return_type = return_type.clone();

            let go_hints = if let Expression::DotAccess {
                expression: receiver_expression,
                member,
                ..
            } = inner
            {
                self.go_qualified_name(receiver_expression, member)
                    .and_then(|name| self.facts.definition(name.as_str()))
                    .map(|d| d.go_hints().to_vec())
                    .unwrap_or_default()
            } else {
                vec![]
            };

            return self.facts.classify_go_return_type(&return_type, &go_hints);
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
        let go_fn_str = self.emit_operand(output, expression, ExpressionContext::value());

        let is_go_module_fn = matches!(
            expression.unwrap_parens(),
            Expression::DotAccess { expression, .. }
            if expression.get_type().as_import_namespace()
                .is_some_and(|m| m.starts_with(go_name::GO_IMPORT_PREFIX))
        );
        if is_go_module_fn {
            return go_fn_str;
        }

        if is_order_sensitive(expression) {
            self.hoist_tmp_value(output, "fn", &go_fn_str)
        } else {
            go_fn_str
        }
    }

    pub(crate) fn build_wrapper_params(&mut self, params: &[Type]) -> (Vec<String>, Vec<String>) {
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
        let fn_type = expression.get_type();
        let (params, return_type) = match fn_type.unwrap_forall() {
            Type::Function {
                params,
                return_type,
                ..
            } => (params.clone(), (**return_type).clone()),
            _ => return self.emit_operand(output, expression, ExpressionContext::value()),
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
        self.requirements.require_stdlib();

        let fn_type = expression.get_type();
        let (params, return_type) = match fn_type.unwrap_forall() {
            Type::Function {
                params,
                return_type,
                ..
            } => (params.clone(), (**return_type).clone()),
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
                self.emit_comma_ok_wrapping(&mut body, &call_str, &return_type, true)
            }
            GoCallStrategy::NullableReturn => {
                let raw_var = self.hoist_tmp_value(&mut body, "raw", &call_str);
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
            GoCallStrategy::Sentinel { value } => {
                self.emit_sentinel_wrapping(&mut body, &call_str, &return_type, *value)
            }
        };

        write_line!(body, "return {}", result_var);

        format!(
            "func({}) {} {{\n{}}}",
            param_strs.join(", "),
            ret_ty_str,
            body
        )
    }
}
