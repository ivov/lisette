use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::declarations::TypeParameter;
use crate::target::names::go_name;
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;
use syntax::EcoString;
use syntax::types::Type;

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

impl PlannerQueries<'_, '_> {
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
                    if is_zeroable_bound(bound) {
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
                    name: self.names.generic_go_name(name).into_owned(),
                    bounds: planned,
                }
            })
            .collect()
    }
}
