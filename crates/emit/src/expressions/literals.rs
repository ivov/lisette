use std::fmt::Write;

use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use syntax::ast::{FormatStringPart, Literal};
use syntax::types::{SimpleKind, Type};

impl Planner<'_> {
    pub(super) fn emit_literal(
        &mut self,
        output: &mut String,
        literal: &Literal,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        match literal {
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
            Literal::FormatString(parts) => self.emit_format_string(output, parts, fx),
            Literal::Slice(elements) => {
                let element_lisette_ty = ty
                    .get_type_params()
                    .expect("Slice type must have type args")
                    .first()
                    .expect("Slice type must have element type")
                    .clone();
                let element_ty = self.go_type_string(&element_lisette_ty, fx);

                if elements.is_empty() {
                    // Parens around the slice type disambiguate the conversion when
                    // the element type itself ends in `)` (e.g. `func(int)`); Go
                    // otherwise parses `[]func(int)(nil)` as a call expression.
                    return format!("([]{})(nil)", element_ty);
                }

                let stages: Vec<StagedExpression> = elements
                    .iter()
                    .map(|e| self.stage_composite(e, ExpressionContext::value(), fx))
                    .collect();
                let (setup, rendered) = self.sequence_structured(stages, "_v");
                output.push_str(&Renderer.render_setup(&setup));

                let mut wrapped: Vec<String> = Vec::with_capacity(rendered.len());
                for (expr, emitted) in elements.iter().zip(rendered) {
                    let coercion = Coercion::resolve(
                        self,
                        &expr.get_type(),
                        &element_lisette_ty,
                        CoercionDirection::Internal,
                    );
                    wrapped.push(coercion.apply(self, output, emitted, fx));
                }
                let elements = wrapped;

                if elements.len() > 1 && elements.iter().any(|e| e.len() > 30) {
                    let indented = elements
                        .iter()
                        .map(|e| format!("\t{}", e))
                        .collect::<Vec<_>>()
                        .join(",\n");
                    format!("[]{}{{\n{},\n}}", element_ty, indented)
                } else {
                    format!("[]{}{{ {} }}", element_ty, elements.join(", "))
                }
            }
        }
    }

    fn emit_format_string(
        &mut self,
        output: &mut String,
        parts: &[FormatStringPart],
        fx: &mut EmitEffects,
    ) -> String {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));

        let stages: Vec<StagedExpression> = parts
            .iter()
            .filter_map(|p| {
                if let FormatStringPart::Expression(e) = p {
                    Some(self.stage_composite(e, ExpressionContext::value(), fx))
                } else {
                    None
                }
            })
            .collect();
        let (setup, emitted) = self.sequence_structured(stages, "_fmtarg");
        output.push_str(&Renderer.render_setup(&setup));

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
                    let format_verb = match expression.get_type().as_simple() {
                        Some(SimpleKind::Rune) => "%c",
                        Some(SimpleKind::String) => "%s",
                        Some(SimpleKind::Bool) => "%t",
                        Some(k) if k.is_signed_int() || k.is_unsigned_int() => "%d",
                        Some(k) if k.is_float() => "%g",
                        _ => "%v",
                    };
                    format_string.push_str(format_verb);
                    args.push(
                        emitted
                            .next()
                            .expect("emitted count matches expression parts"),
                    );
                }
            }
        }

        if args.is_empty() {
            return format!("\"{}\"", format_string);
        }

        fx.require_fmt();
        // Solo-expression f-strings round-trip through fmt.Sprint, which skips
        // the format-string parse. Excluded: `%c`, because Sprint on a rune
        // prints the integer codepoint instead of the character.
        if args.len() == 1
            && matches!(parts, [FormatStringPart::Expression(_)])
            && format_string != "%c"
        {
            return format!("fmt.Sprint({})", args[0]);
        }
        format!("fmt.Sprintf(\"{}\", {})", format_string, args.join(", "))
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
