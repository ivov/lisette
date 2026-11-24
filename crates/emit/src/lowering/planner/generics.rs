use crate::Planner;
use crate::lookup::state::scope::TypeParam;
use crate::lowering::definitions::interface_adapter::bound_needs_dispatch;
use crate::target::ir::declarations::TypeParameter;
use crate::target::names::go_name;
use crate::target::names::packages::PackageUse;
use crate::target::names::receiver::ReceiverGenerics;
use crate::target::types::go_type::GoType;
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

    pub(in crate::lowering) fn plan_generics(
        &self,
        generics: &[(EcoString, Vec<Type>)],
    ) -> Vec<TypeParameter> {
        generics
            .iter()
            .map(|(name, bounds)| {
                let has_ordered = bounds.iter().any(is_ordered_bound);
                let mut planned = Vec::new();
                for bound in bounds {
                    if bound_needs_dispatch(&self.facts, bound) || is_zeroable_bound(bound) {
                        continue;
                    }
                    let ty = if is_comparable_bound(bound) {
                        if has_ordered {
                            continue;
                        }
                        GoType::new("comparable")
                    } else if is_ordered_bound(bound) {
                        GoType::named(
                            "Ordered",
                            &[],
                            Some(PackageUse::generated(go_name::GeneratedPackage::Cmp)),
                        )
                    } else {
                        self.go_type(bound)
                    };
                    if !planned.contains(&ty) {
                        planned.push(ty);
                    }
                }
                TypeParameter {
                    name: self.generic_go_name(name).into_owned(),
                    bounds: planned,
                }
            })
            .collect()
    }
}

fn is_zeroable_bound(bound: &Type) -> bool {
    bound.get_qualified_id() == Some("prelude.Zeroable")
}

fn is_comparable_bound(bound: &Type) -> bool {
    bound.get_qualified_id() == Some("prelude.Comparable")
}

fn is_ordered_bound(bound: &Type) -> bool {
    matches!(
        bound.get_qualified_id(),
        Some("prelude.Ordered" | "go:cmp.Ordered")
    )
}
