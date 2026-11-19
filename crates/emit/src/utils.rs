use crate::EmitFacts;
use crate::names::go_name;
use crate::types::go_type::GoType;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use syntax::ast::{Expression, Literal};
use syntax::program::DotAccessResolution;
use syntax::types::Type;

macro_rules! write_line {
    ($dst:expr, $($arg:tt)*) => {
        { use std::fmt::Write as _; writeln!($dst, $($arg)*).unwrap() }
    };
}
pub(crate) use write_line;

fn receiver_letter(type_name: &str) -> String {
    type_name
        .trim_start_matches('*')
        .split('[')
        .next()
        .unwrap_or(type_name)
        .chars()
        .find(|character| character.is_alphabetic())
        .map_or_else(
            || "r".to_string(),
            |letter| letter.to_lowercase().to_string(),
        )
}

pub(crate) fn fresh_receiver_name(type_name: &str, taken: impl Fn(&str) -> bool) -> String {
    let letter = receiver_letter(type_name);
    if !taken(&letter) {
        return letter;
    }
    let doubled = format!("{letter}{letter}");
    if !taken(&doubled) {
        return doubled;
    }
    (2..)
        .map(|n| format!("{letter}{n}"))
        .find(|candidate| !taken(candidate))
        .expect("freshening counter is unbounded")
}

#[derive(Clone, Default)]
pub(crate) struct ReceiverGenerics(pub(crate) Vec<String>);
impl Display for ReceiverGenerics {
    fn fmt(&self, output: &mut Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            Ok(())
        } else {
            write!(output, "[{}]", self.0.join(", "))
        }
    }
}
impl ReceiverGenerics {
    pub(crate) fn receiver_type(&self, type_name: &str) -> GoType {
        GoType::named(
            &go_name::escape_type_name(type_name),
            &self.0.iter().map(GoType::new).collect::<Vec<_>>(),
            None,
        )
    }
}

pub(crate) fn synthesized_receiver_name(
    type_name: &str,
    receiver_generics: &ReceiverGenerics,
) -> String {
    fresh_receiver_name(type_name, |name| {
        receiver_generics.0.iter().any(|generic| generic == name)
    })
}

pub(crate) fn synthesized_local_name(
    base: &str,
    receiver: &str,
    receiver_generics: &ReceiverGenerics,
) -> String {
    go_name::fresh_suffixed(base, |name| {
        name == receiver || receiver_generics.0.iter().any(|generic| generic == name)
    })
}

pub(crate) fn group_params(params: &[(String, String)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    if params.len() == 1 {
        return format!("{} {}", params[0].0, params[0].1);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut names: Vec<&str> = vec![&params[0].0];
    let mut current_ty = &params[0].1;

    for param in &params[1..] {
        if param.1 == *current_ty {
            names.push(&param.0);
        } else {
            parts.push(format!("{} {}", names.join(", "), current_ty));
            names.clear();
            names.push(&param.0);
            current_ty = &param.1;
        }
    }
    parts.push(format!("{} {}", names.join(", "), current_ty));
    parts.join(", ")
}

fn is_scalar_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_),
            ..
        }
    )
}

pub(crate) fn is_order_sensitive(expression: &Expression) -> bool {
    !(is_scalar_literal(expression)
        || matches!(expression.unwrap_parens(), Expression::Identifier { .. }))
}

pub(crate) fn reads_value_member(
    facts: &EmitFacts,
    resolution: &DotAccessResolution,
    base: &Expression,
    base_ty: &Type,
) -> bool {
    resolution.is_field_read() && base.deref_inner().is_none() && !facts.is_reference(base_ty)
}
