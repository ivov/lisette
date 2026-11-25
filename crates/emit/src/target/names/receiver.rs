use crate::target::names::go_name;
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
    let mut n = 2;
    loop {
        let candidate = format!("{letter}{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
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
