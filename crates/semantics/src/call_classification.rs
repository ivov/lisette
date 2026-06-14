use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::Expression;
use syntax::program::{DefinitionBody, Module};
use syntax::types::{Symbol, Type, type_args_match_params};

pub fn is_ufcs_method_type(method_ty: &Type, base_generics_count: usize) -> bool {
    let Type::Forall { vars, body } = method_ty else {
        return base_generics_count > 0;
    };

    if vars.len() > base_generics_count {
        return true;
    }

    if let Type::Function(f) = body.as_ref()
        && let Some(receiver_param) = f.params.first()
        && let Type::Nominal {
            params: receiver_params,
            ..
        } = receiver_param.strip_refs()
        && !type_args_match_params(&receiver_params, vars.iter())
    {
        return true;
    }

    false
}

/// Compute UFCS methods for a single module's types.
///
/// Three conditions (any one suffices):
/// 1. Extra type params: method's Forall vars exceed base type's generics count
/// 2. Partial receiver: receiver is not the impl's own type parameters in order
/// 3. Mixed impl blocks: type has both bounded and unbounded impl blocks
pub fn compute_module_ufcs(module: &Module, module_id: &str) -> Vec<(String, String)> {
    let mut ufcs = Vec::new();

    // Conditions 1+2: check each method's type signature
    for (key, definition) in &module.definitions {
        let (methods, base_generics_count) = match &definition.body {
            DefinitionBody::Struct {
                methods, generics, ..
            } => (methods, generics.len()),
            DefinitionBody::Enum {
                methods, generics, ..
            } => (methods, generics.len()),
            DefinitionBody::TypeAlias {
                methods, generics, ..
            } => (methods, generics.len()),
            _ => continue,
        };

        for (method_name, method_ty) in methods {
            if is_ufcs_method_type(method_ty, base_generics_count) {
                ufcs.push((key.to_string(), method_name.to_string()));
            }
        }
    }

    // Condition 3: mixed constrained/unconstrained impl blocks
    let mut constrained_methods: HashMap<String, Vec<String>> = HashMap::default();
    let mut unconstrained_types: HashSet<String> = HashSet::default();

    for file in module.files.values() {
        for item in &file.items {
            if let Expression::ImplBlock {
                receiver_name,
                generics,
                methods,
                ..
            } = item
            {
                let qualified_type = Symbol::from_parts(module_id, receiver_name).to_string();
                if generics.iter().any(|g| !g.bounds.is_empty()) {
                    let method_names: Vec<String> = methods
                        .iter()
                        .filter_map(|m| {
                            if let Expression::Function { name, .. } = m {
                                Some(name.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();
                    constrained_methods
                        .entry(qualified_type)
                        .or_default()
                        .extend(method_names);
                } else {
                    unconstrained_types.insert(qualified_type);
                }
            }
        }
    }

    for (type_name, methods) in constrained_methods {
        if unconstrained_types.contains(&type_name) {
            for method_name in methods {
                ufcs.push((type_name.clone(), method_name));
            }
        }
    }

    ufcs
}
