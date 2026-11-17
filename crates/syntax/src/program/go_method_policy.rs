use crate::types::GO_IMPORT_PREFIX;

use super::{Definition, DefinitionBody, interface_requirements};
use crate::go_names::snake_to_camel;
use crate::types::{Symbol, Type, peel_alias, unqualified_name};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct GoMethodPolicy {
    pub exported_method_names: BTreeSet<String>,
    pub tagged_method_names: BTreeSet<String>,
}

impl GoMethodPolicy {
    pub fn for_definitions<'a>(
        &self,
        definitions: impl Iterator<Item = (&'a Symbol, &'a Definition)>,
    ) -> Self {
        let mut selectors = BTreeSet::new();
        for (name, definition) in definitions {
            if let Some(methods) = definition.methods() {
                selectors.extend(
                    methods
                        .values()
                        .map(|method| method.source_name.to_string()),
                );
            }
            if matches!(definition.body, DefinitionBody::Value { .. })
                && name.split('.').nth(2).is_some()
            {
                selectors.insert(unqualified_name(name).to_string());
            }
        }
        Self {
            exported_method_names: self
                .exported_method_names
                .intersection(&selectors)
                .cloned()
                .collect(),
            tagged_method_names: selectors
                .iter()
                .map(|name| snake_to_camel(name))
                .filter(|name| self.tagged_method_names.contains(name))
                .collect(),
        }
    }

    pub fn compute<'a>(
        definitions: impl Iterator<Item = (&'a Symbol, &'a Definition)>,
        lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
    ) -> Self {
        let mut policy = Self::default();
        for (key, definition) in definitions {
            policy.register_exported_methods(key, definition);
            policy.register_bound_returns(definition, lookup);
        }
        policy
    }

    fn register_bound_returns<'a>(
        &mut self,
        definition: &Definition,
        lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
    ) {
        let function_bounds = definition
            .ty
            .as_function_type()
            .into_iter()
            .flat_map(|function| &function.bounds)
            .map(|bound| &bound.ty);
        let type_bounds = definition
            .body
            .generics()
            .into_iter()
            .flatten()
            .flat_map(|generic| generic.resolved_bounds().into_iter().flatten());
        let method_bounds = definition
            .methods()
            .into_iter()
            .flat_map(|methods| methods.values())
            .filter_map(|method| method.ty.as_function_type())
            .flat_map(|function| &function.bounds)
            .map(|bound| &bound.ty);
        for bound in function_bounds.chain(type_bounds).chain(method_bounds) {
            for requirement in interface_requirements(bound, |id| lookup(id)) {
                if requirement
                    .declaring_interface
                    .starts_with(GO_IMPORT_PREFIX)
                {
                    continue;
                }
                let Some(return_ty) = requirement.method.ty.unwrap_forall().get_function_ret()
                else {
                    continue;
                };
                if matches!(peel_alias(return_ty, |id| lookup(id)), Type::Parameter(_)) {
                    self.tagged_method_names
                        .insert(snake_to_camel(&requirement.name));
                }
            }
        }
    }

    fn register_exported_methods(&mut self, key: &Symbol, definition: &Definition) {
        let is_user_definition = !key.starts_with(GO_IMPORT_PREFIX) && !key.starts_with("prelude.");
        match &definition.body {
            DefinitionBody::Interface {
                definition: interface_definition,
                ..
            } if definition.visibility.is_public() => {
                for method_name in interface_definition.methods.keys() {
                    self.exported_method_names.insert(method_name.to_string());
                }
            }
            DefinitionBody::Value { .. }
                if definition.visibility.is_public()
                    && is_user_definition
                    && key.split('.').nth(2).is_some() =>
            {
                let method_name = unqualified_name(key);
                self.exported_method_names.insert(method_name.to_string());
            }
            _ => {}
        }

        if is_user_definition && let Some(methods) = definition.methods() {
            for method in methods
                .values()
                .filter(|method| method.visibility.is_public())
            {
                self.exported_method_names
                    .insert(method.source_name.to_string());
            }
        }

        if definition.visibility.is_public() && definition.is_display() {
            self.exported_method_names.insert("to_string".to_string());
        }
    }
}
