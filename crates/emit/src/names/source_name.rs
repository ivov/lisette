use syntax::EcoString;

use crate::names::go_name;
use crate::plan::local::GoIdentifier;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceName(String);

impl SourceName {
    pub(crate) fn new(source: &EcoString) -> Self {
        let name = go_name::snake_to_lower_camel(source);
        match go_name::keyword_local_name(&name) {
            Some(short) => Self(short.to_string()),
            None => Self(name),
        }
    }

    pub(crate) fn constant(source: &EcoString) -> Self {
        Self(go_name::screaming_snake_to_camel(source))
    }

    pub(crate) fn component(&self, index: usize) -> Self {
        Self(format!("{}{index}", self.0))
    }

    pub(crate) fn base(&self) -> &str {
        &self.0
    }

    pub(crate) fn preferred(&self) -> String {
        go_name::escape_local(&self.0).into_owned()
    }
}

impl From<&SourceName> for GoIdentifier {
    fn from(name: &SourceName) -> Self {
        GoIdentifier::name(name.preferred())
    }
}

impl From<SourceName> for GoIdentifier {
    fn from(name: SourceName) -> Self {
        GoIdentifier::from(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_take_camel_case() {
        assert_eq!(
            SourceName::new(&"retry_count".into()).preferred(),
            "retryCount"
        );
        assert_eq!(
            SourceName::new(&"_unused_value".into()).preferred(),
            "_unusedValue"
        );
        assert_eq!(SourceName::new(&"x_1".into()).preferred(), "x1");
        assert_eq!(SourceName::new(&"_".into()).preferred(), "_");
    }

    #[test]
    fn reserved_words_are_escaped_after_conversion() {
        let name = SourceName::new(&"type_".into());
        assert_eq!(name.base(), "type");
        assert_eq!(name.preferred(), "type_");
    }
}
