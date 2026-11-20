use crate::names::go_name;
use crate::types::go_type::GoType;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;

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
