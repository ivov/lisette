mod nullable;
mod wrappers;

pub(crate) use wrappers::{NilGuard, WrapperTarget};

use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi};
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    /// Lower a raw callable result through its canonical physical ABI.
    pub(crate) fn lower_abi_wrapped_call(
        &mut self,
        call_expression: &Expression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
    ) -> ValuePlan {
        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value());
        let (wrap, value) = self.lower_abi_to_tagged(&call_str, abi, result_ty);
        setup.extend(wrap);
        value_plan_from_statements(setup, value)
    }

    pub(crate) fn lower_abi_wrapping(
        &mut self,
        call_str: &str,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, Option<String>) {
        match abi {
            CallableReturnAbi::Tagged
            | CallableReturnAbi::Direct
            | CallableReturnAbi::Tuple { .. } => {
                unreachable!("direct and tuple results do not use a scalar wrapper")
            }
            CallableReturnAbi::Result { payload, .. } => {
                self.require_stdlib();
                self.lower_result_wrapping(call_str, result_ty, *payload, target)
            }
            CallableReturnAbi::Partial { payload } => {
                self.require_stdlib();
                self.lower_partial_wrapping(call_str, result_ty, *payload, target)
            }
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::CommaOk,
                payload,
            } => self.lower_comma_ok_wrapping(call_str, result_ty, *payload, target),
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::Nullable,
                ..
            } => {
                let mut statements = Vec::new();
                let raw_var = self.hoist_tmp_value_statement(&mut statements, "raw", call_str);
                let (wrap, outcome) = self.lower_nil_check_option_wrap(&raw_var, result_ty, target);
                statements.extend(wrap);
                (statements, outcome)
            }
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::Sentinel(value),
                ..
            } => self.lower_sentinel_wrapping(call_str, result_ty, *value, target),
        }
    }

    pub(crate) fn lower_abi_wrapped_call_to(
        &mut self,
        expression: &Expression,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> Option<Vec<LoweredStatement>> {
        if matches!(
            abi,
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct | CallableReturnAbi::Tuple { .. }
        ) {
            return None;
        }
        let (mut statements, call_str) =
            self.lower_call(expression, None, ExpressionContext::value());
        let (wrap, _) = self.lower_abi_wrapping(&call_str, abi, result_ty, target);
        statements.extend(wrap);
        Some(statements)
    }

    fn has_go_hint(&self, receiver_expression: &Expression, member: &str, hint: &str) -> bool {
        let Some(qualified_name) = go_qualified_name(receiver_expression, member) else {
            return false;
        };

        self.facts
            .definition(qualified_name.as_str())
            .map(|definition| definition.go_hints().iter().any(|s| s == hint))
            .unwrap_or(false)
    }

    pub(crate) fn has_go_array_return(
        &self,
        receiver_expression: &Expression,
        member: &str,
    ) -> bool {
        self.has_go_hint(receiver_expression, member, "array_return")
    }

    pub(crate) fn emit_go_call_discarded(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        call_expression: &Expression,
    ) -> Option<String> {
        let Expression::Call {
            expression: callee, ..
        } = call_expression
        else {
            return None;
        };

        let plan = self.plan_call(call_expression)?;

        let has_array_return = if let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = callee.unwrap_parens()
            && is_go_receiver(receiver_expression)
        {
            self.has_go_array_return(receiver_expression, member)
        } else {
            false
        };

        if plan.resolved.abi.result.is_passthrough() && !has_array_return {
            return None;
        }

        let mut ctx = ExpressionContext::value();
        if has_array_return {
            ctx = ctx.with_raw_go_array_return();
        }
        let (call_setup, call_str) = self.lower_call(call_expression, None, ctx);
        setup.extend(call_setup);

        Some(call_str)
    }

    pub(crate) fn create_temp_vars(&mut self, hint: &str, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                let v = self.fresh_var(Some(hint));
                self.declare(&v);
                v
            })
            .collect()
    }

    pub(crate) fn emit_tuple_from_vars(
        &mut self,
        output: &mut String,
        vars: &[String],
        tuple_ty: &Type,
    ) -> String {
        let constructor = build_tuple_literal(self, vars, tuple_ty);
        self.hoist_tmp_value(output, "tup", &constructor)
    }

    /// Structured counterpart of `emit_tuple_from_vars`.
    pub(crate) fn plan_tuple_from_vars(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        vars: &[String],
        tuple_ty: &Type,
    ) -> String {
        let constructor = build_tuple_literal(self, vars, tuple_ty);
        self.hoist_tmp_value_statement(statements, "tup", &constructor)
    }
}

pub(crate) fn go_qualified_name(receiver_expression: &Expression, member: &str) -> Option<String> {
    let ty = receiver_expression.get_type();

    if let Some(module_path) = ty.as_import_namespace() {
        return Some(format!("{}.{}", module_path, member));
    }

    if let Type::Nominal { id, .. } = ty.strip_refs()
        && go_name::is_go_import(&id)
    {
        return Some(format!("{}.{}", id, member));
    }

    None
}

pub(crate) fn is_go_receiver(expression: &Expression) -> bool {
    let ty = expression.get_type();

    if let Some(module_id) = ty.as_import_namespace()
        && module_id.starts_with(go_name::GO_IMPORT_PREFIX)
    {
        return true;
    }

    if let Type::Nominal { id, .. } = ty.strip_refs()
        && go_name::is_go_import(&id)
    {
        return true;
    }

    false
}

pub(super) fn build_tuple_literal(planner: &Planner, vars: &[String], _tuple_ty: &Type) -> String {
    planner.require_stdlib();
    format!("lisette.MakeTuple{}({})", vars.len(), vars.join(", "))
}
