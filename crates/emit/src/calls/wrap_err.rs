use crate::Planner;
use crate::expressions::literals::convert_escape_sequences;
use crate::ir::bodies::Statement;
use crate::ir::values::GoExpression;
use crate::names::go_name::GeneratedPackage;
use crate::state::expression::ExpressionContext;
use syntax::ast::{Expression, FormatStringPart, Literal};

pub(crate) struct WrapMessage {
    format: String,
    args: Vec<GoExpression>,
}

impl Planner<'_> {
    pub(crate) fn peel_wrap_err<'a>(
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

    pub(crate) fn prepare_wrap_messages(
        &mut self,
        messages: &[&Expression],
        read_error: bool,
    ) -> (Vec<Statement>, Vec<WrapMessage>) {
        let mut setup = Vec::new();
        let mut prepared = Vec::new();
        for message in messages {
            if !read_error {
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
                                self.eager_operand(expression, value, "fmtarg").into_parts();
                            setup.extend(part_setup);
                            values.push(value);
                        }
                    }
                    self.facts.sprintf_pieces(parts, values, true)
                }
                _ => {
                    let value = self.plan_composite_value(message, ExpressionContext::value());
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
