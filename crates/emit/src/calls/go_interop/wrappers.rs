use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::calls::go_interop::build_tuple_literal;
use crate::calls::go_interop::go_qualified_name;
use crate::calls::go_interop::is_go_receiver;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{
    Fallible, FalliblePlanner, PARTIAL_BOTH_CTOR, PARTIAL_ERR_CTOR, PARTIAL_OK_CTOR,
};
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::plan::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement};
use crate::write_line;
use syntax::ast::Expression;
use syntax::types::Type;

use super::GoCallStrategy;

#[derive(Clone, Copy)]
pub(crate) enum WrapperTarget<'a> {
    /// Allocate a fresh `var slot T` and write `slot = X` per branch.
    FreshSlot,
    /// Write `slot = X` per branch into the caller-provided slot name.
    Slot(&'a str),
    /// Emit `return X` per branch; caller skips its trailing return.
    Return,
}

/// How a fallible callee presents a tuple `Ok`/`Some` payload at the Go
/// boundary: as separate return values (`Flattened`, e.g. a Go-imported
/// `(A, B, error)`) or already bundled into one tuple value (`Packed`, the
/// Lisette `(Tuple_n[...], error)` ABI).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TupleReturnLayout {
    Packed,
    Flattened,
}

impl TupleReturnLayout {
    pub(crate) fn is_flattened(self) -> bool {
        matches!(self, TupleReturnLayout::Flattened)
    }
}

/// `Some(slot_name)` when the wrapper wrote into a fresh or named slot; `None`
/// when it wrote a `return` statement and the caller should not emit its own.
pub(crate) type WrapperOutcome = Option<String>;

pub(super) enum ResolvedSink {
    Slot(String),
    Return,
}

/// `slot = value` / `return value` as a `RawGo` leaf.
pub(super) fn leaf_statement(sink: &ResolvedSink, value: &str) -> LoweredStatement {
    match sink {
        ResolvedSink::Slot(name) => LoweredStatement::RawGo(format!("{} = {}\n", name, value)),
        ResolvedSink::Return => LoweredStatement::RawGo(format!("return {}\n", value)),
    }
}

/// A single-statement branch body for a wrapper-dispatch `If`.
pub(super) fn leaf_block(sink: &ResolvedSink, value: &str) -> LoweredBlock {
    LoweredBlock {
        statements: vec![leaf_statement(sink, value)],
    }
}

impl Planner<'_> {
    /// Prepare a wrapper sink: declare `var slot T` (slot targets) or route
    /// writes to `return`.
    pub(super) fn push_wrapper_slot(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        target: WrapperTarget<'_>,
        type_str: &str,
        name_hint: &'static str,
    ) -> (ResolvedSink, WrapperOutcome) {
        match target {
            WrapperTarget::FreshSlot => {
                let var = self.fresh_var(Some(name_hint));
                self.declare(&var);
                statements.push(LoweredStatement::RawGo(format!(
                    "var {} {}\n",
                    var, type_str
                )));
                (ResolvedSink::Slot(var.clone()), Some(var))
            }
            WrapperTarget::Slot(name) => {
                statements.push(LoweredStatement::RawGo(format!(
                    "var {} {}\n",
                    name, type_str
                )));
                self.declare(name);
                let owned = name.to_string();
                (ResolvedSink::Slot(owned.clone()), Some(owned))
            }
            WrapperTarget::Return => (ResolvedSink::Return, None),
        }
    }

    /// Typed counterpart of `extract_go_returns`.
    fn push_go_returns(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        call_str: &str,
        ok_ty: &Type,
        layout: TupleReturnLayout,
        fx: &mut EmitEffects,
    ) -> (String, String) {
        let mut buffer = String::new();
        let result = self.extract_go_returns(&mut buffer, call_str, ok_ty, layout, fx);
        if !buffer.is_empty() {
            statements.push(LoweredStatement::RawGo(buffer));
        }
        result
    }

    /// Single-leaf write for wrappers that fold to one constructor expression.
    pub(super) fn push_simple_wrapper_value(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        target: WrapperTarget<'_>,
        name_hint: &'static str,
        value_expr: &str,
    ) -> WrapperOutcome {
        match target {
            WrapperTarget::FreshSlot => {
                Some(self.hoist_tmp_value_statement(statements, name_hint, value_expr))
            }
            WrapperTarget::Slot(name) => {
                self.declare(name);
                statements.push(LoweredStatement::RawGo(format!(
                    "{} := {}\n",
                    name, value_expr
                )));
                Some(name.to_string())
            }
            WrapperTarget::Return => {
                statements.push(LoweredStatement::RawGo(format!("return {}\n", value_expr)));
                None
            }
        }
    }
}

impl Planner<'_> {
    pub(super) fn lower_go_tuple_call_wrapped(
        &mut self,
        call_expression: &Expression,
        arity: usize,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::Call { ty, .. } = call_expression else {
            unreachable!("lower_go_tuple_call_wrapped called with non-call expression");
        };

        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value(), fx);

        let temp_vars = self.create_temp_vars("ret", arity);
        setup.push(LoweredStatement::RawGo(format!(
            "{} := {}\n",
            temp_vars.join(", "),
            call_str
        )));

        let constructor = build_tuple_literal(&temp_vars, ty, fx);
        let tuple = self.hoist_tmp_value_statement(&mut setup, "tup", &constructor);
        (setup, tuple)
    }

    pub(super) fn lower_go_partial_call_wrapped(
        &mut self,
        call_expression: &Expression,
        partial_ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        fx.require_stdlib();
        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value(), fx);
        let (wrap_setup, outcome) = self.lower_partial_wrapping(
            &call_str,
            partial_ty,
            TupleReturnLayout::Flattened,
            WrapperTarget::FreshSlot,
            fx,
        );
        setup.extend(wrap_setup);
        (setup, outcome.expect("wrapper produced no slot"))
    }

    /// String-context bridge over `lower_partial_wrapping`.
    pub(crate) fn emit_partial_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        partial_ty: &Type,
        layout: TupleReturnLayout,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> WrapperOutcome {
        let (statements, outcome) =
            self.lower_partial_wrapping(call_str, partial_ty, layout, target, fx);
        output.push_str(&Renderer.render_setup(&statements));
        outcome
    }

    /// Lower a `(T, error)` Go return into a tagged `Partial`.
    pub(crate) fn lower_partial_wrapping(
        &mut self,
        call_str: &str,
        partial_ty: &Type,
        layout: TupleReturnLayout,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let ok_ty = partial_ty.ok_type();
        let err_ty = partial_ty.err_type();
        let ok_ty_str = self.go_type_string(&ok_ty, fx);
        let err_ty_str = self.go_type_string(&err_ty, fx);
        let pkg = go_name::GO_STDLIB_PKG;

        let mut statements = Vec::new();
        let (err_var, val_var) =
            self.push_go_returns(&mut statements, call_str, &ok_ty, layout, fx);
        let nil_check = self.partial_ok_nil_check(&ok_ty, &val_var, fx);

        let type_params = format!("{}, {}", ok_ty_str, err_ty_str);
        let result_ty_str = format!("{pkg}.Partial[{type_params}]");
        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let both = format!("{PARTIAL_BOTH_CTOR}[{type_params}]({val_var}, {err_var})");

        let then_body = if let Some(check) = &nil_check {
            let inner = IfPlan {
                directive: String::new(),
                condition_setup: String::new(),
                condition: check.clone(),
                then_body: leaf_block(
                    &sink,
                    &format!("{PARTIAL_ERR_CTOR}[{type_params}]({err_var})"),
                ),
                else_arm: ElseArm::Else {
                    body: leaf_block(&sink, &both),
                    inline: false,
                },
            };
            LoweredBlock {
                statements: vec![LoweredStatement::If(inner)],
            }
        } else {
            leaf_block(&sink, &both)
        };

        let else_arm = ElseArm::Else {
            body: leaf_block(
                &sink,
                &format!("{PARTIAL_OK_CTOR}[{type_params}]({})", val_var),
            ),
            inline: false,
        };

        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: format!("{} != nil", err_var),
            then_body,
            else_arm,
        }));
        (statements, outcome)
    }

    /// Nil check for a `Partial` ok value; `None` when the type cannot be nil.
    fn partial_ok_nil_check(
        &mut self,
        ok_ty: &Type,
        val: &str,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if self.facts.as_interface(ok_ty).is_some() {
            fx.require_stdlib();
            return Some(format!("lisette.IsNilInterface({val})"));
        }
        let peeled = self.facts.peel_alias(ok_ty);
        let nilable = self.facts.is_nilable_go_type(ok_ty)
            || peeled.is_map()
            || peeled.is_slice()
            || peeled.is_channel();
        nilable.then(|| format!("{val} == nil"))
    }

    pub(super) fn lower_go_result_call_wrapped(
        &mut self,
        call_expression: &Expression,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        fx.require_stdlib();
        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value(), fx);
        let (wrap_setup, outcome) = self.lower_result_wrapping(
            &call_str,
            result_ty,
            TupleReturnLayout::Flattened,
            WrapperTarget::FreshSlot,
            fx,
        );
        setup.extend(wrap_setup);
        (setup, outcome.expect("wrapper produced no slot"))
    }

    /// String-context bridge over `lower_result_wrapping`.
    pub(crate) fn emit_result_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        result_ty: &Type,
        layout: TupleReturnLayout,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> WrapperOutcome {
        let (statements, outcome) =
            self.lower_result_wrapping(call_str, result_ty, layout, target, fx);
        output.push_str(&Renderer.render_setup(&statements));
        outcome
    }

    /// Lower a `(T, error)` Go return into a tagged `Result`.
    pub(crate) fn lower_result_wrapping(
        &mut self,
        call_str: &str,
        result_ty: &Type,
        layout: TupleReturnLayout,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let fallible = Fallible::from_type(result_ty).expect("Result type expected");

        if fallible.ok_ty().is_unit() {
            return self.lower_unit_result_wrapping(call_str, &fallible, target, fx);
        }

        let mut statements = Vec::new();
        let ok_ty = fallible.ok_ty();
        let (err_var, ok_val) = self.push_go_returns(&mut statements, call_str, ok_ty, layout, fx);

        let result_ty_str = {
            let mut fe = FalliblePlanner::new(self, &fallible, fx);
            fe.full_type_string()
        };

        let interface_id = self.facts.as_interface(ok_ty);
        let needs_nil_guard = ok_ty.is_ref()
            || interface_id
                .as_deref()
                .is_some_and(|id| id != go_name::PRELUDE_ERROR_ID);

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let err_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible, fx);
            fe.emit_failure(Some(&err_var))
        };
        let then_body = leaf_block(&sink, &err_wrapper);

        let else_arm = if needs_nil_guard {
            let nil_check = if ok_ty.is_tuple() {
                format!("{}.First", ok_val)
            } else {
                ok_val.clone()
            };
            let nil_condition = if self.facts.is_interface(ok_ty) {
                format!("lisette.IsNilInterface({})", nil_check)
            } else {
                format!("{} == nil", nil_check)
            };
            fx.require_errors();
            let nil_err = {
                let mut fe = FalliblePlanner::new(self, &fallible, fx);
                fe.emit_failure(Some("errors.New(\"unexpected nil\")"))
            };
            let ok_wrapper = {
                let mut fe = FalliblePlanner::new(self, &fallible, fx);
                fe.emit_success(&ok_val)
            };
            ElseArm::ElseIf(Box::new(IfPlan {
                directive: String::new(),
                condition_setup: String::new(),
                condition: nil_condition,
                then_body: leaf_block(&sink, &nil_err),
                else_arm: ElseArm::Else {
                    body: leaf_block(&sink, &ok_wrapper),
                    inline: false,
                },
            }))
        } else {
            let ok_wrapper = {
                let mut fe = FalliblePlanner::new(self, &fallible, fx);
                fe.emit_success(&ok_val)
            };
            ElseArm::Else {
                body: leaf_block(&sink, &ok_wrapper),
                inline: false,
            }
        };

        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: format!("{} != nil", err_var),
            then_body,
            else_arm,
        }));
        (statements, outcome)
    }

    fn lower_unit_result_wrapping(
        &mut self,
        call_str: &str,
        fallible: &Fallible,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let err_var = self.hoist_tmp_value_statement(&mut statements, "ret", call_str);

        let result_ty_str = {
            let mut fe = FalliblePlanner::new(self, fallible, fx);
            fe.full_type_string()
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let err_wrapper = {
            let mut fe = FalliblePlanner::new(self, fallible, fx);
            fe.emit_failure(Some(&err_var))
        };
        let then_body = leaf_block(&sink, &err_wrapper);

        let ok_wrapper = {
            let mut fe = FalliblePlanner::new(self, fallible, fx);
            fe.emit_success("struct{}{}")
        };
        let else_arm = ElseArm::Else {
            body: leaf_block(&sink, &ok_wrapper),
            inline: false,
        };

        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: format!("{} != nil", err_var),
            then_body,
            else_arm,
        }));
        (statements, outcome)
    }

    /// Destructure a Go multi-return into error and value temps. A `Flattened`
    /// tuple ok type (Go-imported `(T1, ..., Tn, error)`) gets N+1 temps and a
    /// rebuilt Lisette tuple; a `Packed` one (Lisette `(Tuple_n[...], error)`)
    /// gets 2 temps like any other ok type.
    fn extract_go_returns(
        &mut self,
        output: &mut String,
        call_str: &str,
        ok_ty: &Type,
        layout: TupleReturnLayout,
        fx: &mut EmitEffects,
    ) -> (String, String) {
        if layout.is_flattened()
            && let Type::Tuple(elements) = ok_ty
        {
            let tuple_arity = elements.len();
            let temp_vars = self.create_temp_vars("ret", tuple_arity + 1);
            write_line!(output, "{} := {}", temp_vars.join(", "), call_str);
            let tuple_var = self.emit_tuple_from_vars(output, &temp_vars[..tuple_arity], ok_ty, fx);
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

    pub(crate) fn classify_go_fn_value(&self, expression: &Expression) -> Option<GoCallStrategy> {
        let inner = expression.unwrap_parens();

        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
            && is_go_receiver(receiver)
        {
            let fn_type = expression.get_type();
            let Type::Function(f) = fn_type.unwrap_forall() else {
                return None;
            };
            let return_type = f.return_type.clone();

            let go_hints = if let Expression::DotAccess {
                expression: receiver_expression,
                member,
                ..
            } = inner
            {
                go_qualified_name(receiver_expression, member)
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
            && is_go_receiver(receiver)
        {
            return self.has_go_array_return(receiver, member);
        }
        false
    }

    fn hoist_go_fn_if_needed(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> String {
        let go_fn_str = self.emit_operand(output, expression, ExpressionContext::value(), fx);

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

    pub(crate) fn build_wrapper_params(
        &mut self,
        params: &[Type],
        fx: &mut EmitEffects,
    ) -> (Vec<String>, Vec<String>) {
        let mut param_strs = Vec::new();
        let mut arg_names = Vec::new();
        let last_index = params.len().saturating_sub(1);
        for (i, param_ty) in params.iter().enumerate() {
            let name = format!("arg{}", i);
            let ty_str = self.go_type_string(param_ty, fx);
            param_strs.push(format!("{} {}", name, ty_str));
            if i == last_index && param_ty.get_name() == Some("VarArgs") {
                arg_names.push(format!("{}...", name));
            } else {
                arg_names.push(name);
            }
        }
        (param_strs, arg_names)
    }

    /// Common wrapper-builder prologue: returns `(return_type, param_strs,
    /// call_str)` for a go-fn expression, or `None` for non-function types.
    fn wrapper_call_parts(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> Option<(Type, Vec<String>, String)> {
        let fn_type = expression.get_type();
        let (params, return_type) = match fn_type.unwrap_forall() {
            Type::Function(f) => (f.params.clone(), (*f.return_type).clone()),
            _ => return None,
        };
        let go_fn_str = self.hoist_go_fn_if_needed(output, expression, fx);
        let (param_strs, arg_names) = self.build_wrapper_params(&params, fx);
        let call_str = format!("{}({})", go_fn_str, arg_names.join(", "));
        Some((return_type, param_strs, call_str))
    }

    pub(crate) fn emit_array_return_wrapper(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> String {
        let Some((return_type, param_strs, call_str)) =
            self.wrapper_call_parts(output, expression, fx)
        else {
            return self.emit_operand(output, expression, ExpressionContext::value(), fx);
        };

        let ret_ty_str = self.go_type_string(&return_type, fx);

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
        fx: &mut EmitEffects,
    ) -> String {
        fx.require_stdlib();

        let (return_type, param_strs, call_str) = self
            .wrapper_call_parts(output, expression, fx)
            .expect("expected function type");

        let ret_ty_str = self.go_type_string(&return_type, fx);

        let mut body = String::new();
        let outcome = match strategy {
            GoCallStrategy::Result => self.emit_result_wrapping(
                &mut body,
                &call_str,
                &return_type,
                TupleReturnLayout::Flattened,
                WrapperTarget::Return,
                fx,
            ),
            GoCallStrategy::CommaOk => self.emit_comma_ok_wrapping(
                &mut body,
                &call_str,
                &return_type,
                TupleReturnLayout::Flattened,
                WrapperTarget::Return,
                fx,
            ),
            GoCallStrategy::NullableReturn => {
                let raw_var = self.hoist_tmp_value(&mut body, "raw", &call_str);
                self.emit_nil_check_option_wrap(
                    &mut body,
                    &raw_var,
                    &return_type,
                    WrapperTarget::Return,
                    fx,
                )
            }
            GoCallStrategy::Tuple { arity } => {
                let temp_vars = self.create_temp_vars("ret", *arity);
                write_line!(body, "{} := {}", temp_vars.join(", "), call_str);
                let tuple_str = self.emit_tuple_from_vars(&mut body, &temp_vars, &return_type, fx);
                Some(tuple_str)
            }
            GoCallStrategy::Partial => self.emit_partial_wrapping(
                &mut body,
                &call_str,
                &return_type,
                TupleReturnLayout::Flattened,
                WrapperTarget::Return,
                fx,
            ),
            GoCallStrategy::Sentinel { value } => self.emit_sentinel_wrapping(
                &mut body,
                &call_str,
                &return_type,
                *value,
                WrapperTarget::Return,
                fx,
            ),
        };

        if let Some(result_var) = outcome {
            write_line!(body, "return {}", result_var);
        }

        format!(
            "func({}) {} {{\n{}}}",
            param_strs.join(", "),
            ret_ty_str,
            body
        )
    }

    /// Closure that bundles a raw `(T1, T2, error)` return into the slot's `(Tuple, error)` shape.
    pub(crate) fn emit_go_fn_lowered_tuple_adapter(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) -> String {
        fx.require_stdlib();

        let (return_type, param_strs, call_str) = self
            .wrapper_call_parts(output, expression, fx)
            .expect("expected function type");

        let ok_ty = return_type.ok_type();
        let err_ty = return_type.err_type();
        let ret_ty_str = format!(
            "({}, {})",
            self.go_type_string(&ok_ty, fx),
            self.go_type_string(&err_ty, fx)
        );
        let arity = ok_ty.tuple_arity().expect("tuple ok type");

        let mut body = String::new();
        let temp_vars = self.create_temp_vars("ret", arity + 1);
        write_line!(body, "{} := {}", temp_vars.join(", "), call_str);
        let tuple_str = self.emit_tuple_from_vars(&mut body, &temp_vars[..arity], &ok_ty, fx);
        write_line!(body, "return {}, {}", tuple_str, temp_vars[arity]);

        format!(
            "func({}) {} {{\n{}}}",
            param_strs.join(", "),
            ret_ty_str,
            body
        )
    }
}
