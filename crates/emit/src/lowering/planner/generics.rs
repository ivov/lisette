use crate::Planner;
use crate::lookup::state::scope::ScopedTypeParameter;
use syntax::EcoString;
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn set_type_params(&mut self, generics: &[(EcoString, Vec<Type>)]) {
        let type_params = generics
            .iter()
            .map(|(name, bounds)| ScopedTypeParameter {
                name: name.clone(),
                go_name: self.names.generic_go_name(name).into_owned(),
                bounds: bounds.clone(),
            })
            .collect();

        self.scope.set_type_params(type_params);
    }
}
