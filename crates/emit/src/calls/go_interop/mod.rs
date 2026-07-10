mod nullable;
mod wrappers;

pub(crate) use wrappers::{NilGuard, WrapperTarget};

use crate::Planner;
use crate::abi::callable::{CallableAbi, CallableReturnAbi, OptionReturnAbi};
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::SlotOrigin;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    /// Lower a raw callable result through its canonical physical ABI.
    pub(crate) fn lower_go_abi_wrapped_call(
        &mut self,
        call_expression: &Expression,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> ValuePlan {
        if let Some(bridge) = self.go_result_layout_bridge(abi, result_ty) {
            let call = self.lower_call(call_expression, None, ExpressionContext::value());
            return call.map_rendered_as_observable_computed(
                |setup, call_string, _contains_deferred_evaluation| {
                    let (bridge_setup, value) = bridge.lower(self, call_string);
                    setup.extend(bridge_setup);
                    GoExpression::opaque(value)
                },
            );
        }

        let payload_bridge = self.option_result_payload_bridge(abi, result_ty);
        let call_plan = self.lower_call(call_expression, None, ExpressionContext::value());
        call_plan.map_rendered_as_observable_computed(
            |setup, call_string, _contains_deferred_evaluation| {
                let (wrap, value) = if payload_bridge.is_some() {
                    let (wrap, outcome) = self.lower_abi_wrapping_with_payload_bridge(
                        &call_string,
                        &abi.result,
                        result_ty,
                        payload_bridge,
                        WrapperTarget::FreshSlot,
                    );
                    (wrap, outcome.expect("wrapper produced no slot"))
                } else {
                    self.lower_abi_to_tagged(&call_string, &abi.result, result_ty)
                };
                setup.extend(wrap);
                GoExpression::opaque(value)
            },
        )
    }

    pub(crate) fn go_result_layout_bridge(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<CoercionPlan> {
        let target = self.value_layout(result_ty, SlotOrigin::Lisette);
        match abi.result {
            CallableReturnAbi::Direct => {}
            CallableReturnAbi::Option {
                encoding: OptionReturnAbi::Nullable,
                ..
            } => {
                let source_payload = abi.return_layout.option_payload()?;
                let target_payload = target.option_payload()?;
                if source_payload.same_representation(target_payload) {
                    return None;
                }
            }
            _ => return None,
        }
        let bridge = CoercionPlan::bridge(self, &abi.return_layout, &target);
        (!bridge.is_identity()).then_some(bridge)
    }

    pub(crate) fn lower_abi_wrapping(
        &mut self,
        call_str: &str,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> (Vec<LoweredStatement>, Option<String>) {
        self.lower_abi_wrapping_with_payload_bridge(call_str, abi, result_ty, None, target)
    }

    fn lower_abi_wrapping_with_payload_bridge(
        &mut self,
        call_str: &str,
        abi: &CallableReturnAbi,
        result_ty: &Type,
        payload_bridge: Option<CoercionPlan>,
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
            } => {
                self.lower_comma_ok_wrapping(call_str, result_ty, *payload, payload_bridge, target)
            }
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
        abi: &CallableAbi,
        result_ty: &Type,
        target: WrapperTarget<'_>,
    ) -> Option<Vec<LoweredStatement>> {
        if matches!(
            abi.result,
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct | CallableReturnAbi::Tuple { .. }
        ) {
            return None;
        }
        let payload_bridge = self.option_result_payload_bridge(abi, result_ty);
        let (mut statements, call_str) = self
            .lower_call(expression, None, ExpressionContext::value())
            .into_parts();
        let (wrap, _) = self.lower_abi_wrapping_with_payload_bridge(
            &call_str,
            &abi.result,
            result_ty,
            payload_bridge,
            target,
        );
        statements.extend(wrap);
        Some(statements)
    }

    fn option_result_payload_bridge(
        &self,
        abi: &CallableAbi,
        result_ty: &Type,
    ) -> Option<CoercionPlan> {
        if !matches!(abi.result, CallableReturnAbi::Option { .. }) {
            return None;
        }
        let source = abi.return_layout.option_payload()?;
        let target_layout = self.value_layout(result_ty, SlotOrigin::Lisette);
        let target = target_layout.option_payload()?;
        let bridge = CoercionPlan::bridge(self, source, target);
        (!bridge.is_identity()).then_some(bridge)
    }

    pub(crate) fn emit_go_call_discarded(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        call_expression: &Expression,
    ) -> Option<String> {
        let plan = self.plan_call(call_expression)?;
        if plan.resolved.abi.result.is_passthrough() {
            return None;
        }

        let (call_setup, call_str) = self
            .lower_call(call_expression, None, ExpressionContext::value())
            .into_parts();
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
