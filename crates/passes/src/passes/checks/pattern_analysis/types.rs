use rustc_hash::FxHashMap as HashMap;

use syntax::ast::Literal;

pub type TagId = String;
pub type TypeName = String;

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
