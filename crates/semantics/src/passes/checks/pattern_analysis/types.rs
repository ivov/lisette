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
    Constructor {
        type_name: TypeName,
        tag: TagId,
        args: Vec<NormalizedPattern>,
    },
}

pub type UnionTable = HashMap<TypeName, Union>;

pub const INTERFACE_UNKNOWN_TAG: &str = "__interface_unknown__";
