use rustc_hash::FxHashMap as HashMap;
use std::ops::Deref;

use syntax::ast::Literal;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TagId(String);

impl TagId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TagId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for TagId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for TagId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeName(String);

impl TypeName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TypeName {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for TypeName {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Deref for TypeName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

pub type Row = Vec<NormalizedPattern>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Constructor {
    pub tag_id: TagId,
    pub arity: usize,
}

pub type Union = Vec<Constructor>;

#[derive(Clone, Debug, PartialEq)]
pub enum NormalizedPattern {
    Wildcard,
    Literal(Literal),
    /// A const pattern whose value is not known at analysis time, keyed by the
    /// constant's qualified name. Behaves like a literal singleton (open domain,
    /// never exhaustive) but in a separate namespace, so it catches repeated use
    /// of the same constant without colliding with real string literals.
    OpaqueConst(String),
    Constructor {
        type_name: TypeName,
        tag: TagId,
        args: Vec<NormalizedPattern>,
    },
}

pub type UnionTable = HashMap<TypeName, Union>;

pub const INTERFACE_UNKNOWN_TAG: &str = "__interface_unknown__";
