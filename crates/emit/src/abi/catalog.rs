use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
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
    imported_types: HashSet<String>,
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
            catalog.register_callable(definitions, qualified_name, definition);
            catalog.register_fields(qualified_name, definition);
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
        self.imported_types.contains(qualified_name)
    }

    fn register_callable(
        &mut self,
        definitions: &HashMap<Symbol, Definition>,
        qualified_name: &str,
        definition: &Definition,
    ) {
        let Type::Function(function) = definition.ty.unwrap_forall() else {
            return;
        };
        let parameters = function
            .params
            .iter()
            .map(|parameter| GoSlotDescriptor {
                origin: SlotOrigin::go_parameter(parameter),
                declared_type: parameter.clone(),
            })
            .collect();
        let return_slot = GoSlotDescriptor {
            origin: SlotOrigin::go_return(&function.return_type),
            declared_type: (*function.return_type).clone(),
        };
        let return_abi =
            classify_go_return_type(definitions, &function.return_type, definition.go_hints());
        self.callables.insert(
            qualified_name.to_string(),
            GoCallableSlots {
                parameters,
                return_slot,
                return_abi,
            },
        );
    }

    fn register_fields(&mut self, qualified_name: &str, definition: &Definition) {
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return;
        };
        self.imported_types.insert(qualified_name.to_string());
        let slots = self.fields.entry(qualified_name.to_string()).or_default();
        for field in fields {
            slots.insert(
                field.name.to_string(),
                GoSlotDescriptor {
                    origin: SlotOrigin::go_field(&field.ty),
                    declared_type: field.ty.clone(),
                },
            );
        }
    }
}
