use rustc_hash::FxHashMap as HashMap;
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

use crate::abi::callable::CallableReturnAbi;
use crate::abi::layout::SlotOrigin;
use crate::classify_go_return_type;
use crate::names::go_name;

#[derive(Debug, Default)]
pub(crate) struct GoAbiCatalog {
    callables: HashMap<String, GoCallableSlots>,
    fields: HashMap<String, HashMap<String, GoSlotDescriptor>>,
}

#[derive(Debug)]
struct GoCallableSlots {
    parameters: Vec<GoSlotDescriptor>,
    return_slot: GoSlotDescriptor,
    return_abi: Option<CallableReturnAbi>,
}

#[derive(Debug, Clone)]
pub(crate) struct GoSlotDescriptor {
    pub(crate) origin: SlotOrigin,
    pub(crate) declared_type: Type,
}

impl GoAbiCatalog {
    pub(crate) fn from_definitions(definitions: &HashMap<Symbol, Definition>) -> Self {
        let mut catalog = Self::default();
        for (qualified_name, definition) in definitions {
            if !go_name::is_go_import(qualified_name) {
                continue;
            }
            catalog.register_callable(
                definitions,
                qualified_name,
                &definition.ty,
                definition.go_hints(),
            );
            if let Some(methods) = definition.methods() {
                for method in methods.values() {
                    catalog.register_callable(
                        definitions,
                        &format!("{qualified_name}.{}", method.source_name),
                        &method.ty,
                        &method.go_hints,
                    );
                }
            }
            catalog.register_fields(definitions, qualified_name, definition);
        }
        catalog
    }

    pub(crate) fn callable_parameter(
        &self,
        qualified_name: &str,
        index: usize,
    ) -> Option<&GoSlotDescriptor> {
        self.callables.get(qualified_name)?.parameters.get(index)
    }

    pub(crate) fn callable_return_slot(&self, qualified_name: &str) -> Option<&GoSlotDescriptor> {
        self.callables
            .get(qualified_name)
            .map(|callable| &callable.return_slot)
    }

    pub(crate) fn callable_return_abi(&self, qualified_name: &str) -> Option<&CallableReturnAbi> {
        self.callables.get(qualified_name)?.return_abi.as_ref()
    }

    pub(crate) fn field(&self, owner: &str, field: &str) -> Option<&GoSlotDescriptor> {
        self.fields.get(owner)?.get(field)
    }

    pub(crate) fn is_imported_type(&self, qualified_name: &str) -> bool {
        self.fields.contains_key(qualified_name)
    }

    fn register_callable(
        &mut self,
        definitions: &HashMap<Symbol, Definition>,
        qualified_name: &str,
        ty: &Type,
        go_hints: &[String],
    ) {
        let Type::Function(function) = ty.unwrap_forall() else {
            return;
        };
        let parameters = function
            .params
            .iter()
            .map(|parameter| GoSlotDescriptor {
                origin: SlotOrigin::go_parameter(syntax::types::resolves_to_unknown(
                    &parameter.ty,
                    |id| definitions.get(id),
                )),
                declared_type: parameter.ty.clone(),
            })
            .collect();
        let return_slot = GoSlotDescriptor {
            origin: SlotOrigin::go_return(syntax::types::resolves_to_unknown(
                &function.return_type,
                |id| definitions.get(id),
            )),
            declared_type: (*function.return_type).clone(),
        };
        let return_abi = classify_go_return_type(definitions, &function.return_type, go_hints);
        self.callables.insert(
            qualified_name.to_string(),
            GoCallableSlots {
                parameters,
                return_slot,
                return_abi,
            },
        );
    }

    fn register_fields(
        &mut self,
        definitions: &HashMap<Symbol, Definition>,
        qualified_name: &str,
        definition: &Definition,
    ) {
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return;
        };
        let slots = self.fields.entry(qualified_name.to_string()).or_default();
        for field in fields {
            slots.insert(
                field.name.to_string(),
                GoSlotDescriptor {
                    origin: SlotOrigin::go_field(syntax::types::resolves_to_unknown(
                        &field.ty,
                        |id| definitions.get(id),
                    )),
                    declared_type: field.ty.clone(),
                },
            );
        }
    }
}
