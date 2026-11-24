use syntax::program::Definition;
use syntax::types::{self, SimpleKind, Type};

pub(crate) const UNIT_DISPLAY: &str = "()";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayForm {
    Unit,
    Function,
    Char,
    String,
    Bool,
    Integer,
    Float,
    Value,
}

impl DisplayForm {
    pub(crate) fn of<'d>(ty: &Type, definition: impl Fn(&str) -> Option<&'d Definition>) -> Self {
        if is_raw_function_type(ty) {
            return DisplayForm::Function;
        }
        if types::resolves_to_unit(ty, &definition) {
            return DisplayForm::Unit;
        }
        match types::simple_kind(ty, &definition) {
            Some(SimpleKind::Rune) => DisplayForm::Char,
            Some(SimpleKind::String) => DisplayForm::String,
            Some(SimpleKind::Bool) => DisplayForm::Bool,
            Some(k) if k.is_integer() => DisplayForm::Integer,
            Some(k) if k.is_float() => DisplayForm::Float,
            _ => DisplayForm::Value,
        }
    }

    pub(crate) fn interpolation_verb(self) -> &'static str {
        match self {
            DisplayForm::Unit => UNIT_DISPLAY,
            DisplayForm::Char => "%c",
            DisplayForm::String => "%s",
            DisplayForm::Bool => "%t",
            DisplayForm::Integer => "%d",
            DisplayForm::Float => "%g",
            DisplayForm::Function => "%p",
            DisplayForm::Value => "%v",
        }
    }
}

pub(crate) fn is_raw_function_type(ty: &Type) -> bool {
    match ty {
        Type::Function(_) => true,
        Type::Forall { body, .. } => is_raw_function_type(body),
        _ => false,
    }
}
