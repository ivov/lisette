use rustc_hash::FxHashMap;
use syntax::ast::Expression;
use syntax::program::Definition;
use syntax::types::Symbol;

pub(in crate::lookup::facts) fn known_definition<'d>(
    definitions: &'d FxHashMap<Symbol, Definition>,
    id: &str,
) -> &'d Definition {
    definitions
        .get(id)
        .unwrap_or_else(|| panic!("internal error: `{id}` has no definition"))
}

pub(in crate::lookup::facts) fn type_query_lookup<'d>(
    definitions: &'d FxHashMap<Symbol, Definition>,
) -> impl Copy + Fn(&str) -> Option<&'d Definition> + 'd {
    move |id| Some(known_definition(definitions, id))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredName<'a>(&'a str);

impl<'a> DeclaredName<'a> {
    pub(crate) fn of(item: &'a Expression) -> Option<Self> {
        match item {
            Expression::Function { name, .. }
            | Expression::Struct { name, .. }
            | Expression::Enum { name, .. }
            | Expression::TypeAlias { name, .. }
            | Expression::Interface { name, .. } => Some(Self(name)),
            Expression::ImplBlock { receiver_name, .. } => Some(Self(receiver_name)),
            Expression::Const { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::PackageImport { .. }
            | body_expression!() => None,
        }
    }

    pub(crate) fn declared(item: &'a Expression) -> Self {
        Self::of(item).expect("internal error: a declaration has a name")
    }

    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}
