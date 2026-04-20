use crate::Emitter;
use crate::control_flow::fallible::{Fallible, FallibleEmitter, OPTION_SOME_TAG};
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

impl Emitter<'_> {
    pub(super) fn emit_go_option_call_wrapped(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        option_ty: &Type,
    ) -> String {
        let call_str = self.emit_call(output, call_expression, None);
        self.emit_comma_ok_wrapping(output, &call_str, option_ty)
    }

    pub(super) fn emit_comma_ok_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        option_ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        let fallible = Fallible::from_type(option_ty).expect("Option type expected");

        let inner_ty = option_ty.ok_type();
        let inner_tuple_arity = inner_ty.tuple_arity();

        let val_vars = if let Some(arity) = inner_tuple_arity {
            self.create_temp_vars("ret", arity)
        } else {
            self.create_temp_vars("ret", 1)
        };
        let ok_var = self.fresh_var(Some("ret"));
        self.declare(&ok_var);

        let all_vars: Vec<&str> = val_vars
            .iter()
            .map(|s| s.as_str())
            .chain(std::iter::once(ok_var.as_str()))
            .collect();
        write_line!(output, "{} := {}", all_vars.join(", "), call_str);

        let val_expression = if inner_tuple_arity.is_some() {
            self.build_tuple_literal(&val_vars, &inner_ty)
        } else {
            val_vars[0].clone()
        };

        let mut fe = FallibleEmitter::new(self, &fallible);
        let option_ty_str = fe.full_type_string();
        let option_var = fe.emitter.fresh_var(Some("option"));
        fe.emitter.declare(&option_var);

        let condition = if self.is_interface_option(option_ty) {
            format!("{} && !lisette.IsNilInterface({})", ok_var, val_vars[0])
        } else if self.is_nullable_option(option_ty) {
            format!("{} && {} != nil", ok_var, val_vars[0])
        } else {
            ok_var.clone()
        };
        write_line!(output, "var {} {}", option_var, option_ty_str);
        write_line!(output, "if {} {{", condition);

        let mut fe = FallibleEmitter::new(self, &fallible);
        let some_wrapper = fe.emit_success(&val_expression);
        write_line!(output, "{} = {}", option_var, some_wrapper);

        output.push_str("} else {\n");

        let mut fe = FallibleEmitter::new(self, &fallible);
        let none_wrapper = fe.emit_failure(None);
        write_line!(output, "{} = {}", option_var, none_wrapper);

        output.push_str("}\n");

        option_var
    }

    pub(crate) fn emit_nil_check_option_wrap(
        &mut self,
        output: &mut String,
        raw_value: &str,
        option_ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        let fallible = Fallible::from_type(option_ty).expect("Option type expected");

        let mut fe = FallibleEmitter::new(self, &fallible);
        let option_ty_str = fe.full_type_string();
        let option_var = fe.emitter.fresh_var(Some("option"));
        fe.emitter.declare(&option_var);

        let is_interface = self.is_interface_option(option_ty);
        write_line!(output, "var {} {}", option_var, option_ty_str);
        if is_interface {
            write_line!(output, "if !lisette.IsNilInterface({}) {{", raw_value);
        } else {
            write_line!(output, "if {} != nil {{", raw_value);
        }

        let mut fe = FallibleEmitter::new(self, &fallible);
        let some_wrapper = fe.emit_success(raw_value);
        write_line!(output, "{} = {}", option_var, some_wrapper);

        output.push_str("} else {\n");

        let mut fe = FallibleEmitter::new(self, &fallible);
        let none_wrapper = fe.emit_failure(None);
        write_line!(output, "{} = {}", option_var, none_wrapper);

        output.push_str("}\n");

        option_var
    }

    pub(crate) fn maybe_unwrap_go_nullable(
        &mut self,
        output: &mut String,
        value_str: &str,
        value_ty: &Type,
    ) -> String {
        if self.is_nullable_option(value_ty) {
            return self.emit_option_unwrap_to_nullable(output, value_str, value_ty);
        }
        if let Some(elem_option_ty) = self.nullable_collection_element_ty(value_ty) {
            return self.emit_collection_nullable_unwrap(
                output,
                value_str,
                value_ty,
                &elem_option_ty,
            );
        }
        value_str.to_string()
    }

    pub(crate) fn maybe_wrap_go_nullable(
        &mut self,
        output: &mut String,
        value_str: &str,
        value_ty: &Type,
    ) -> String {
        if self.is_nullable_option(value_ty) {
            return self.emit_nil_check_option_wrap(output, value_str, value_ty);
        }
        if let Some(elem_option_ty) = self.nullable_collection_element_ty(value_ty) {
            return self.emit_collection_nullable_wrap(
                output,
                value_str,
                value_ty,
                &elem_option_ty,
            );
        }
        value_str.to_string()
    }

    pub(crate) fn emit_option_unwrap_to_nullable(
        &mut self,
        output: &mut String,
        option_value: &str,
        option_ty: &Type,
    ) -> String {
        let inner_ty = option_ty.ok_type();
        let go_inner_ty = self.go_type_as_string(&inner_ty);

        let opt_var = self.fresh_var(Some("opt"));
        self.declare(&opt_var);
        let unwrapped_var = self.fresh_var(Some("unwrap"));
        self.declare(&unwrapped_var);

        self.flags.needs_stdlib = true;

        write_line!(output, "{} := {}", opt_var, option_value);
        write_line!(output, "var {} {}", unwrapped_var, go_inner_ty);
        write_line!(output, "if {}.Tag == {} {{", opt_var, OPTION_SOME_TAG);
        write_line!(output, "{} = {}.SomeVal", unwrapped_var, opt_var);
        output.push_str("}\n");

        unwrapped_var
    }

    pub(crate) fn emit_collection_nullable_wrap(
        &mut self,
        output: &mut String,
        raw_value: &str,
        collection_ty: &Type,
        elem_option_ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        let lisette_collection_ty = self.go_type_as_string(collection_ty);
        let src_var = self.fresh_var(Some("src"));
        self.declare(&src_var);
        let wrapped_var = self.fresh_var(Some("wrapped"));
        self.declare(&wrapped_var);
        let idx_var = self.fresh_var(Some("i"));
        self.declare(&idx_var);
        let val_var = self.fresh_var(Some("v"));
        self.declare(&val_var);

        write_line!(output, "{} := {}", src_var, raw_value);
        write_line!(
            output,
            "{} := make({}, len({}))",
            wrapped_var,
            lisette_collection_ty,
            src_var
        );

        write_line!(
            output,
            "for {}, {} := range {} {{",
            idx_var,
            val_var,
            src_var
        );

        let fallible = Fallible::from_type(elem_option_ty).expect("Option type expected");

        let is_interface = self.is_interface_option(elem_option_ty);
        if is_interface {
            write_line!(output, "if !lisette.IsNilInterface({}) {{", val_var);
        } else {
            write_line!(output, "if {} != nil {{", val_var);
        }
        let mut fe = FallibleEmitter::new(self, &fallible);
        let some_wrapper = fe.emit_success(&val_var);
        write_line!(output, "{}[{}] = {}", wrapped_var, idx_var, some_wrapper);
        output.push_str("} else {\n");
        let mut fe = FallibleEmitter::new(self, &fallible);
        let none_wrapper = fe.emit_failure(None);
        write_line!(output, "{}[{}] = {}", wrapped_var, idx_var, none_wrapper);
        output.push_str("}\n");

        output.push_str("}\n");

        wrapped_var
    }

    pub(crate) fn emit_collection_nullable_unwrap(
        &mut self,
        output: &mut String,
        lisette_value: &str,
        collection_ty: &Type,
        elem_option_ty: &Type,
    ) -> String {
        self.flags.needs_stdlib = true;

        let resolved = collection_ty.resolve();
        let is_map = resolved.has_name("Map");

        let inner_ty = elem_option_ty.ok_type();
        let go_inner_ty = self.go_type_as_string(&inner_ty);
        let raw_collection_ty = if is_map {
            let params = resolved
                .get_type_params()
                .expect("Map should have type params");
            let key_ty = self.go_type_as_string(&params[0]);
            format!("map[{}]{}", key_ty, go_inner_ty)
        } else {
            format!("[]{}", go_inner_ty)
        };

        let src_var = self.fresh_var(Some("src"));
        self.declare(&src_var);
        let unwrapped_var = self.fresh_var(Some("unwrapped"));
        self.declare(&unwrapped_var);
        let idx_var = self.fresh_var(Some("i"));
        self.declare(&idx_var);
        let val_var = self.fresh_var(Some("v"));
        self.declare(&val_var);

        write_line!(output, "{} := {}", src_var, lisette_value);
        write_line!(
            output,
            "{} := make({}, len({}))",
            unwrapped_var,
            raw_collection_ty,
            src_var
        );

        write_line!(
            output,
            "for {}, {} := range {} {{",
            idx_var,
            val_var,
            src_var
        );

        if is_map {
            write_line!(output, "if {}.Tag == {} {{", val_var, OPTION_SOME_TAG);
            write_line!(
                output,
                "{}[{}] = {}.SomeVal",
                unwrapped_var,
                idx_var,
                val_var
            );
            output.push_str("} else {\n");
            write_line!(output, "{}[{}] = nil", unwrapped_var, idx_var);
            output.push_str("}\n");
        } else {
            write_line!(output, "if {}.Tag == {} {{", val_var, OPTION_SOME_TAG);
            write_line!(
                output,
                "{}[{}] = {}.SomeVal",
                unwrapped_var,
                idx_var,
                val_var
            );
            output.push_str("}\n");
        }

        output.push_str("}\n");

        unwrapped_var
    }

    pub(super) fn emit_go_single_return_option_wrapped(
        &mut self,
        output: &mut String,
        call_expression: &Expression,
        option_ty: &Type,
    ) -> String {
        let call_str = self.emit_call(output, call_expression, None);

        let raw_var = self.fresh_var(Some("raw"));
        self.declare(&raw_var);
        write_line!(output, "{} := {}", raw_var, call_str);

        self.emit_nil_check_option_wrap(output, &raw_var, option_ty)
    }
}
