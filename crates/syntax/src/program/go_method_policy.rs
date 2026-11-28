use super::{Definition, DefinitionBody, InterfaceRequirement, interface_requirements};
use super::{is_go_import, is_prelude_symbol};
use crate::go_names::snake_to_camel;
use crate::types::{Symbol, Type, peel_alias, unqualified_name};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct GoMethodPolicy {
    pub exported_method_names: BTreeSet<String>,
    pub tagged_method_names: BTreeSet<String>,
}

impl GoMethodPolicy {
    pub fn exports_method(&self, method_name: &str) -> bool {
        self.exported_method_names.contains(method_name)
            || matches!(method_name, "string" | "goString" | "error")
    }

    pub fn for_definitions<'a>(
        &self,
        definitions: impl Iterator<Item = (&'a Symbol, &'a Definition)>,
    ) -> Self {
        let mut selectors: Vec<&str> = Vec::new();
        for (name, definition) in definitions {
            if let Some(methods) = definition.methods() {
                selectors.extend(methods.values().map(|method| method.source_name.as_str()));
            }
            if matches!(definition.body, DefinitionBody::Value { .. })
                && name.bytes().filter(|byte| *byte == b'.').nth(1).is_some()
            {
                selectors.push(unqualified_name(name));
            }
        }
        selectors.sort_unstable();
        selectors.dedup();
        let tagged_method_names = if self.tagged_method_names.is_empty() {
            BTreeSet::new()
        } else {
            selectors
                .iter()
                .map(|name| snake_to_camel(name))
                .filter(|name| self.tagged_method_names.contains(name))
                .collect()
        };
        Self {
            exported_method_names: selectors
                .iter()
                .filter(|name| self.exported_method_names.contains(**name))
                .map(|name| (*name).to_string())
                .collect(),
            tagged_method_names,
        }
    }

    pub fn compute<'a>(
        definitions: impl Iterator<Item = (&'a Symbol, &'a Definition)>,
        lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
    ) -> Self {
        let mut policy = Self::default();
        for (key, definition) in definitions {
            policy.register_exported_methods(key, definition);
            policy.register_tagged_interface_methods(key, definition, lookup);
        }
        policy
    }

    fn register_tagged_interface_methods<'a>(
        &mut self,
        key: &Symbol,
        definition: &Definition,
        lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
    ) {
        if is_go_import(key) {
            return;
        }
        let DefinitionBody::Interface {
            definition: interface_definition,
        } = &definition.body
        else {
            return;
        };
        for method in interface_definition.methods.values() {
            if interface_method_is_tagged(&method.ty, lookup) {
                self.tagged_method_names
                    .insert(snake_to_camel(&method.source_name));
            }
        }
    }

    fn register_exported_methods(&mut self, key: &Symbol, definition: &Definition) {
        let is_user_definition = !is_go_import(key) && !is_prelude_symbol(key);
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

pub fn interface_method_is_tagged<'a>(
    method_ty: &Type,
    lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
) -> bool {
    let Some(return_ty) = method_ty.unwrap_forall().get_function_ret() else {
        return false;
    };
    let peeled = peel_alias(return_ty, lookup);
    match &peeled {
        Type::Parameter(_) => true,
        Type::Nominal { params, .. } if peeled.is_option() || peeled.is_result() => {
            params.iter().any(Type::contains_type_parameter)
        }
        _ => peeled.is_partial(),
    }
}

pub fn requirement_uses_tagged_counterpart<'a>(
    requirement: &InterfaceRequirement,
    lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
) -> bool {
    !is_go_import(&requirement.declaring_interface)
        && interface_method_is_tagged(&requirement.method.ty, lookup)
}

pub fn supplying_requirement<'a>(
    suppliers: &[Type],
    name: &str,
    wanted: Option<&InterfaceRequirement>,
    lookup: impl Fn(&str) -> Option<&'a Definition> + Copy,
) -> Option<InterfaceRequirement> {
    let mut candidates: Vec<InterfaceRequirement> = suppliers
        .iter()
        .flat_map(|supplier| interface_requirements(supplier, lookup))
        .filter(|candidate| candidate.name == name)
        .collect();
    let index = wanted
        .and_then(|wanted| {
            let tagged = requirement_uses_tagged_counterpart(wanted, lookup);
            candidates.iter().position(|candidate| {
                requirement_uses_tagged_counterpart(candidate, lookup) == tagged
            })
        })
        .unwrap_or(0);
    (index < candidates.len()).then(|| candidates.swap_remove(index))
}
