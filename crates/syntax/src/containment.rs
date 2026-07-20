use rustc_hash::FxHashSet as HashSet;
use std::cell::RefCell;

use crate::program::{Definition, DefinitionBody};
use crate::types::{CompoundKind, Type, peel_alias};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnumPayloads {
    Traverse,
    Skip,
}

pub fn enum_payload_pointer_wrapped<'d, F>(
    enum_id: &str,
    variant: usize,
    field: usize,
    payload: &Type,
    lookup: F,
) -> bool
where
    F: Fn(&str) -> Option<&'d Definition>,
{
    let severed = HashSet::default();
    let wrap_checking = RefCell::new(HashSet::default());
    ContainmentWalk {
        lookup: &lookup,
        enum_payloads: EnumPayloads::Traverse,
        severed: &severed,
        wrap_checking: &wrap_checking,
    }
    .payload_pointer_wrapped(enum_id, variant, field, payload)
}

pub fn definition_contains_by_value<'d, F>(
    current_id: &str,
    target_id: &str,
    enum_payloads: EnumPayloads,
    severed: &HashSet<String>,
    lookup: F,
) -> bool
where
    F: Fn(&str) -> Option<&'d Definition>,
{
    let wrap_checking = RefCell::new(HashSet::default());
    ContainmentWalk {
        lookup: &lookup,
        enum_payloads,
        severed,
        wrap_checking: &wrap_checking,
    }
    .definition_contains(current_id, target_id, &mut HashSet::default())
}

struct ContainmentWalk<'w, F> {
    lookup: &'w F,
    enum_payloads: EnumPayloads,
    severed: &'w HashSet<String>,
    wrap_checking: &'w RefCell<HashSet<(String, usize, usize)>>,
}

impl<'d, F: Fn(&str) -> Option<&'d Definition>> ContainmentWalk<'_, F> {
    fn type_contains(&self, ty: &Type, target_id: &str, visited: &mut HashSet<String>) -> bool {
        let peeled = peel_alias(ty, |id| {
            (self.lookup)(id).is_some_and(Definition::is_type_alias)
        });
        match &peeled {
            Type::Nominal { id, params, .. } => {
                if is_indirection_type(id.as_str()) {
                    return false;
                }

                if id == target_id {
                    return true;
                }

                for (position, param) in params.iter().enumerate() {
                    if self.argument_stored_inline(id.as_str(), position, &mut HashSet::default())
                        && self.type_contains(param, target_id, visited)
                    {
                        return true;
                    }
                }

                self.definition_contains(id.as_str(), target_id, visited)
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|e| self.type_contains(e, target_id, visited)),
            Type::Array { element, .. } => self.type_contains(element, target_id, visited),
            _ => false,
        }
    }

    fn definition_contains(
        &self,
        current_id: &str,
        target_id: &str,
        visited: &mut HashSet<String>,
    ) -> bool {
        if self.severed.contains(current_id) {
            return false;
        }
        if !visited.insert(current_id.to_string()) {
            return false;
        }
        match (self.lookup)(current_id).map(|d| &d.body) {
            Some(DefinitionBody::Struct { fields, .. }) => fields
                .iter()
                .any(|field| self.type_contains(&field.ty, target_id, visited)),
            Some(DefinitionBody::Enum { variants, .. })
                if self.enum_payloads == EnumPayloads::Traverse =>
            {
                variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter())
                    .any(|field| self.type_contains(&field.ty, target_id, visited))
            }
            _ => false,
        }
    }

    fn argument_stored_inline(
        &self,
        id: &str,
        position: usize,
        checking: &mut HashSet<(String, usize)>,
    ) -> bool {
        if !checking.insert((id.to_string(), position)) {
            return false;
        }
        let Some(definition) = (self.lookup)(id) else {
            return true;
        };
        match &definition.body {
            DefinitionBody::Struct {
                generics, fields, ..
            } => {
                let Some(generic) = generics.get(position) else {
                    return true;
                };
                fields
                    .iter()
                    .any(|field| self.parameter_stored_inline(&field.ty, &generic.name, checking))
            }
            DefinitionBody::Enum {
                generics, variants, ..
            } => {
                let Some(generic) = generics.get(position) else {
                    return true;
                };
                variants
                    .iter()
                    .enumerate()
                    .flat_map(|(vi, variant)| {
                        variant
                            .fields
                            .iter()
                            .enumerate()
                            .map(move |(fi, field)| (vi, fi, field))
                    })
                    .any(|(vi, fi, field)| {
                        !self.payload_pointer_wrapped(id, vi, fi, &field.ty)
                            && self.parameter_stored_inline(&field.ty, &generic.name, checking)
                    })
            }
            DefinitionBody::TypeAlias { .. } => {
                let Type::Forall { vars, body } = &definition.ty else {
                    return true;
                };
                let Some(name) = vars.get(position) else {
                    return true;
                };
                match body.as_ref() {
                    Type::Nominal {
                        id: body_id,
                        underlying_ty,
                        ..
                    } if body_id.as_str() == id => match underlying_ty {
                        Some(underlying) => {
                            self.parameter_stored_inline(underlying, name, checking)
                        }
                        None => true,
                    },
                    _ => self.parameter_stored_inline(body, name, checking),
                }
            }
            DefinitionBody::Interface { .. } => false,
            _ => true,
        }
    }

    fn payload_pointer_wrapped(
        &self,
        enum_id: &str,
        variant: usize,
        field: usize,
        payload: &Type,
    ) -> bool {
        let key = (enum_id.to_string(), variant, field);
        if !self.wrap_checking.borrow_mut().insert(key.clone()) {
            return false;
        }
        let severed = HashSet::default();
        let wrapped = ContainmentWalk {
            lookup: self.lookup,
            enum_payloads: EnumPayloads::Traverse,
            severed: &severed,
            wrap_checking: self.wrap_checking,
        }
        .type_contains(payload, enum_id, &mut HashSet::default());
        self.wrap_checking.borrow_mut().remove(&key);
        wrapped
    }

    fn parameter_stored_inline(
        &self,
        ty: &Type,
        parameter: &str,
        checking: &mut HashSet<(String, usize)>,
    ) -> bool {
        match ty {
            Type::Parameter(name) => name == parameter,
            Type::Nominal { id, params, .. } => {
                if is_indirection_type(id.as_str()) {
                    return false;
                }
                params.iter().enumerate().any(|(position, argument)| {
                    self.parameter_stored_inline(argument, parameter, checking)
                        && self.argument_stored_inline(id.as_str(), position, checking)
                })
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|e| self.parameter_stored_inline(e, parameter, checking)),
            Type::Array { element, .. } => {
                self.parameter_stored_inline(element, parameter, checking)
            }
            _ => false,
        }
    }
}

fn is_indirection_type(id: &str) -> bool {
    CompoundKind::from_name(id.strip_prefix("prelude.").unwrap_or(id)).is_some()
}
