use crate::Emitter;
use crate::control_flow::fallible::{
    Fallible, FallibleEmitter, OPTION_SOME_TAG, PARTIAL_BOTH_CTOR, PARTIAL_ERR_TAG,
    PARTIAL_OK_CTOR, PARTIAL_OK_TAG, RESULT_OK_TAG,
};
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::types::abi::{AbiShape, tuple_element_types};
use crate::utils::optimize_region;
use crate::write_line;
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
        let Expression::Call { ty, .. } = call_expression else {
            unreachable!("emit_go_tuple_call_wrapped called with non-call expression");
        };

        let call_str = self.emit_call(output, call_expression, None);

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

        let call_str = self.emit_call(output, call_expression, None);
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
        self.flags.needs_stdlib = true;

        let call_str = self.emit_call(output, call_expression, None);
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

    /// Wrap a lowered-callee `call_str` into the Lisette tagged shape declared
    /// by `result_ty`.
    pub(crate) fn emit_callee_abi_wrapping(
        &mut self,
        output: &mut String,
        shape: &AbiShape,
        call_str: &str,
        result_ty: &Type,
    ) -> String {
        match shape {
            AbiShape::PartialTuple => self.emit_partial_wrapping(output, call_str, result_ty),
            AbiShape::CommaOk => self.emit_comma_ok_wrapping(output, call_str, result_ty, false),
            AbiShape::NullableReturn => {
                let raw_var = self.fresh_var(Some("raw"));
                self.declare(&raw_var);
                write_line!(output, "{} := {}", raw_var, call_str);
                self.emit_nil_check_option_wrap(output, &raw_var, result_ty)
            }
            AbiShape::ResultTuple | AbiShape::BareError => {
                self.emit_result_wrapping(output, call_str, result_ty)
            }
            AbiShape::Tuple { arity } => {
                let temps = self.create_temp_vars("ret", *arity);
                write_line!(output, "{} := {}", temps.join(", "), call_str);
                let slot_tys = tuple_element_types(&self.peel_alias(result_ty));
                let wrapped: Vec<String> = temps
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        slot_tys
                            .get(i)
                            .filter(|slot_ty| self.is_nullable_option(slot_ty))
                            .map(|slot_ty| self.emit_nil_check_option_wrap(output, v, slot_ty))
                            .unwrap_or_else(|| v.clone())
                    })
                    .collect();
                self.emit_tuple_from_vars(output, &wrapped, result_ty)
            }
        }
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
            if expression.get_type().as_import_namespace()
                .is_some_and(|m| m.starts_with(go_name::GO_IMPORT_PREFIX))
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
        let fn_type = expression.get_type();
        let (params, return_type) = match fn_type.unwrap_forall() {
            Type::Function {
                params,
                return_type,
                ..
            } => (params.clone(), (**return_type).clone()),
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
            GoCallStrategy::Sentinel { value } => {
                self.emit_sentinel_wrapping(&mut body, &call_str, &return_type, *value)
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

    pub(crate) fn emit_return_adapter(
        &mut self,
        inner_call: &str,
        lisette_return_type: &Type,
    ) -> Option<(String, String)> {
        let return_type = lisette_return_type;
        self.flags.needs_stdlib = true;

        if return_type.is_result() {
            return Some(self.emit_result_return_adapter(inner_call, return_type));
        }
        if return_type.is_partial() {
            return Some(self.emit_partial_return_adapter(inner_call, return_type));
        }
        if return_type.is_option() {
            return Some(self.emit_option_return_adapter(inner_call, return_type));
        }
        if return_type.tuple_arity().is_some_and(|n| n >= 2) {
            return self.emit_tuple_return_adapter(inner_call, return_type);
        }
        None
    }

    /// `Result<(), error>` → `error`; `Result<T, error>` → `(T, error)`.
    fn emit_result_return_adapter(
        &mut self,
        inner_call: &str,
        return_type: &Type,
    ) -> (String, String) {
        let ok_ty = return_type.ok_type();
        let err_ty = return_type.err_type();
        let err_ty_str = self.go_type_as_string(&err_ty);
        let res = self.fresh_var(Some("res"));
        self.declare(&res);

        let mut b = format!("{res} := {inner_call}\n");
        let ok_tag = RESULT_OK_TAG;
        if ok_ty.is_unit() {
            write_line!(
                b,
                "if {res}.Tag == {ok_tag} {{\nreturn nil\n}}\nreturn {res}.ErrVal"
            );
            return (err_ty_str, b);
        }
        let ok_ty_str = self.go_type_as_string(&ok_ty);
        write_line!(
            b,
            "if {res}.Tag == {ok_tag} {{\nreturn {res}.OkVal, nil\n}}\n\
             return *new({ok_ty_str}), {res}.ErrVal"
        );
        (format!("({ok_ty_str}, {err_ty_str})"), b)
    }

    /// `Partial<T, error>` → `(T, error)`, distinguishing Ok/Err/both branches.
    fn emit_partial_return_adapter(
        &mut self,
        inner_call: &str,
        return_type: &Type,
    ) -> (String, String) {
        let ok_ty = return_type.ok_type();
        let err_ty = return_type.err_type();
        let ok_ty_str = self.go_type_as_string(&ok_ty);
        let err_ty_str = self.go_type_as_string(&err_ty);
        let res = self.fresh_var(Some("res"));
        self.declare(&res);

        let b = format!(
            "{res} := {inner_call}\n\
             if {res}.Tag == {PARTIAL_OK_TAG} {{\nreturn {res}.OkVal, nil\n}}\n\
             if {res}.Tag == {PARTIAL_ERR_TAG} {{\nreturn *new({ok_ty_str}), {res}.ErrVal\n}}\n\
             return {res}.OkVal, {res}.ErrVal\n"
        );
        (format!("({ok_ty_str}, {err_ty_str})"), b)
    }

    /// `Option<fn>`/`Option<Ref<T>>`/`Option<Interface>` → bare nilable Go type
    /// (collapsed because Go's nil already encodes absence). Other payloads use
    /// the Go-idiomatic `(T, bool)` comma-ok convention.
    fn emit_option_return_adapter(
        &mut self,
        inner_call: &str,
        return_type: &Type,
    ) -> (String, String) {
        let inner = return_type.ok_type();
        let some_tag = OPTION_SOME_TAG;
        let opt = self.fresh_var(Some("opt"));
        self.declare(&opt);

        let is_nilable = self.is_nilable_go_type(&inner);
        if is_nilable {
            let go_ret = self.go_type_as_string(&inner);
            let b = format!(
                "{opt} := {inner_call}\n\
                 if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal\n}}\n\
                 return nil\n"
            );
            return (go_ret, b);
        }

        let inner_ty_str = self.go_type_as_string(&inner);
        let b = format!(
            "{opt} := {inner_call}\n\
             if {opt}.Tag == {some_tag} {{\nreturn {opt}.SomeVal, true\n}}\n\
             return *new({inner_ty_str}), false\n"
        );
        (format!("({inner_ty_str}, bool)"), b)
    }

    /// Arity-2+ tuple → Go multi-return. Each slot recurses through
    /// `emit_return_adapter`, wrapping in an IIFE when the slot itself needs
    /// adapter-style unwrapping. Returns `None` only if the resolved type
    /// isn't actually a tuple/constructor shape.
    fn emit_tuple_return_adapter(
        &mut self,
        inner_call: &str,
        return_type: &Type,
    ) -> Option<(String, String)> {
        let tuple_params: Vec<Type> = match return_type {
            Type::Tuple(elements) => elements.clone(),
            Type::Nominal { params, .. } => params.clone(),
            _ => return None,
        };
        let arity = tuple_params.len();
        let tup = self.fresh_var(Some("tup"));
        self.declare(&tup);

        let mut body = format!("{tup} := {inner_call}\n");
        let mut ret_types: Vec<String> = Vec::with_capacity(arity);
        let mut field_exprs: Vec<String> = Vec::with_capacity(arity);

        for (i, slot_ty) in tuple_params.iter().enumerate() {
            let raw_field = format!("{tup}.{}", TUPLE_FIELDS[i]);
            match self.emit_return_adapter(&raw_field, slot_ty) {
                Some((inner_ret, inner_body)) => {
                    let sub = self.fresh_var(Some("sub"));
                    self.declare(&sub);
                    body.push_str(&format!(
                        "{sub} := func() {inner_ret} {{\n{inner_body}}}()\n"
                    ));
                    field_exprs.push(sub);
                    ret_types.push(inner_ret);
                }
                None => {
                    ret_types.push(self.go_type_as_string(slot_ty));
                    field_exprs.push(raw_field);
                }
            }
        }

        body.push_str(&format!("return {}\n", field_exprs.join(", ")));
        Some((format!("({})", ret_types.join(", ")), body))
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

        let return_type = return_type.as_ref();

        let (param_strs, arg_names) = self.build_wrapper_params(params);
        let params_str = param_strs.join(", ");

        let cb_var = self.fresh_var(Some("cb"));
        self.declare(&cb_var);
        write_line!(output, "{} := {}", cb_var, fn_value);

        let mut prelude = String::new();
        let inner_args: Vec<String> = arg_names
            .iter()
            .zip(params.iter())
            .map(|(name, param_ty)| self.lower_arg_to_tagged(&mut prelude, name, param_ty))
            .collect();

        let call_str = format!("{}({})", cb_var, inner_args.join(", "));

        // Option<fn> adaptation only fires in interface-method shims. Here
        // a closure-valued Option means the caller owns the nil check.
        if let Type::Nominal { id, params: ps, .. } = return_type
            && id == "Option"
            && let Some(inner) = ps.first()
            && matches!(inner.unwrap_forall(), Type::Function { .. })
        {
            return fn_value.to_string();
        }

        let Some((go_ret, body)) = self.emit_return_adapter(&call_str, return_type) else {
            return fn_value.to_string();
        };

        format!("func({params_str}) {go_ret} {{\n{prelude}{body}}}")
    }

    /// Convert a fn-typed wrapper arg from lowered Go ABI back to tagged for
    /// the inner call. Identity for non-fn args and for fn args with no
    /// lowered return.
    pub(crate) fn lower_arg_to_tagged(
        &mut self,
        prelude: &mut String,
        arg_name: &str,
        param_ty: &Type,
    ) -> String {
        let unwrapped = param_ty.unwrap_forall();
        let Type::Function {
            params: inner_params,
            return_type: inner_ret,
            ..
        } = unwrapped
        else {
            return arg_name.to_string();
        };
        let inner_ret = inner_ret.as_ref();
        let Some(shape) = self.classify_direct_emission(inner_ret) else {
            return arg_name.to_string();
        };

        let (inner_param_strs, inner_arg_names) = self.build_wrapper_params(inner_params);
        let inner_call = format!("{}({})", arg_name, inner_arg_names.join(", "));
        let tagged_ret = self.go_type_as_string(inner_ret);

        let mut body = String::new();
        let result_var = self.emit_callee_abi_wrapping(&mut body, &shape, &inner_call, inner_ret);
        write_line!(body, "return {}", result_var);

        let tagged_var = self.fresh_var(Some("tagged"));
        self.declare(&tagged_var);
        write_line!(
            prelude,
            "{} := func({}) {} {{\n{}}}",
            tagged_var,
            inner_param_strs.join(", "),
            tagged_ret,
            body
        );
        tagged_var
    }
}
