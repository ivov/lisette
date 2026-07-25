use rustc_hash::FxHashSet as HashSet;

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
    ContainmentWalk {
        lookup: &lookup,
        enum_payloads: EnumPayloads::Traverse,
        severed: &severed,
    }
    .payload_pointer_wrapped(enum_id, variant, field, payload, &HashSet::default())
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
    ContainmentWalk {
        lookup: &lookup,
        enum_payloads,
        severed,
    }
    .definition_contains(
        current_id,
        target_id,
        &mut HashSet::default(),
        &HashSet::default(),
    )
}

struct ContainmentWalk<'w, F> {
    lookup: &'w F,
    enum_payloads: EnumPayloads,
    severed: &'w HashSet<String>,
}

impl<'d, F: Fn(&str) -> Option<&'d Definition>> ContainmentWalk<'_, F> {
    fn type_contains(
        &self,
        ty: &Type,
        target_id: &str,
        visited: &mut HashSet<String>,
        wrap_checking: &HashSet<(String, usize, usize)>,
    ) -> bool {
        let peeled = peel_alias(ty, self.lookup);
        match &peeled {
            Type::Nominal { id, params, .. } => {
                if is_indirection_type(id.as_str()) {
                    return false;
                }

                if id == target_id {
                    return true;
                }

                for (position, param) in params.iter().enumerate() {
                    if self.argument_stored_inline(
                        id.as_str(),
                        position,
                        &mut HashSet::default(),
                        wrap_checking,
                    ) && self.type_contains(param, target_id, visited, wrap_checking)
                    {
                        return true;
                    }
                }

                self.definition_contains(id.as_str(), target_id, visited, wrap_checking)
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|e| self.type_contains(e, target_id, visited, wrap_checking)),
            Type::Array { element, .. } => {
                self.type_contains(element, target_id, visited, wrap_checking)
            }
            _ => false,
        }
    }

    fn definition_contains(
        &self,
        current_id: &str,
        target_id: &str,
        visited: &mut HashSet<String>,
        wrap_checking: &HashSet<(String, usize, usize)>,
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
                .any(|field| self.type_contains(&field.ty, target_id, visited, wrap_checking)),
            Some(DefinitionBody::Enum { variants, .. })
                if self.enum_payloads == EnumPayloads::Traverse =>
            {
                variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter())
                    .any(|field| self.type_contains(&field.ty, target_id, visited, wrap_checking))
            }
            _ => false,
        }
    }

    fn argument_stored_inline(
        &self,
        id: &str,
        position: usize,
        checking: &mut HashSet<(String, usize)>,
        wrap_checking: &HashSet<(String, usize, usize)>,
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
                fields.iter().any(|field| {
                    self.parameter_stored_inline(&field.ty, &generic.name, checking, wrap_checking)
                })
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
                        !self.payload_pointer_wrapped(id, vi, fi, &field.ty, wrap_checking)
                            && self.parameter_stored_inline(
                                &field.ty,
                                &generic.name,
                                checking,
                                wrap_checking,
                            )
                    })
            }
            DefinitionBody::TypeAlias {
                generics, alias, ..
            } => {
                let Some(generic) = generics.get(position) else {
                    return true;
                };
                match alias {
                    crate::program::AliasKind::Transparent { target, .. } => {
                        self.parameter_stored_inline(target, &generic.name, checking, wrap_checking)
                    }
                    crate::program::AliasKind::Opaque(_) => true,
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
        wrap_checking: &HashSet<(String, usize, usize)>,
    ) -> bool {
        let key = (enum_id.to_string(), variant, field);
        if wrap_checking.contains(&key) {
            return false;
        }
        let mut nested_checking = wrap_checking.clone();
        nested_checking.insert(key);
        let severed = HashSet::default();
        ContainmentWalk {
            lookup: self.lookup,
            enum_payloads: EnumPayloads::Traverse,
            severed: &severed,
        }
        .type_contains(payload, enum_id, &mut HashSet::default(), &nested_checking)
    }

    fn parameter_stored_inline(
        &self,
        ty: &Type,
        parameter: &str,
        checking: &mut HashSet<(String, usize)>,
        wrap_checking: &HashSet<(String, usize, usize)>,
    ) -> bool {
        match ty {
            Type::Parameter(name) => name == parameter,
            Type::Nominal { id, params, .. } => {
                if is_indirection_type(id.as_str()) {
                    return false;
                }
                params.iter().enumerate().any(|(position, argument)| {
                    self.parameter_stored_inline(argument, parameter, checking, wrap_checking)
                        && self.argument_stored_inline(
                            id.as_str(),
                            position,
                            checking,
                            wrap_checking,
                        )
                })
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|e| self.parameter_stored_inline(e, parameter, checking, wrap_checking)),
            Type::Array { element, .. } => {
                self.parameter_stored_inline(element, parameter, checking, wrap_checking)
            }
            _ => false,
        }
    }
}

fn is_indirection_type(id: &str) -> bool {
    CompoundKind::from_name(id.strip_prefix("prelude.").unwrap_or(id)).is_some()
}
