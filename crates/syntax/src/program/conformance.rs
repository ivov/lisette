use super::{Definition, InterfaceRequirement};
use crate::EcoString;
use crate::types::{Symbol, Type};
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::mem;

#[derive(Debug, Clone, Default)]
pub struct ConformanceIndex {
    witnesses: FxHashMap<(Type, Type), ConformanceWitness>,
}

impl ConformanceIndex {
    pub fn insert<'d>(
        &mut self,
        witness: ConformanceWitness,
        lookup: impl Fn(&str) -> Option<&'d Definition>,
    ) {
        self.witnesses.insert(
            (
                conformance_key(&witness.receiver, &lookup),
                conformance_key(&witness.interface, &lookup),
            ),
            witness,
        );
    }

    pub fn get<'d>(
        &self,
        receiver: &Type,
        interface: &Type,
        lookup: impl Fn(&str) -> Option<&'d Definition>,
    ) -> Option<&ConformanceWitness> {
        self.witnesses.get(&(
            conformance_key(receiver, &lookup),
            conformance_key(interface, &lookup),
        ))
    }

    pub fn merge(&mut self, other: Self) {
        self.witnesses.extend(other.witnesses);
    }

    pub fn visit_types_mut<'d>(
        &mut self,
        mut visit: impl FnMut(&mut Type),
        lookup: impl Fn(&str) -> Option<&'d Definition>,
    ) {
        for mut witness in mem::take(&mut self.witnesses).into_values() {
            witness.visit_types_mut(&mut visit);
            self.insert(witness, &lookup);
        }
    }
}

fn conformance_key<'d>(ty: &Type, lookup: &impl Fn(&str) -> Option<&'d Definition>) -> Type {
    fn normalize<'d>(
        ty: &Type,
        lookup: &impl Fn(&str) -> Option<&'d Definition>,
        expanding: &mut FxHashSet<Symbol>,
    ) -> Type {
        if let Type::Nominal {
            id,
            params: parameters,
            writable,
        } = ty
            && let Some(target) = lookup(id)
                .and_then(|definition| definition.instantiate_alias_target(parameters, *writable))
            && expanding.insert(id.clone())
        {
            let normalized = normalize(&target, lookup, expanding);
            expanding.remove(id);
            return normalized;
        }
        ty.map_children(|child| normalize(child, lookup, expanding))
            .shallow_demoted()
    }
    normalize(ty, lookup, &mut FxHashSet::default()).strip_refs()
}

#[derive(Debug, Clone)]
pub struct CheckedMethod {
    pub required_interface: Symbol,
    pub required_name: EcoString,
    pub selected_definition: Symbol,
    pub selected_name: EcoString,
    pub selected_requirement: Option<InterfaceRequirement>,
    pub signature: Type,
    pub declared_signature: Type,
    pub instantiated_declared_signature: Type,
    pub pointer_receiver: bool,
    pub is_exported: bool,
}
#[derive(Debug, Clone)]
pub struct ConformanceWitness {
    pub receiver: Type,
    pub interface: Type,
    pub methods: Vec<CheckedMethod>,
}
impl ConformanceWitness {
    pub fn method_for(&self, requirement: &InterfaceRequirement) -> &CheckedMethod {
        self.methods
            .iter()
            .find(|method| {
                method.required_interface == requirement.declaring_interface
                    && method.required_name == requirement.name
            })
            .expect("internal error: a checked conformance selects every required method")
    }

    pub fn visit_types_mut(&mut self, mut visit: impl FnMut(&mut Type)) {
        visit(&mut self.receiver);
        visit(&mut self.interface);
        for method in &mut self.methods {
            visit(&mut method.signature);
            visit(&mut method.declared_signature);
            visit(&mut method.instantiated_declared_signature);
            if let Some(requirement) = &mut method.selected_requirement {
                visit(&mut requirement.ty);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ref_alias_keys_match_after_freezing_at_every_type_depth() {
        use crate::ast::{Annotation, Visibility};
        use crate::program::{AliasKind, DefinitionBody};
        use crate::types::CompoundKind;
        let nominal = |id: &str, parameters| Type::Nominal {
            id: id.into(),
            params: parameters,
            writable: false,
        };
        let item = nominal("a.Item", vec![]);
        let reference = Type::Compound {
            kind: CompoundKind::Ref,
            args: vec![item.clone()],
            writable: false,
        };
        let alias = nominal("a.ItemRef", vec![]);
        let definition = Definition {
            visibility: Visibility::Public,
            ty: alias.clone(),
            name_span: None,
            doc: None,
            body: DefinitionBody::TypeAlias {
                generics: vec![],
                alias: AliasKind::Transparent {
                    annotation: Annotation::Unknown,
                    target: reference.clone(),
                },
                methods: Default::default(),
                attributes: Default::default(),
            },
        };
        let lookup = |id: &str| (id == "a.ItemRef").then_some(&definition);
        let aliased_interface = nominal("api.View", vec![alias.clone()]);
        let expanded_interface = nominal("api.View", vec![reference.clone()]);
        let mut index = ConformanceIndex::default();
        index.insert(
            ConformanceWitness {
                receiver: alias.clone(),
                interface: aliased_interface.clone(),
                methods: vec![],
            },
            lookup,
        );
        assert!(index.get(&item, &expanded_interface, lookup).is_some());
        assert!(
            index
                .get(
                    &reference.clone().make_writable(),
                    &aliased_interface,
                    lookup
                )
                .is_some()
        );
        index.visit_types_mut(
            |ty| {
                if *ty == alias {
                    *ty = reference.clone();
                }
                if *ty == aliased_interface {
                    *ty = expanded_interface.clone();
                }
            },
            lookup,
        );
        assert!(index.get(&alias, &aliased_interface, lookup).is_some());
        assert!(
            index
                .get(&item, &nominal("api.View", vec![item.clone()]), lookup)
                .is_none(),
            "nested references are significant"
        );
        assert!(
            index
                .get(&nominal("b.Item", vec![]), &expanded_interface, lookup)
                .is_none(),
            "package identity is significant"
        );
        index.insert(
            ConformanceWitness {
                receiver: item,
                interface: expanded_interface,
                methods: vec![],
            },
            lookup,
        );
        assert_eq!(index.witnesses.len(), 1);
    }

    #[test]
    fn repeated_and_permission_qualified_checks_share_one_witness() {
        let receiver = Type::Nominal {
            id: "a.Item".into(),
            params: vec![],
            writable: false,
        };
        let interface = Type::Nominal {
            id: "api.View".into(),
            params: vec![receiver.clone()],
            writable: false,
        };
        let mut index = ConformanceIndex::default();
        for receiver in [
            receiver.clone(),
            receiver.clone().make_writable(),
            receiver.clone(),
        ] {
            index.insert(
                ConformanceWitness {
                    receiver,
                    interface: interface.clone(),
                    methods: vec![],
                },
                |_| None,
            );
        }
        assert_eq!(index.witnesses.len(), 1);
        assert!(index.get(&receiver, &interface, |_| None).is_some());
        let other = Type::Nominal {
            id: "b.Item".into(),
            params: vec![],
            writable: false,
        };
        assert!(index.get(&other, &interface, |_| None).is_none());
    }
}
