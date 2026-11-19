use std::fmt::Write;
use std::mem::take;

use crate::Planner;
use crate::expressions::display_form::{DisplayForm, UNIT_DISPLAY};
use crate::ir::go_expression::{BinaryOp, CompositeLayout};
use crate::ir::values::{CaptureBoundary, ConstantKind, GoExpression, Stability, ValuePlan};
use crate::names::go_name::GeneratedPackage;
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::program::SlotOrigin;
use syntax::types::Type;

impl Planner<'_> {
    pub(super) fn emit_literal(&mut self, literal: &Literal, ty: &Type) -> ValuePlan {
        let (value, kind) = match literal {
            Literal::Integer { value, text } => {
                let rendered = match text {
                    Some(original) => original.clone(),
                    None => value.to_string(),
                };
                (rendered, ConstantKind::Int)
            }
            Literal::Float { value, text } => {
                let rendered = match text {
                    Some(t) => t.clone(),
                    None => {
                        let s = value.to_string();
                        if s.contains('.') || s.contains('e') || s.contains('E') {
                            s
                        } else {
                            format!("{}.0", s)
                        }
                    }
                };
                (rendered, ConstantKind::Float)
            }
            Literal::Imaginary(coef) => {
                let rendered = if *coef == coef.trunc() && coef.abs() < 1e15 {
                    format!("{}i", *coef as i64)
                } else {
                    format!("{}i", coef)
                };
                (rendered, ConstantKind::Complex)
            }
            Literal::Boolean(b) => (b.to_string(), ConstantKind::Bool),
            Literal::String { value, raw: false } => (
                format!("\"{}\"", convert_escape_sequences(value)),
                ConstantKind::String,
            ),
            Literal::String { value, raw: true } => (emit_raw_string(value), ConstantKind::String),
            Literal::Char(c) => (
                format!("'{}'", convert_escape_sequences(c)),
                ConstantKind::Rune,
            ),
            Literal::FormatString(parts) => return self.emit_format_string(parts),
            Literal::Slice(elements) => return self.emit_slice_literal(elements, ty),
        };
        ValuePlan::constant(value, kind)
    }

    pub(crate) fn constant_needs_go_type(
        &mut self,
        constant: Option<ConstantKind>,
        slot_ty: &Type,
    ) -> Option<GoType> {
        let kind = constant?;
        if self.facts.is_interface_or_unknown(slot_ty) {
            return None;
        }
        let peeled = self.facts.peel_alias(slot_ty);
        match &peeled {
            Type::Simple(_) => {}
            Type::Nominal { params, .. } if params.is_empty() => {}
            _ => return None,
        }
        if kind.is_default_go_type(&self.go_type(&peeled)) {
            return None;
        }
        Some(self.go_type(slot_ty))
    }

    fn emit_slice_literal(&mut self, elements: &[Expression], ty: &Type) -> ValuePlan {
        let element_lisette_ty = match ty {
            Type::Array { element, .. } => element.as_ref().clone(),
            _ => ty
                .get_type_params()
                .expect("Slice type must have type args")
                .first()
                .expect("Slice type must have element type")
                .clone(),
        };
        let element_ty = self.go_type(&element_lisette_ty);
        let go_type = self.go_type(ty);
        if elements.is_empty() {
            return ValuePlan::computed(Vec::new(), GoExpression::empty_composite(go_type));
        }

        let stages: Vec<ValuePlan> = elements
            .iter()
            .map(|e| {
                let origin = self.function_type_origin(&element_lisette_ty, SlotOrigin::Lisette);
                self.lower_composite_value(
                    e,
                    ExpressionContext::value().with_function_slot_origin(origin),
                )
            })
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "v");
        let stability = sequenced.stability;
        let mut setup = sequenced.setup;

        let mut wrapped = Vec::with_capacity(sequenced.values.len());
        let mut widest = 0;
        for (expr, value) in elements.iter().zip(sequenced.values) {
            let coercion = self.value_slot_coercion(expr, &element_lisette_ty);
            let is_whole_literal = coercion.is_identity() && value.is_composite_literal();
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            widest = widest.max(coerced.rendered().len());
            wrapped.push((
                None,
                if is_whole_literal {
                    coerced.elide_composite_type(&element_ty)
                } else {
                    coerced
                },
            ));
        }

        let layout = CompositeLayout::for_elements(wrapped.len(), widest);
        let value = GoExpression::composite(Some(go_type), wrapped, layout)
            .position_elements(|index, _| self.source_position(&elements[index].get_span()));
        ValuePlan::built_from(setup, value, stability)
    }

    fn emit_format_string(&mut self, parts: &[FormatStringPart]) -> ValuePlan {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));

        let mut stages: Vec<ValuePlan> = parts
            .iter()
            .filter_map(|p| {
                if let FormatStringPart::Expression(e) = p {
                    Some(self.lower_format_operand(e))
                } else {
                    None
                }
            })
            .collect();
        let concatenates = self.format_string_concatenates(parts);
        if concatenates && stages.len() == 1 && parts.len() == 1 {
            return stages.pop().expect("one staged operand");
        }
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "fmtarg");
        let stability = sequenced.stability;
        let setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();

        if concatenates {
            let mut pieces: Vec<GoExpression> = Vec::with_capacity(parts.len());
            let mut text = String::new();
            for part in parts {
                match part {
                    FormatStringPart::Text(part_text) => {
                        let unescaped = part_text.replace("{{", "{").replace("}}", "}");
                        text.push_str(&convert_escape_sequences(&unescaped));
                    }
                    FormatStringPart::Expression(expression) => {
                        let value = values
                            .next()
                            .expect("sequenced count matches expression parts");
                        if self.display_form(expression) == DisplayForm::Unit {
                            text.push_str(UNIT_DISPLAY);
                            continue;
                        }
                        if !text.is_empty() {
                            pieces.push(string_constant(take(&mut text)));
                        }
                        pieces.push(value);
                    }
                }
            }
            if !text.is_empty() {
                pieces.push(string_constant(text));
            }
            let mut pieces = pieces.into_iter();
            let first = pieces.next().expect("an interpolated f-string has a piece");
            let concatenation = pieces.fold(first, |left, right| {
                GoExpression::binary(left, BinaryOp::Add, right)
            });
            return ValuePlan::built_from(setup, concatenation, stability);
        }

        let (format_string, args) = self.sprintf_pieces(parts, values, has_interpolation);

        if args.is_empty() {
            return ValuePlan::computed(
                setup,
                GoExpression::literal(format!("\"{}\"", format_string)),
            );
        }

        if !self.format_string_lowers_to_sprintf(parts) {
            return ValuePlan::plain_call(
                setup,
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Fmt, "Sprint"),
                    args,
                ),
            );
        }
        let mut arguments = vec![GoExpression::literal(format!("\"{}\"", format_string))];
        arguments.extend(args);
        ValuePlan::plain_call(
            setup,
            GoExpression::call(
                GoExpression::generated(GeneratedPackage::Fmt, "Sprintf"),
                arguments,
            ),
        )
    }

    pub(crate) fn sprintf_pieces(
        &self,
        parts: &[FormatStringPart],
        values: impl IntoIterator<Item = GoExpression>,
        escape_percent: bool,
    ) -> (String, Vec<GoExpression>) {
        let mut values = values.into_iter();
        let mut format_string = String::new();
        let mut args = Vec::new();
        for part in parts {
            match part {
                FormatStringPart::Text(text) => {
                    let unescaped = text.replace("{{", "{").replace("}}", "}");
                    let unescaped = convert_escape_sequences(&unescaped);
                    if escape_percent {
                        format_string.push_str(&unescaped.replace('%', "%%"));
                    } else {
                        format_string.push_str(&unescaped);
                    }
                }
                FormatStringPart::Expression(expression) => {
                    let value = values
                        .next()
                        .expect("sequenced count matches expression parts");
                    let form = self.display_form(expression);
                    format_string.push_str(form.interpolation_verb());
                    if form != DisplayForm::Unit {
                        args.push(value);
                    }
                }
            }
        }
        (format_string, args)
    }

    pub(crate) fn lower_format_operand(&mut self, operand: &Expression) -> ValuePlan {
        if self.display_form(operand) != DisplayForm::Unit {
            return self.lower_composite_value(operand, ExpressionContext::value());
        }
        let setup = self.lower_discard_value(operand);
        ValuePlan::built_from(
            setup,
            string_constant(UNIT_DISPLAY.to_string()),
            Stability::Fixed,
        )
    }

    pub(crate) fn format_string_lowers_to_sprintf(&self, parts: &[FormatStringPart]) -> bool {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));
        if !has_interpolation || self.format_string_concatenates(parts) {
            return false;
        }
        let [FormatStringPart::Expression(solo)] = parts else {
            return true;
        };
        self.display_form(solo) == DisplayForm::Char
    }

    fn display_form(&self, expression: &Expression) -> DisplayForm {
        DisplayForm::of(&expression.get_type(), |id| self.facts.definition(id))
    }

    fn format_string_concatenates(&self, parts: &[FormatStringPart]) -> bool {
        let mut operands = parts
            .iter()
            .filter_map(|part| match part {
                FormatStringPart::Expression(expression) => Some(expression),
                FormatStringPart::Text(_) => None,
            })
            .peekable();
        operands.peek().is_some()
            && operands.all(|expression| {
                matches!(
                    self.display_form(expression),
                    DisplayForm::Unit | DisplayForm::String
                )
            })
    }
}

fn string_constant(text: String) -> GoExpression {
    GoExpression::constant(format!("\"{text}\""), ConstantKind::String)
}

pub(crate) fn emit_raw_string(value: &str) -> String {
    if !value.contains('`') && !value.contains('\r') {
        format!("`{}`", value)
    } else {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n");
        format!("\"{}\"", escaped)
    }
}

pub(crate) fn convert_escape_sequences(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.peek() == Some(&'\\') {
                result.push('\\');
                result.push('\\');
                chars.next();
            } else if matches!(chars.peek(), Some('0'..='7')) {
                let mut value: u16 = 0;
                for _ in 0..3 {
                    match chars.peek() {
                        Some(&d @ '0'..='7') => {
                            value = value * 8 + (d as u16 - b'0' as u16);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                write!(result, "\\x{:02x}", value).unwrap();
            } else if chars.peek() == Some(&'u') && {
                let mut lookahead = chars.clone();
                lookahead.next();
                lookahead.peek() == Some(&'{')
            } {
                chars.next();
                chars.next();
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let codepoint = u32::from_str_radix(&hex, 16).unwrap_or(0);
                if codepoint <= 0xFFFF {
                    write!(result, "\\u{:04X}", codepoint).unwrap();
                } else {
                    write!(result, "\\U{:08X}", codepoint).unwrap();
                }
            } else {
                result.push(c);
            }
        } else if c == '\n' {
            result.push_str("\\n");
        } else if c == '\r' {
            result.push_str("\\r");
        } else {
            result.push(c);
        }
    }
    result
}
