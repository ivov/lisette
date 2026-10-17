use std::fmt::{self, Display, Formatter};
use std::ops::Deref;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LocalId(pub(crate) u32);

#[derive(Clone, Debug)]
pub(crate) struct GoIdentifier {
    spelling: String,
    target: IdentifierTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentifierTarget {
    Local(LocalId),
    External,
    Pending,
}

impl PartialEq for GoIdentifier {
    fn eq(&self, other: &Self) -> bool {
        self.refers_to_same(other) && self.spelling == other.spelling
    }
}

impl Eq for GoIdentifier {}

impl From<String> for GoIdentifier {
    fn from(spelling: String) -> Self {
        Self::name(spelling)
    }
}

impl From<&String> for GoIdentifier {
    fn from(spelling: &String) -> Self {
        Self::name(spelling.clone())
    }
}

impl From<&str> for GoIdentifier {
    fn from(spelling: &str) -> Self {
        Self::name(spelling.to_string())
    }
}

impl From<&GoIdentifier> for GoIdentifier {
    fn from(name: &GoIdentifier) -> Self {
        name.clone()
    }
}

impl GoIdentifier {
    pub(crate) fn name(spelling: String) -> Self {
        Self {
            spelling,
            target: IdentifierTarget::Pending,
        }
    }

    pub(crate) fn external(spelling: String) -> Self {
        Self {
            spelling,
            target: IdentifierTarget::External,
        }
    }

    pub(crate) fn local(spelling: String, id: LocalId) -> Self {
        Self {
            spelling,
            target: IdentifierTarget::Local(id),
        }
    }

    pub(crate) fn id(&self) -> Option<LocalId> {
        match self.target {
            IdentifierTarget::Local(id) => Some(id),
            IdentifierTarget::External | IdentifierTarget::Pending => None,
        }
    }

    pub(crate) fn refers_to_same(&self, other: &Self) -> bool {
        match (self.target, other.target) {
            (IdentifierTarget::Local(left), IdentifierTarget::Local(right)) => left == right,
            (IdentifierTarget::External, IdentifierTarget::External)
            | (IdentifierTarget::Pending, IdentifierTarget::Pending) => {
                self.spelling == other.spelling
            }
            _ => false,
        }
    }

    pub(crate) fn identify(&mut self, id: LocalId) {
        assert!(
            self.id().is_none_or(|existing| existing == id),
            "cannot change the identity of local `{}`",
            self.spelling
        );
        self.target = IdentifierTarget::Local(id);
    }

    pub(crate) fn finish(&mut self) {
        if self.target == IdentifierTarget::Pending {
            self.target = IdentifierTarget::External;
        }
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.target == IdentifierTarget::Pending
    }

    pub(crate) fn spelling(&self) -> &str {
        &self.spelling
    }

    pub(crate) fn spelling_mut(&mut self) -> &mut String {
        &mut self.spelling
    }
}

impl Deref for GoIdentifier {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.spelling
    }
}

impl Display for GoIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.spelling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_spelling_does_not_identify_distinct_locals() {
        let outer = GoIdentifier::local("x".to_string(), LocalId(1));
        let inner = GoIdentifier::local("x".to_string(), LocalId(2));
        assert!(!outer.refers_to_same(&inner));
        assert_ne!(outer, inner);
    }

    #[test]
    #[should_panic(expected = "cannot change the identity")]
    fn identifying_a_local_cannot_retarget_it() {
        let mut name = GoIdentifier::local("x".to_string(), LocalId(1));
        name.identify(LocalId(2));
    }
}
