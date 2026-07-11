use std::borrow::Cow;

use rustc_hash::FxHashMap as HashMap;

use crate::Planner;
use crate::names::go_name;
use syntax::EcoString;
use syntax::ast::{Annotation, Generic};
use syntax::program::{GenericConstraint, GenericConstraints};
use syntax::types::Type;

fn build_type_map(generics: &[Generic], type_args: &[Type]) -> HashMap<EcoString, Type> {
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
    pub(crate) fn generic_go_name<'a>(&'a self, source_name: &'a str) -> Cow<'a, str> {
        match self.module.generic_rename(source_name) {
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

    pub(crate) fn generics_to_string_for_symbol(
        &mut self,
        symbol: &str,
        generics: &[Generic],
    ) -> String {
        if generics.is_empty() {
            return String::new();
        }
        let constraints = self
            .facts
            .generic_constraints
            .get(symbol)
            .expect("passes records constraints for every emitted generic definition");

        let rendered = generics
            .iter()
            .map(|g| {
                let constraints = constraints
                    .iter()
                    .find(|constraints| constraints.parameter == g.name)
                    .expect("generic constraints preserve every generic parameter");
                let constraint = self.render_constraint(constraints);
                format!("{} {}", self.generic_go_name(&g.name), constraint)
            })
            .collect::<Vec<_>>()
            .join(", ");

        format!("[{}]", rendered)
    }

    fn render_constraint(&mut self, constraints: &GenericConstraints) -> String {
        // `Ordered` already implies `Comparable`; never double up.
        let needs_comparable_appendage = constraints.inferred_comparable
            && !constraints.explicit.iter().any(|constraint| {
                matches!(
                    constraint,
                    GenericConstraint::Comparable | GenericConstraint::Ordered
                )
            });

        let mut named_bounds: Vec<String> = Vec::new();
        let mut comparable_seen = false;
        for constraint in &constraints.explicit {
            match constraint {
                GenericConstraint::Comparable => {
                    comparable_seen = true;
                }
                GenericConstraint::Ordered => {
                    self.require_cmp();
                    named_bounds.push("cmp.Ordered".to_string());
                }
                GenericConstraint::Named(annotation) => {
                    named_bounds.push(self.named_bound_go_type(annotation));
                }
            }
        }

        if needs_comparable_appendage || comparable_seen {
            named_bounds.push("comparable".to_string());
        }

        match named_bounds.as_slice() {
            [] => "any".to_string(),
            [single] => single.clone(),
            multiple => format!("interface {{ {} }}", multiple.join("; ")),
        }
    }

    fn named_bound_go_type(&mut self, annotation: &Annotation) -> String {
        let resolved = self
            .facts
            .resolved_bound_type(annotation.get_span())
            .cloned()
            .expect("checker records a resolved type for every generic bound");
        self.go_type_string(&resolved)
    }
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
                extract_type_mapping(g, c, mapping);
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
