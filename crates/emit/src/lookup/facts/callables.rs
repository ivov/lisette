use syntax::program::{Definition, GoHint, GoTypeRecipe, Method, Visibility};
use syntax::types::Type;

#[derive(Debug, Clone, Copy)]
pub(crate) enum CallableDeclaration<'a> {
    Definition(&'a Definition),
    Method(&'a Method),
}

impl<'a> CallableDeclaration<'a> {
    pub(crate) fn ty(self) -> &'a Type {
        match self {
            Self::Definition(definition) => &definition.ty,
            Self::Method(method) => &method.ty,
        }
    }

    pub(crate) fn visibility(self) -> &'a Visibility {
        match self {
            Self::Definition(definition) => &definition.visibility,
            Self::Method(method) => &method.visibility,
        }
    }

    pub(crate) fn is_type_definition(self) -> bool {
        matches!(self, Self::Definition(definition) if definition.is_type_definition())
    }

    pub(crate) fn go_hints(self) -> &'a [GoHint] {
        match self {
            Self::Definition(definition) => definition.go_hints(),
            Self::Method(method) => &method.go_hints,
        }
    }

    pub(crate) fn go_name(self) -> Option<&'a str> {
        match self {
            Self::Definition(definition) => definition.go_name(),
            Self::Method(_) => None,
        }
    }

    pub(crate) fn go_type_param_recipe(self) -> Option<&'a [GoTypeRecipe]> {
        match self {
            Self::Definition(definition) => definition.go_type_param_recipe(),
            Self::Method(_) => None,
        }
    }
}
