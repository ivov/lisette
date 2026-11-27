use crate::Planner;
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::calls::bound_value::{BoundPair, SlotPlacement};
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::comma_ok::{CommaOkValueSlot, PairKind};
use crate::lowering::expressions::literals::convert_escape_sequences;
use crate::lowering::expressions::literals::{PercentSigns, sprintf_pieces};
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::Statement;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::names::source_name::SourceName;
use syntax::ast::{Expression, FormatStringPart, Literal};

pub(super) struct WrapMessage {
    format: String,
    args: Vec<GoExpression>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::lowering) struct WrapChain<'a>(Vec<&'a Expression>);

impl WrapChain<'_> {
    pub(in crate::lowering) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub(in crate::lowering) struct FusedCall<'a> {
    pub(super) site: CallSite<'a>,
    pub(super) nil_guard: Option<NilGuard>,
    pub(super) wraps: WrapChain<'a>,
}

impl<'a> FusedCall<'a> {
    pub(in crate::lowering) fn into_plain_site(self) -> Option<CallSite<'a>> {
        (self.wraps.is_empty() && self.nil_guard.is_none()).then_some(self.site)
    }

    pub(in crate::lowering) fn wraps_error(&self) -> bool {
        !self.wraps.is_empty()
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn bind_fused_call<P: SlotPlacement>(
        &mut self,
        call: FusedCall<'_>,
        slot: CommaOkValueSlot,
        carries_value: bool,
        error_name: Option<&SourceName>,
    ) -> BoundPair<P> {
        let FusedCall {
            site,
            nil_guard,
            wraps,
        } = call;
        let (setup, call) = self
            .plan_call_expression(site, None, ExpressionContext::value())
            .into_parts();
        let (message_setup, messages) = self.prepare_wrap_messages(&wraps.0);

        let slot = if message_setup.is_empty() {
            slot
        } else {
            match slot {
                CommaOkValueSlot::Arm(name) => {
                    let name = self.declared_arm_value_name(&name);
                    CommaOkValueSlot::Named(name)
                }
                CommaOkValueSlot::Unused => CommaOkValueSlot::Discarded,
                slot @ CommaOkValueSlot::Named(_)
                | slot @ CommaOkValueSlot::Temp
                | slot @ CommaOkValueSlot::Discarded => slot,
            }
        };

        let kind = if carries_value {
            PairKind::Result { nil_guard }
        } else {
            PairKind::BareError
        };
        self.bind_pair(setup, call, slot, kind, error_name)
            .with_wraps(message_setup, messages)
    }

    pub(in crate::lowering) fn wrap_planned_error(
        &mut self,
        statements: &mut Vec<Statement>,
        wraps: &WrapChain<'_>,
        error: ValuePlan,
        finish: impl FnOnce(&mut Self, &mut Vec<Statement>, GoExpression) -> GoExpression,
    ) -> GoExpression {
        let (message_setup, messages) = self.prepare_wrap_messages(&wraps.0);
        let error = if message_setup.is_empty() {
            error
        } else {
            self.eager_operand(error, "err")
        };
        let (setup, error) = error.into_parts();
        statements.extend(setup);
        let error = finish(self, statements, error);
        statements.extend(message_setup);
        wrap_error(&messages, error)
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    fn prepare_wrap_messages(
        &mut self,
        messages: &[&Expression],
    ) -> (Vec<Statement>, Vec<WrapMessage>) {
        let mut setup = Vec::new();
        let mut prepared = Vec::new();
        for message in messages {
            let (format, args) = match message {
                Expression::Literal {
                    literal: Literal::String { value, raw: false },
                    ..
                } => (
                    convert_escape_sequences(value).replace('%', "%%"),
                    Vec::new(),
                ),
                Expression::Literal {
                    literal: Literal::FormatString(parts),
                    ..
                } if parts.iter().all(|part| match part {
                    FormatStringPart::Expression(e) => {
                        self.facts.simple_kind(&e.get_type()).is_some()
                    }
                    FormatStringPart::Text(_) => true,
                }) =>
                {
                    let mut values = Vec::new();
                    for part in parts {
                        if let FormatStringPart::Expression(expression) = part {
                            let value = self.plan_format_operand(expression);
                            let (part_setup, value) =
                                self.eager_operand(value, "fmtarg").into_parts();
                            setup.extend(part_setup);
                            values.push(value);
                        }
                    }
                    sprintf_pieces(&self.facts, parts, values, PercentSigns::Escape)
                }
                _ => {
                    let value = self.plan_composite_value(message, ExpressionContext::value());
                    let (message_setup, value) = self.eager_operand(value, "msg").into_parts();
                    setup.extend(message_setup);
                    ("%s".to_string(), vec![value])
                }
            };

            prepared.push(WrapMessage { format, args });
        }
        (setup, prepared)
    }
}

pub(super) fn wrap_error(messages: &[WrapMessage], error: GoExpression) -> GoExpression {
    messages.iter().fold(error, |error, message| {
        let mut args = vec![GoExpression::token(format!("\"{}: %w\"", message.format))];
        args.extend(message.args.iter().cloned());
        args.push(error);
        GoExpression::call(
            GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
            args,
        )
    })
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn peel_wrap_err<'a>(
        &self,
        mut expression: &'a Expression,
    ) -> (&'a Expression, WrapChain<'a>) {
        let mut messages = Vec::new();
        while let Some((receiver, message)) = self.wrap_err_call(expression) {
            messages.push(message);
            expression = receiver;
        }
        messages.reverse();
        (expression, WrapChain(messages))
    }

    fn wrap_err_call<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<(&'a Expression, &'a Expression)> {
        let (receiver, [message]) = self.prelude_method_call(expression, "Result", "wrap_err")?
        else {
            return None;
        };
        Some((receiver, message))
    }
}
