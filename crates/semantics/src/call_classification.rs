use syntax::program::{DefinitionBody, Module};
use syntax::types::{Type, type_args_match_params};

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
/// Two conditions (either suffices):
/// 1. Extra type params: method's Forall vars exceed base type's generics count
/// 2. Partial receiver: receiver is not the impl's own type parameters in order
pub fn compute_module_ufcs(module: &Module) -> Vec<(String, String)> {
    let mut ufcs = Vec::new();

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

    ufcs
}
