use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::literals::convert_escape_sequences;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{GoExpression, ValuePlan};
use syntax::ast::{Expression, FormatStringPart, Literal};

pub(crate) struct WrapMessage {
    format: String,
    args: Vec<GoExpression>,
}

impl Planner<'_> {
    /// The call under a chain of `.wrap_err(message)`, with the messages inner to outer.
    pub(crate) fn peel_wrap_err<'a>(
        &self,
        expression: &'a Expression,
    ) -> (&'a Expression, Vec<&'a Expression>) {
        let mut expression = expression.unwrap_parens();
        let mut messages = Vec::new();
        while let Some((receiver, message)) = self.wrap_err_call(expression) {
            messages.push(message);
            expression = receiver.unwrap_parens();
        }
        messages.reverse();
        (expression, messages)
    }

    fn wrap_err_call<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<(&'a Expression, &'a Expression)> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } = expression
        else {
            return None;
        };
        let [message] = args.as_slice() else {
            return None;
        };
        if spread.is_some() || !self.plan_call(expression)?.resolved.is_prelude_dispatch {
            return None;
        }
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee.unwrap_parens()
        else {
            return None;
        };
        let wraps = member == "wrap_err" && self.facts.peel_alias(&receiver.get_type()).is_result();
        wraps.then_some((receiver.as_ref(), message))
    }

    /// Messages, including custom String calls, must run even on success.
    pub(crate) fn prepare_wrap_messages(
        &mut self,
        messages: &[&Expression],
        read_error: bool,
    ) -> (Vec<LoweredStatement>, Vec<WrapMessage>) {
        let mut setup = Vec::new();
        let mut prepared = Vec::new();
        for message in messages {
            if !read_error {
                setup.extend(self.lower_discard_value(message));
                continue;
            }
            let (format, args) = match message.unwrap_parens() {
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
                        self.facts.peel_alias(&e.get_type()).as_simple().is_some()
                    }
                    FormatStringPart::Text(_) => true,
                }) =>
                {
                    let mut values = Vec::new();
                    for part in parts {
                        if let FormatStringPart::Expression(expression) = part {
                            let value =
                                self.lower_composite_value(expression, ExpressionContext::value());
                            let ValuePlan {
                                setup: part_setup,
                                expression: value,
                                ..
                            } = self.eager_operand(expression, value, "fmtarg");
                            setup.extend(part_setup);
                            values.push(value);
                        }
                    }
                    self.sprintf_pieces(parts, values, true)
                }
                _ => {
                    let value = self.lower_composite_value(message, ExpressionContext::value());
                    let (message_setup, value) =
                        self.eager_operand(message, value, "msg").into_parts();
                    setup.extend(message_setup);
                    ("%s".to_string(), vec![value])
                }
            };
            prepared.push(WrapMessage { format, args });
        }
        (setup, prepared)
    }

    pub(crate) fn wrap_error(
        &mut self,
        messages: &[WrapMessage],
        error: GoExpression,
    ) -> GoExpression {
        messages.iter().fold(error, |error, message| {
            let mut args = vec![GoExpression::literal(format!("\"{}: %w\"", message.format))];
            args.extend(message.args.iter().cloned());
            args.push(error);
            GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Errorf"),
                args,
            )
        })
    }
}
