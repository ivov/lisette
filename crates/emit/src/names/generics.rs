use std::borrow::Cow;

use rustc_hash::FxHashMap as HashMap;

use crate::Planner;
use crate::names::go_name;
use crate::state::scope::TypeParam;
use syntax::EcoString;
use syntax::ast::Generic;
use syntax::types::{SubstitutionMap, Type};

fn build_type_map(generics: &[Generic], type_args: &[Type]) -> SubstitutionMap {
    generics
        .iter()
        .map(|g| g.name.clone())
        .zip(type_args.iter().cloned())
        .collect()
}

pub(crate) use syntax::types::substitute;

/// Substitute a field's type using generics and their concrete type arguments.
pub(crate) fn resolve_field_type(
    generics: &[Generic],
    type_args: &[Type],
    field_ty: &Type,
) -> Type {
    let type_map = build_type_map(generics, type_args);
    substitute(field_ty, &type_map)
}

impl Planner<'_> {
    pub(crate) fn format_value_type_args(
        &mut self,
        definition: Option<&str>,
        instantiation: Option<&SubstitutionMap>,
    ) -> Option<String> {
        let declaration = self.callable_declaration(definition?)?;
        let instantiation = instantiation?;
        match declaration.go_type_param_recipe() {
            Some(recipe) => self.reconstruct_collapsed_type_args(recipe, instantiation),
            None => self.format_generic_instantiation(declaration.ty(), instantiation),
        }
    }

    pub(crate) fn format_generic_instantiation(
        &mut self,
        definition_ty: &Type,
        mapping: &SubstitutionMap,
    ) -> Option<String> {
        let Type::Forall { vars, .. } = definition_ty else {
            return None;
        };
        if vars.is_empty() {
            return None;
        }

        let args: Vec<String> = vars
            .iter()
            .filter_map(|var| {
                let concrete = mapping.get(var.as_str())?;
                Some(self.use_go_type(concrete))
            })
            .collect();

        if args.len() != vars.len() || args.iter().any(|a| a.contains("interface{}")) {
            return None;
        }

        Some(format!("[{}]", args.join(", ")))
    }

    /// Make `generics` the type parameters of the declaration being emitted.
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

    pub(crate) fn generic_go_name<'a>(&'a self, source_name: &'a str) -> Cow<'a, str> {
        match self.package.generic_rename(source_name) {
            Some(renamed) => Cow::Borrowed(renamed),
            None => go_name::escape_type_name(source_name),
        }
    }

    pub(crate) fn receiver_generics_string(&self, generics: &[Generic]) -> String {
        if generics.is_empty() {
            String::new()
        } else {
            let params: Vec<Cow<'_, str>> = generics
                .iter()
                .map(|g| self.generic_go_name(&g.name))
                .collect();
            format!("[{}]", params.join(", "))
        }
    }

    pub(crate) fn generics_to_string(&mut self, generics: &[Generic]) -> String {
        let resolved_generics = generics
            .iter()
            .map(|generic| {
                let bounds = generic
                    .resolved_bounds()
                    .expect("generic bounds must be resolved before emission")
                    .cloned()
                    .collect();
                (generic.name.clone(), bounds)
            })
            .collect::<Vec<_>>();
        self.resolved_generics_to_string(&resolved_generics)
    }

    pub(crate) fn resolved_generics_to_string(
        &mut self,
        generics: &[(EcoString, Vec<Type>)],
    ) -> String {
        if generics.is_empty() {
            return String::new();
        }

        let rendered = generics
            .iter()
            .map(|(name, bounds)| {
                let constraint = self.render_bounds(bounds);
                format!("{} {}", self.generic_go_name(name), constraint)
            })
            .collect::<Vec<_>>()
            .join(", ");

        format!("[{rendered}]")
    }

    fn render_bounds(&mut self, bounds: &[Type]) -> String {
        let has_ordered = bounds.iter().any(is_ordered_bound);
        let mut named_bounds: Vec<String> = Vec::new();
        for bound in bounds {
            if is_zeroable_bound(bound) {
                // Lisette-only constraint: every Go type is zeroable.
                continue;
            }
            let rendered = if is_comparable_bound(bound) {
                if has_ordered {
                    continue;
                }
                "comparable".to_string()
            } else if is_ordered_bound(bound) {
                self.require_cmp();
                "cmp.Ordered".to_string()
            } else {
                self.use_go_type(bound)
            };
            if !named_bounds.contains(&rendered) {
                named_bounds.push(rendered);
            }
        }

        match named_bounds.as_slice() {
            [] => "any".to_string(),
            [single] => single.clone(),
            multiple => format!("interface {{ {} }}", multiple.join("; ")),
        }
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

    // Walk type arguments for any type that carries them (Constructor,
    // Compound, or mixed-variant pairs).
    if let (Some(gen_params), Some(conc_params)) =
        (generic.get_type_params(), concrete.get_type_params())
    {
        for (g, c) in gen_params.iter().zip(conc_params.iter()) {
            extract_type_mapping(g, c, mapping);
        }
        return;
    }

    match (generic, concrete) {
        (Type::Function(gen_f), Type::Function(conc_f)) => {
            for (g, c) in gen_f.params.iter().zip(conc_f.params.iter()) {
                extract_type_mapping(&g.ty, &c.ty, mapping);
            }
            extract_type_mapping(&gen_f.return_type, &conc_f.return_type, mapping);
        }
        (Type::Tuple(generic_elements), Type::Tuple(conc)) => {
            for (g, c) in generic_elements.iter().zip(conc.iter()) {
                extract_type_mapping(g, c, mapping);
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
