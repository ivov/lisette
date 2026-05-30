use rustc_hash::FxHashMap as HashMap;

use crate::EmitEffects;
use crate::Planner;
use crate::names::constraints::{ConstraintAtom, ParamConstraintSet, classify_builtin_name};
use syntax::EcoString;
use syntax::ast::{Annotation, Generic};
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

pub(crate) fn receiver_generics_string(generics: &[Generic]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        let params: Vec<&str> = generics.iter().map(|g| g.name.as_str()).collect();
        format!("[{}]", params.join(", "))
    }
}

impl Planner<'_> {
    pub(crate) fn generics_to_string_for_symbol(
        &mut self,
        symbol: &str,
        generics: &[Generic],
        fx: &mut EmitEffects,
    ) -> String {
        if generics.is_empty() {
            return String::new();
        }
        let constraints = self
            .module
            .generic_constraints_for(symbol)
            .map(<[ParamConstraintSet]>::to_vec);

        let rendered = generics
            .iter()
            .map(|g| {
                let set = constraints
                    .as_ref()
                    .and_then(|sets| sets.iter().find(|s| s.name == g.name));
                let constraint = self.render_constraint(g, set, fx);
                format!("{} {}", g.name, constraint)
            })
            .collect::<Vec<_>>()
            .join(", ");

        format!("[{}]", rendered)
    }

    fn render_constraint(
        &mut self,
        generic: &Generic,
        constraint_set: Option<&ParamConstraintSet>,
        fx: &mut EmitEffects,
    ) -> String {
        let (explicit_atoms, inferred_comparable) = match constraint_set {
            Some(set) => (set.explicit.clone(), set.inferred_comparable),
            None => {
                // Fallback for symbols that bypassed `collect_generic_constraints`.
                let atoms: Vec<ConstraintAtom> = generic
                    .bounds
                    .iter()
                    .map(|ann| {
                        if let Annotation::Constructor { name, .. } = ann
                            && let Some(b) = classify_builtin_name(name)
                        {
                            b
                        } else {
                            ConstraintAtom::Named(ann.clone())
                        }
                    })
                    .collect();
                (atoms, false)
            }
        };

        // `Ordered` already implies `Comparable`; never double up.
        let needs_comparable_appendage = inferred_comparable
            && !explicit_atoms
                .iter()
                .any(ConstraintAtom::implies_comparable);

        let mut named_bounds: Vec<String> = Vec::new();
        let mut comparable_seen = false;
        for atom in &explicit_atoms {
            match atom {
                ConstraintAtom::Comparable => {
                    comparable_seen = true;
                }
                ConstraintAtom::Ordered => {
                    fx.require_go_import("cmp");
                    named_bounds.push("cmp.Ordered".to_string());
                }
                ConstraintAtom::Named(ann) => {
                    named_bounds.push(self.annotation_to_go_type(ann, fx));
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
        _ => {}
    }
}
