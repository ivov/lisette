use crate::Planner;
use crate::lookup::state::scope::TypeParam;
use crate::target::names::go_name;
use crate::target::names::receiver::ReceiverGenerics;
use std::borrow::Cow;
use syntax::EcoString;
use syntax::ast::Generic;
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn set_type_params(&mut self, generics: &[(EcoString, Vec<Type>)]) {
        let type_params = generics
            .iter()
            .map(|(name, bounds)| TypeParam {
                name: name.clone(),
                go_name: self.generic_go_name(name).into_owned(),
                bounds: bounds.clone(),
            })
            .collect();
        self.scope.set_type_params(type_params);
    }

    pub(in crate::lowering) fn generic_go_name<'argument>(
        &'argument self,
        source_name: &'argument str,
    ) -> Cow<'argument, str> {
        match self.names.generic_rename(source_name) {
            Some(renamed) => Cow::Borrowed(renamed),
            None => go_name::escape_type_name(source_name),
        }
    }

    pub(in crate::lowering) fn receiver_generics(&self, generics: &[Generic]) -> ReceiverGenerics {
        ReceiverGenerics(
            generics
                .iter()
                .map(|generic| self.generic_go_name(&generic.name).into_owned())
                .collect(),
        )
    }
}
