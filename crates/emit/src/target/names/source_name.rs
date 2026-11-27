use syntax::EcoString;

use crate::target::names::go_name;

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
