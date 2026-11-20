use crate::ir::declarations::TypeParameter;
use crate::names::packages::PackageUse;
use crate::names::receiver::ReceiverGenerics;
use std::borrow::Cow;

use rustc_hash::FxHashMap as HashMap;

use crate::Planner;
use crate::names::go_name;
use crate::state::scope::TypeParam;
use crate::types::go_type::GoType;
use syntax::EcoString;
use syntax::ast::{Expression, Generic};
use syntax::types::{SubstitutionMap, Type};

impl Planner<'_> {
    pub(crate) fn format_value_type_args(&mut self, expression: &Expression) -> Option<GoType> {
        let recorded = self.facts.checked(expression).function_value()?;
        let declaration = self
            .facts
            .callable_declaration(recorded.definition.as_deref()?)?;
        let instantiation = &recorded.type_arguments;
        match declaration.go_type_param_recipe() {
            Some(recipe) => self.reconstruct_collapsed_type_args(recipe, instantiation),
            None => self.format_generic_instantiation(declaration.ty(), instantiation),
        }
    }

    pub(crate) fn format_generic_instantiation(
        &mut self,
        definition_ty: &Type,
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let Type::Forall { vars, .. } = definition_ty else {
            return None;
        };
        if vars.is_empty() {
            return None;
        }

        let arguments: Vec<GoType> = vars
            .iter()
            .filter_map(|var| Some(self.go_type(mapping.get(var.as_str())?)))
            .collect();

        if arguments.len() != vars.len()
            || arguments
                .iter()
                .any(|argument| argument.contains_empty_interface())
        {
            return None;
        }

        Some(GoType::type_arguments(&arguments))
    }

    pub(crate) fn set_type_params(&mut self, generics: &[(EcoString, Vec<Type>)]) {
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

    pub(crate) fn generic_go_name<'argument>(
        &'argument self,
        source_name: &'argument str,
    ) -> Cow<'argument, str> {
        match self.package.generic_rename(source_name) {
            Some(renamed) => Cow::Borrowed(renamed),
            None => go_name::escape_type_name(source_name),
        }
    }

    pub(crate) fn receiver_generics(&self, generics: &[Generic]) -> ReceiverGenerics {
        ReceiverGenerics(
            generics
                .iter()
                .map(|generic| self.generic_go_name(&generic.name).into_owned())
                .collect(),
        )
    }

    pub(crate) fn plan_generics(&self, generics: &[(EcoString, Vec<Type>)]) -> Vec<TypeParameter> {
        generics
            .iter()
            .map(|(name, bounds)| {
                let has_ordered = bounds.iter().any(is_ordered_bound);
                let mut planned = Vec::new();
                for bound in bounds {
                    if self.facts.bound_needs_dispatch(bound) || is_zeroable_bound(bound) {
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

pub(crate) fn extract_type_mapping(
    generic: &Type,
    concrete: &Type,
    mapping: &mut HashMap<String, Type>,
) {
    if let Type::Parameter(name) = generic {
        mapping
            .entry(name.to_string())
            .or_insert_with(|| concrete.clone());
        return;
    }

    if let (Some(gen_params), Some(conc_params)) =
        (generic.get_type_params(), concrete.get_type_params())
    {
        for (generic, c) in gen_params.iter().zip(conc_params.iter()) {
            extract_type_mapping(generic, c, mapping);
        }
        return;
    }

    match (generic, concrete) {
        (Type::Function(gen_f), Type::Function(conc_f)) => {
            for (generic, c) in gen_f.params.iter().zip(conc_f.params.iter()) {
                extract_type_mapping(&generic.ty, &c.ty, mapping);
            }
            extract_type_mapping(&gen_f.return_type, &conc_f.return_type, mapping);
        }
        (Type::Tuple(generic_elements), Type::Tuple(conc)) => {
            for (generic, c) in generic_elements.iter().zip(conc.iter()) {
                extract_type_mapping(generic, c, mapping);
            }
        }
        (
            Type::Array {
                element: gen_element,
                ..
            },
            Type::Array {
                element: conc_element,
                ..
            },
        ) => {
            extract_type_mapping(gen_element, conc_element, mapping);
        }
        _ => {}
    }
}

pub(crate) fn generic_bounds(generic: &Generic) -> (EcoString, Vec<Type>) {
    let bounds = generic
        .resolved_bounds()
        .expect("generic bounds are resolved before emission")
        .cloned()
        .collect();
    (generic.name.clone(), bounds)
}
