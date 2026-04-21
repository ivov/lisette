use std::fmt::Write;

use crate::Emitter;
use crate::utils::Staged;
use syntax::ast::{FormatStringPart, Literal};
use syntax::types::Type;

impl Emitter<'_> {
    pub(super) fn emit_literal(
        &mut self,
        output: &mut String,
        literal: &Literal,
        ty: &Type,
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
            Literal::String(s) => {
                format!("\"{}\"", convert_escape_sequences(s))
            }
            Literal::Char(c) => {
                format!("'{}'", convert_escape_sequences(c))
            }
            Literal::FormatString(parts) => self.emit_format_string(output, parts),
            Literal::Slice(elems) => {
                let stages: Vec<Staged> = elems.iter().map(|e| self.stage_composite(e)).collect();
                let elements = self.sequence(output, stages, "_v");

                let elem_lisette_ty = ty
                    .get_type_params()
                    .expect("Slice type must have type args")
                    .first()
                    .expect("Slice type must have element type")
                    .clone();
                let elem_ty = self.go_type_as_string(&elem_lisette_ty);

                let elements: Vec<String> = elems
                    .iter()
                    .zip(elements)
                    .map(|(expr, emitted)| {
                        self.maybe_wrap_as_go_interface(emitted, &expr.get_type(), &elem_lisette_ty)
                    })
                    .collect();

                if elements.len() > 1 && elements.iter().any(|e| e.len() > 30) {
                    let indented = elements
                        .iter()
                        .map(|e| format!("\t{}", e))
                        .collect::<Vec<_>>()
                        .join(",\n");
                    format!("[]{}{{\n{},\n}}", elem_ty, indented)
                } else {
                    format!("[]{}{{ {} }}", elem_ty, elements.join(", "))
                }
            }
        }
    }

    fn emit_format_string(&mut self, output: &mut String, parts: &[FormatStringPart]) -> String {
        let has_interpolation = parts
            .iter()
            .any(|p| matches!(p, FormatStringPart::Expression(_)));

        // Stage all expression parts for eval-order sequencing
        let stages: Vec<Staged> = parts
            .iter()
            .filter_map(|p| {
                if let FormatStringPart::Expression(e) = p {
                    Some(self.stage_composite(e))
                } else {
                    None
                }
            })
            .collect();
        let emitted = self.sequence(output, stages, "_fmtarg");

        let mut format_string = String::new();
        let mut args = Vec::new();
        let mut expression_idx = 0;

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
                    let format_verb = if expression.get_type().resolve().is_rune() {
                        "%c"
                    } else {
                        "%v"
                    };
                    format_string.push_str(format_verb);
                    args.push(emitted[expression_idx].clone());
                    expression_idx += 1;
                }
            }
        }

        if args.is_empty() {
            return format!("\"{}\"", format_string);
        }

        self.flags.needs_fmt = true;
        if format_string == "%v" && args.len() == 1 {
            return format!("fmt.Sprint({})", args[0]);
        }
        format!("fmt.Sprintf(\"{}\", {})", format_string, args.join(", "))
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
        } else {
            result.push(c);
        }
    }
    result
}
