use crate::Planner;
use crate::lowering::expressions::literals::convert_escape_sequences;
use crate::lowering::expressions::literals::{PercentSigns, sprintf_pieces};
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::Statement;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use syntax::ast::{Expression, FormatStringPart, Literal};

pub(in crate::lowering) struct WrapMessage {
    format: String,
    args: Vec<GoExpression>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum ErrorUse {
    Read,
    Unread,
}

impl Planner<'_> {
    pub(in crate::lowering) fn prepare_wrap_messages(
        &mut self,
        messages: &[&Expression],
        error_use: ErrorUse,
    ) -> (Vec<Statement>, Vec<WrapMessage>) {
        let mut setup = Vec::new();
        let mut prepared = Vec::new();
        for message in messages {
            if error_use == ErrorUse::Unread {
                setup.extend(self.lower_discard_value(message));
                continue;
            }
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

pub(in crate::lowering) fn wrap_error(
    messages: &[WrapMessage],
    error: GoExpression,
) -> GoExpression {
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
    ) -> (&'a Expression, Vec<&'a Expression>) {
        let mut messages = Vec::new();
        while let Some((receiver, message)) = self.wrap_err_call(expression) {
            messages.push(message);
            expression = receiver;
        }
        messages.reverse();
        (expression, messages)
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
