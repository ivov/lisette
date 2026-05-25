mod nullable;
mod wrappers;

pub(crate) use wrappers::WrapperTarget;

use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::calls::CallBoundary;
use crate::calls::go_interop::wrappers::WrapperOutcome;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Debug, Clone)]
pub(crate) enum GoCallStrategy {
    /// `(T1, T2, ...)` → tuple struct (arity ≥ 2, no error/bool suffix).
    Tuple { arity: usize },
    /// `(T, error)` → `Result<T, Error>`.
    Result,
    /// `(T, bool)` → `Option<T>` (non-nullable or `#[go(comma_ok)]`).
    CommaOk,
    /// Single pointer/interface return → `Option<Ref<T>>` via nil check.
    NullableReturn,
    /// `(T, error)` → `Partial<T, error>` (value and error not exclusive,
    /// e.g. `io.Reader.Read`).
    Partial,
    /// Single `T` return → `Option<T>` via `val != sentinel`.
    Sentinel { value: i64 },
}

impl GoCallStrategy {
    pub(crate) fn is_multi_return(&self) -> bool {
        !matches!(
            self,
            GoCallStrategy::NullableReturn | GoCallStrategy::Sentinel { .. }
        )
    }
}

impl Planner<'_> {
    /// String-context bridge over `lower_go_wrapped_call`.
    pub(crate) fn emit_go_wrapped_call(
        &mut self,
        output: &mut String,
        expression: &Expression,
        strategy: &GoCallStrategy,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let (statements, value) = self.lower_go_wrapped_call(expression, strategy, result_ty, fx);
        output.push_str(&Renderer.render_setup(&statements));
        value
    }

    /// Lower a Go-imported callee through its ABI bridge.
    pub(crate) fn lower_go_wrapped_call(
        &mut self,
        call_expression: &Expression,
        strategy: &GoCallStrategy,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        match strategy {
            GoCallStrategy::Tuple { arity } => {
                self.lower_go_tuple_call_wrapped(call_expression, *arity, fx)
            }
            GoCallStrategy::Result => {
                self.lower_go_result_call_wrapped(call_expression, result_ty, fx)
            }
            GoCallStrategy::Partial => {
                self.lower_go_partial_call_wrapped(call_expression, result_ty, fx)
            }
            GoCallStrategy::CommaOk => {
                self.lower_go_option_call_wrapped(call_expression, result_ty, fx)
            }
            GoCallStrategy::NullableReturn => {
                self.lower_go_single_return_option_wrapped(call_expression, result_ty, fx)
            }
            GoCallStrategy::Sentinel { value } => {
                self.lower_go_sentinel_call_wrapped(call_expression, result_ty, *value, fx)
            }
        }
    }

    /// `emit_go_wrapped_call` writing into `target`. `None` for `Tuple`.
    pub(crate) fn emit_go_wrapped_call_to(
        &mut self,
        output: &mut String,
        expression: &Expression,
        strategy: &GoCallStrategy,
        result_ty: &Type,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> Option<WrapperOutcome> {
        match strategy {
            GoCallStrategy::Tuple { .. } => None,
            GoCallStrategy::Result => {
                let call_str =
                    self.emit_call(output, expression, None, ExpressionContext::value(), fx);
                fx.require_stdlib();
                Some(self.emit_result_wrapping(output, &call_str, result_ty, target, fx))
            }
            GoCallStrategy::CommaOk => {
                let call_str =
                    self.emit_call(output, expression, None, ExpressionContext::value(), fx);
                Some(self.emit_comma_ok_wrapping(output, &call_str, result_ty, true, target, fx))
            }
            GoCallStrategy::NullableReturn => {
                let call_str =
                    self.emit_call(output, expression, None, ExpressionContext::value(), fx);
                let raw_var = self.hoist_tmp_value(output, "raw", &call_str);
                Some(self.emit_nil_check_option_wrap(output, &raw_var, result_ty, target, fx))
            }
            GoCallStrategy::Partial => {
                fx.require_stdlib();
                let call_str =
                    self.emit_call(output, expression, None, ExpressionContext::value(), fx);
                Some(self.emit_partial_wrapping(output, &call_str, result_ty, target, fx))
            }
            GoCallStrategy::Sentinel { value } => {
                let call_str =
                    self.emit_call(output, expression, None, ExpressionContext::value(), fx);
                Some(self.emit_sentinel_wrapping(output, &call_str, result_ty, *value, target, fx))
            }
        }
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
        output: &mut String,
        call_expression: &Expression,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Expression::Call {
            expression: callee, ..
        } = call_expression
        else {
            return None;
        };

        let boundary = self.classify_call(call_expression);

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

        if matches!(boundary, CallBoundary::Plain) && !has_array_return {
            return None;
        }

        let mut ctx = ExpressionContext::value();
        if has_array_return {
            ctx = ctx.with_raw_go_array_return();
        }
        let call_str = self.emit_call(output, call_expression, None, ctx, fx);

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
        fx: &mut EmitEffects,
    ) -> String {
        let constructor = build_tuple_literal(vars, tuple_ty, fx);
        self.hoist_tmp_value(output, "tup", &constructor)
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

pub(super) fn build_tuple_literal(
    vars: &[String],
    _tuple_ty: &Type,
    fx: &mut EmitEffects,
) -> String {
    fx.require_stdlib();
    format!("lisette.MakeTuple{}({})", vars.len(), vars.join(", "))
}
