use rustc_hash::FxHashMap;

use syntax::EcoString;
use syntax::ast::Generic;
use syntax::types::Type;

pub(crate) fn extract_type_mapping(
    generic: &Type,
    concrete: &Type,
    mapping: &mut FxHashMap<String, Type>,
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
