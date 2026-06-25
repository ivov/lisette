use syntax::program::Definition;
use syntax::types::{Symbol, Type};

use crate::Planner;

pub(crate) struct ResolvedNominal<'a> {
    pub(crate) id: Symbol,
    pub(crate) definition: &'a Definition,
}

impl<'a> Planner<'a> {
    pub(crate) fn resolve_nominal(&self, ty: &Type) -> Option<ResolvedNominal<'a>> {
        let Type::Nominal { id, .. } = self.facts.strip_and_peel(ty) else {
            return None;
        };
        let definition = self.facts.definition(id.as_str())?;
        Some(ResolvedNominal { id, definition })
    }
}
