use syntax::program::Definition;
use syntax::types::{self, SimpleKind, Type};

use crate::definitions::structs::is_raw_function_type;

pub(crate) const UNIT_DISPLAY: &str = "()";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayForm {
    Unit,
    Function,
    /// Go stores a rune as an `int32`, so only the static type tells them apart.
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
        let peeled = types::peel_alias(ty, &definition);
        if peeled.is_unit() {
            return DisplayForm::Unit;
        }
        match peeled.as_simple() {
            Some(SimpleKind::Rune) => DisplayForm::Char,
            Some(SimpleKind::String) => DisplayForm::String,
            Some(SimpleKind::Bool) => DisplayForm::Bool,
            Some(k) if k.is_signed_int() || k.is_unsigned_int() => DisplayForm::Integer,
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
            DisplayForm::Function | DisplayForm::Value => "%v",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    const ALLOWED: [(&str, &str); 3] = [
        ("expressions/display_form.rs", "the display table"),
        (
            "definitions/structs.rs",
            "`StringFormat::verb` reads the table, and the stringer shadow copies Go's own struct print",
        ),
        (
            "calls/wrap_err.rs",
            "an error message is a string, not a displayed value",
        ),
    ];

    fn is_lone_verb(literal: &str) -> bool {
        let bytes = literal.as_bytes();
        bytes.len() == 2 && bytes[0] == b'%' && bytes[1].is_ascii_lowercase()
    }

    fn lone_verb_sites(root: &Path, dir: &Path, sites: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                lone_verb_sites(root, &path, sites);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(&path).unwrap();
            for (index, line) in source
                .lines()
                .take_while(|line| line.trim() != "#[cfg(test)]")
                .enumerate()
            {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.split('"').skip(1).step_by(2).any(is_lone_verb) {
                    sites.push(format!("{relative}:{}", index + 1));
                }
            }
        }
    }

    #[test]
    fn display_verbs_are_chosen_only_by_display_form() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sites = Vec::new();
        lone_verb_sites(&src, &src, &mut sites);
        let outside: Vec<&String> = sites
            .iter()
            .filter(|site| {
                !ALLOWED
                    .iter()
                    .any(|(file, _)| site.starts_with(&format!("{file}:")))
            })
            .collect();
        assert!(
            outside.is_empty(),
            "a `fmt` verb is chosen outside `DisplayForm`: {outside:?}. \
             Add the rule to `DisplayForm::of` and read the verb from it"
        );
    }
}
