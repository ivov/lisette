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

    if let (Some(generic_params), Some(concrete_params)) =
        (generic.get_type_params(), concrete.get_type_params())
    {
        for (generic, c) in generic_params.iter().zip(concrete_params.iter()) {
            extract_type_mapping(generic, c, mapping);
        }
        return;
    }

    match (generic, concrete) {
        (Type::Function(generic_function), Type::Function(concrete_function)) => {
            for (generic, c) in generic_function
                .params
                .iter()
                .zip(concrete_function.params.iter())
            {
                extract_type_mapping(&generic.ty, &c.ty, mapping);
            }
            extract_type_mapping(
                &generic_function.return_type,
                &concrete_function.return_type,
                mapping,
            );
        }
        (Type::Tuple(generic_elements), Type::Tuple(concrete_elements)) => {
            for (generic, c) in generic_elements.iter().zip(concrete_elements.iter()) {
                extract_type_mapping(generic, c, mapping);
            }
        }
        (
            Type::Array {
                element: generic_element,
                ..
            },
            Type::Array {
                element: concrete_element,
                ..
            },
        ) => {
            extract_type_mapping(generic_element, concrete_element, mapping);
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
