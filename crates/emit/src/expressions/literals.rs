use std::fmt::Write;

use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::context::expression::ExpressionContext;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::types::{SimpleKind, Type};

impl Planner<'_> {
    pub(super) fn emit_literal(&mut self, literal: &Literal, ty: &Type) -> ValuePlan {
        let value = match literal {
            Literal::Integer { value, text } => match text {
                Some(original) => original.clone(),
                None => value.to_string(),
            },
            Literal::Float { value, text } => match text {
                Some(t) => t.clone(),
                None => {
                    let s = value.to_string();
                    if s.contains('.') || s.contains('e') || s.contains('E') {
                        s
                    } else {
                        format!("{}.0", s)
                    }
                }
            },
            Literal::Imaginary(coef) => {
                if *coef == coef.trunc() && coef.abs() < 1e15 {
                    format!("{}i", *coef as i64)
                } else {
                    format!("{}i", coef)
                }
            }
            Literal::Boolean(b) => b.to_string(),
            Literal::String { value, raw: false } => {
                format!("\"{}\"", convert_escape_sequences(value))
            }
            Literal::String { value, raw: true } => emit_raw_string(value),
            Literal::Char(c) => {
                format!("'{}'", convert_escape_sequences(c))
            }
            Literal::FormatString(parts) => return self.emit_format_string(parts),
            Literal::Slice(elements) => return self.emit_slice_literal(elements, ty),
        };
        ValuePlan::literal(value)
    }

    fn emit_slice_literal(&mut self, elements: &[Expression], ty: &Type) -> ValuePlan {
        // A list literal builds a slice or a fixed-size array, per its type.
        let (element_lisette_ty, type_prefix, is_array) = match ty {
            Type::Array { length, element } => {
                (element.as_ref().clone(), format!("[{}]", length), true)
            }
            _ => (
                ty.get_type_params()
                    .expect("Slice type must have type args")
                    .first()
                    .expect("Slice type must have element type")
                    .clone(),
                "[]".to_string(),
                false,
            ),
        };
        let element_ty = self.go_type_string(&element_lisette_ty);

        if elements.is_empty() {
            let value = if is_array {
                format!("{}{}{{}}", type_prefix, element_ty)
            } else {
                // Parens around the slice type disambiguate the conversion when
                // the element type itself ends in `)` (e.g. `func(int)`); Go
                // otherwise parses `[]func(int)(nil)` as a call expression.
                format!("({}{})(nil)", type_prefix, element_ty)
            };
            return ValuePlan::computed(
                Vec::new(),
                GoExpression::composite_literal(value, false),
                EvaluationEffect::Pure,
            );
        }

        let stages: Vec<ValuePlan> = elements
            .iter()
            .map(|e| self.stage_composite(e, ExpressionContext::value()))
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "_v");
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (mut setup, rendered) = sequenced.into_rendered();

        let mut wrapped: Vec<String> = Vec::with_capacity(rendered.len());
        for (expr, emitted) in elements.iter().zip(rendered) {
            let coercion = CoercionPlan::internal(self, &expr.get_type(), &element_lisette_ty);
            let (coercion_setup, coerced) = coercion.lower(self, emitted);
            setup.extend(coercion_setup);
            wrapped.push(coerced);
        }
        let elements = wrapped;

        let value = if elements.len() > 1 && elements.iter().any(|e| e.len() > 30) {
            let indented = elements
                .iter()
                .map(|e| format!("\t{}", e))
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{}{}{{\n{},\n}}", type_prefix, element_ty, indented)
        } else {
            format!("{}{}{{ {} }}", type_prefix, element_ty, elements.join(", "))
        };
        ValuePlan::computed(
            setup,
            GoExpression::composite_literal(value, contains_deferred_evaluation),
            effect,
        )
    }

    fn emit_format_string(&mut self, parts: &[FormatStringPart]) -> ValuePlan {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));

        let stages: Vec<ValuePlan> = parts
            .iter()
            .filter_map(|p| {
                if let FormatStringPart::Expression(e) = p {
                    Some(self.stage_composite(e, ExpressionContext::value()))
                } else {
                    None
                }
            })
            .collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "_fmtarg");
        let effect = sequenced.effect;
        let (setup, emitted) = sequenced.into_rendered();

        let mut format_string = String::new();
        let mut args = Vec::with_capacity(emitted.len());
        let mut emitted = emitted.into_iter();

        for part in parts {
            match part {
                FormatStringPart::Text(text) => {
                    let unescaped = text.replace("{{", "{").replace("}}", "}");
                    let unescaped = convert_escape_sequences(&unescaped);
                    if has_interpolation {
                        format_string.push_str(&unescaped.replace('%', "%%"));
                    } else {
                        format_string.push_str(&unescaped);
                    }
                }
                FormatStringPart::Expression(expression) => {
                    let peeled = self.facts.peel_alias(&expression.get_type());
                    format_string.push_str(format_verb_for(&peeled));
                    args.push(
                        emitted
                            .next()
                            .expect("emitted count matches expression parts"),
                    );
                }
            }
        }

        if args.is_empty() {
            return ValuePlan::evaluated_literal(setup, format!("\"{}\"", format_string), effect);
        }

        self.require_fmt();
        // Solo-expression f-strings round-trip through fmt.Sprint, which skips
        // the format-string parse. Excluded: `%c`, because Sprint on a rune
        // prints the integer codepoint instead of the character.
        if args.len() == 1
            && matches!(parts, [FormatStringPart::Expression(_)])
            && format_string != "%c"
        {
            return ValuePlan::observable_call(
                setup,
                GoExpression::call(
                    GoExpression::opaque("fmt.Sprint".to_string()),
                    vec![GoExpression::opaque(args[0].clone())],
                ),
                effect,
            );
        }
        let mut arguments = vec![GoExpression::literal(format!("\"{}\"", format_string))];
        arguments.extend(args.into_iter().map(GoExpression::opaque));
        ValuePlan::observable_call(
            setup,
            GoExpression::call(GoExpression::opaque("fmt.Sprintf".to_string()), arguments),
            effect,
        )
    }
}

/// The `fmt` printf verb for interpolating a value of alias-peeled `ty` into
/// an f-string.
fn format_verb_for(ty: &Type) -> &'static str {
    match ty.as_simple() {
        Some(SimpleKind::Rune) => "%c",
        Some(SimpleKind::String) => "%s",
        Some(SimpleKind::Bool) => "%t",
        Some(k) if k.is_signed_int() || k.is_unsigned_int() => "%d",
        Some(k) if k.is_float() => "%g",
        _ => "%v",
    }
}

pub(crate) fn emit_raw_string(value: &str) -> String {
    // Go backtick raw strings cannot contain backticks, and Go discards `\r`
    // from them, so fall back to double-quoted form in either case.
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
                chars.next(); // consume 'u'
                chars.next(); // consume '{'
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
