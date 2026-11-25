use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::output::render::expressions::rendered;
use crate::target::ir::go_expression::{BinaryOp, CompositeLayout};
use crate::target::ir::values::{CaptureBoundary, ConstantKind, GoExpression, Stability};
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::display_form::{DisplayForm, UNIT_DISPLAY};
use crate::target::types::go_type::GoType;
use std::fmt::Write;
use std::iter::Peekable;
use std::mem::take;
use std::str::Chars;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::program::SlotOrigin;
use syntax::types::Type;

impl Planner<'_> {
    pub(super) fn plan_literal(&mut self, literal: &Literal, ty: &Type) -> ValuePlan {
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
            Literal::String { value, raw: true } => {
                (raw_string_literal(value), ConstantKind::String)
            }
            Literal::Char(c) => (
                format!("'{}'", convert_escape_sequences(c)),
                ConstantKind::Rune,
            ),
            Literal::FormatString(parts) => return self.plan_format_string(parts),
            Literal::Slice(elements) => return self.plan_slice_literal(elements, ty),
        };

        ValuePlan::constant(value, kind)
    }

    pub(in crate::lowering) fn constant_needs_go_type(
        &self,
        constant: Option<ConstantKind>,
        slot_ty: &Type,
    ) -> Option<GoType> {
        let kind = constant?;
        if self.facts.is_interface_or_unknown(slot_ty) {
            return None;
        }
        let peeled = self.plain_literal_go_type(slot_ty)?;
        if kind.is_default_go_type(&peeled) {
            return None;
        }
        Some(self.go_type(slot_ty))
    }

    fn plan_slice_literal(&mut self, elements: &[Expression], ty: &Type) -> ValuePlan {
        let element_lisette_ty = match ty {
            Type::Array { element, .. } => element.as_ref().clone(),
            _ => self
                .facts
                .first_type_argument(ty)
                .expect("Slice type must have element type"),
        };

        let element_ty = self.go_type(&element_lisette_ty);
        let go_type = self.go_type(ty);
        if elements.is_empty() {
            return ValuePlan::computed(Vec::new(), GoExpression::empty_composite(go_type));
        }

        let operands: Vec<ValuePlan> = elements
            .iter()
            .map(|e| {
                let origin = self
                    .facts
                    .function_type_origin(&element_lisette_ty, SlotOrigin::Lisette);
                self.plan_composite_value(
                    e,
                    ExpressionContext::value().with_function_slot_origin(origin),
                )
            })
            .collect();

        let sequenced = self.sequence_values(operands, CaptureBoundary::SiblingSequence, "v");
        let stability = sequenced.stability;
        let mut setup = sequenced.setup;

        let mut wrapped = Vec::with_capacity(sequenced.values.len());
        let mut widest = 0;
        for (expr, value) in elements.iter().zip(sequenced.values) {
            let coercion = self.value_slot_coercion(expr, &element_lisette_ty);
            let is_whole_literal = coercion.is_identity() && value.is_composite_literal();
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            widest = widest.max(rendered(&coerced).len());
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
            .position_elements(|index, _| self.facts.source_position(&elements[index].get_span()));
        ValuePlan::new(setup, value, stability)
    }

    fn plan_format_string(&mut self, parts: &[FormatStringPart]) -> ValuePlan {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));

        let mut operands: Vec<ValuePlan> = parts
            .iter()
            .filter_map(|p| {
                if let FormatStringPart::Expression(e) = p {
                    Some(self.plan_format_operand(e))
                } else {
                    None
                }
            })
            .collect();

        let concatenates = format_string_concatenates(&self.facts, parts);
        if concatenates && operands.len() == 1 && parts.len() == 1 {
            return operands.pop().expect("one operand");
        }
        let sequenced = self.sequence_values(operands, CaptureBoundary::SiblingSequence, "fmtarg");
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
                        if display_form(&self.facts, expression) == DisplayForm::Unit {
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
            return ValuePlan::new(setup, concatenation, stability);
        }

        let (format_string, args) = sprintf_pieces(&self.facts, parts, values, has_interpolation);

        if args.is_empty() {
            return ValuePlan::computed(
                setup,
                GoExpression::literal(format!("\"{}\"", format_string)),
            );
        }

        if !format_string_lowers_to_sprintf(&self.facts, parts) {
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

    pub(in crate::lowering) fn plan_format_operand(&mut self, operand: &Expression) -> ValuePlan {
        if display_form(&self.facts, operand) != DisplayForm::Unit {
            return self.plan_composite_value(operand, ExpressionContext::value());
        }
        let setup = self.lower_discard_value(operand);
        ValuePlan::new(
            setup,
            string_constant(UNIT_DISPLAY.to_string()),
            Stability::Fixed,
        )
    }

    pub(in crate::lowering) fn unit_value(
        &self,
        source: Option<&Expression>,
        slot: Option<&Type>,
    ) -> GoExpression {
        if let Some(slot) = slot
            && !self.slot_holds_unit(slot)
        {
            panic!("internal error: a unit value reached a slot of type `{slot}`");
        }
        if let Some(source) = source {
            self.record_converted_value(source);
        }
        GoExpression::unit()
    }

    fn slot_holds_unit(&self, slot: &Type) -> bool {
        self.facts.resolves_to_unit(slot)
            || self.facts.is_interface_or_unknown(slot)
            || self.facts.is_parameter_or_open(slot)
    }
}

fn string_constant(text: String) -> GoExpression {
    GoExpression::constant(format!("\"{text}\""), ConstantKind::String)
}

pub(in crate::lowering) fn raw_string_literal(value: &str) -> String {
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

pub(in crate::lowering) fn convert_escape_sequences(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => convert_escape(&mut chars, &mut result),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            _ => result.push(c),
        }
    }
    result
}

fn convert_escape(chars: &mut Peekable<Chars<'_>>, result: &mut String) {
    match chars.peek().copied() {
        Some('\\') => {
            chars.next();
            result.push_str("\\\\");
        }
        Some('0'..='7') => write_octal_escape(chars, result),
        Some('u') if starts_unicode_escape(chars) => write_unicode_escape(chars, result),
        _ => result.push('\\'),
    }
}

fn write_octal_escape(chars: &mut Peekable<Chars<'_>>, result: &mut String) {
    let mut value: u16 = 0;
    for _ in 0..3 {
        let Some(&digit @ '0'..='7') = chars.peek() else {
            break;
        };
        value = value * 8 + (digit as u16 - b'0' as u16);
        chars.next();
    }
    write!(result, "\\x{:02x}", value).unwrap();
}

fn starts_unicode_escape(chars: &Peekable<Chars<'_>>) -> bool {
    let mut lookahead = chars.clone();
    lookahead.next();
    lookahead.peek() == Some(&'{')
}

fn write_unicode_escape(chars: &mut Peekable<Chars<'_>>, result: &mut String) {
    chars.next();
    chars.next();
    let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
    let codepoint = u32::from_str_radix(&hex, 16).unwrap_or(0);
    if codepoint <= 0xFFFF {
        write!(result, "\\u{:04X}", codepoint).unwrap();
    } else {
        write!(result, "\\U{:08X}", codepoint).unwrap();
    }
}

fn display_form(facts: &EmitFacts<'_>, expression: &Expression) -> DisplayForm {
    DisplayForm::of(&expression.get_type(), |id| facts.definition(id))
}

pub(in crate::lowering) fn sprintf_pieces(
    facts: &EmitFacts<'_>,
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
                let form = display_form(facts, expression);
                format_string.push_str(form.interpolation_verb());
                if form != DisplayForm::Unit {
                    args.push(value);
                }
            }
        }
    }
    (format_string, args)
}

fn format_string_concatenates(facts: &EmitFacts<'_>, parts: &[FormatStringPart]) -> bool {
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
                display_form(facts, expression),
                DisplayForm::Unit | DisplayForm::String
            )
        })
}

pub(in crate::lowering) fn format_string_lowers_to_sprintf(
    facts: &EmitFacts<'_>,
    parts: &[FormatStringPart],
) -> bool {
    let has_interpolation = parts
        .iter()
        .any(|p| matches!(p, FormatStringPart::Expression(_)));
    if !has_interpolation || format_string_concatenates(facts, parts) {
        return false;
    }
    let [FormatStringPart::Expression(solo)] = parts else {
        return true;
    };
    display_form(facts, solo) == DisplayForm::Char
}
