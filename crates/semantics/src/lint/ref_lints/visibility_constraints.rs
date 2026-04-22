use rustc_hash::FxHashMap as HashMap;

use diagnostics::LisetteDiagnostic;
use syntax::ast::{Annotation, Expression};
use syntax::program::File;
use syntax::program::{Module, Visibility};
use syntax::types::Type;

pub fn check_visibility_constraints(
    module: &Module,
    files: &HashMap<u32, File>,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    for (qualified_name, definition) in &module.definitions {
        if definition.visibility() != &Visibility::Public {
            continue;
        }

        let item_name = qualified_name
            .split('.')
            .next_back()
            .unwrap_or(qualified_name);

        let annotation = find_function_annotation(files, item_name)
            .or_else(|| find_function_annotation(&module.typedefs, item_name));

        check_type_for_private_leak(
            module,
            definition.ty(),
            annotation.as_ref(),
            item_name,
            diagnostics,
        );
    }
}

fn find_function_annotation(files: &HashMap<u32, File>, name: &str) -> Option<Annotation> {
    for file in files.values() {
        for item in &file.items {
            if let Expression::Function {
                name: fn_name,
                return_annotation,
                ..
            } = item
                && fn_name == name
            {
                return Some(return_annotation.clone());
            }
        }
    }
    None
}

fn check_type_for_private_leak(
    module: &Module,
    ty: &Type,
    annotation: Option<&Annotation>,
    public_definition: &str,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    match ty {
        Type::Nominal { id, params, .. } => {
            if let Some(definition) = module.definitions.get(id.as_str())
                && definition.visibility() == &Visibility::Private
            {
                let span = annotation.map(|ann| ann.get_span());
                let type_name = id.rsplit('.').next().unwrap_or(id);
                diagnostics.push(diagnostics::lint::private_type_in_public_api(
                    span.as_ref(),
                    type_name,
                    public_definition,
                ));
            }
            for (i, param) in params.iter().enumerate() {
                let param_ann = annotation.and_then(|a| match a {
                    Annotation::Constructor { params, .. } => params.get(i),
                    _ => None,
                });
                check_type_for_private_leak(
                    module,
                    param,
                    param_ann,
                    public_definition,
                    diagnostics,
                );
            }
        }
        Type::Function {
            params,
            return_type,
            ..
        } => match annotation {
            Some(Annotation::Function {
                return_type: ret_ann,
                ..
            }) => {
                for param in params {
                    check_type_for_private_leak(
                        module,
                        param,
                        None,
                        public_definition,
                        diagnostics,
                    );
                }
                check_type_for_private_leak(
                    module,
                    return_type,
                    Some(ret_ann.as_ref()),
                    public_definition,
                    diagnostics,
                );
            }
            Some(ann @ (Annotation::Constructor { .. } | Annotation::Tuple { .. })) => {
                for param in params {
                    check_type_for_private_leak(
                        module,
                        param,
                        None,
                        public_definition,
                        diagnostics,
                    );
                }
                check_type_for_private_leak(
                    module,
                    return_type,
                    Some(ann),
                    public_definition,
                    diagnostics,
                );
            }
            _ => {
                for param in params {
                    check_type_for_private_leak(
                        module,
                        param,
                        None,
                        public_definition,
                        diagnostics,
                    );
                }
                check_type_for_private_leak(
                    module,
                    return_type,
                    None,
                    public_definition,
                    diagnostics,
                );
            }
        },
        Type::Forall { body, .. } => {
            check_type_for_private_leak(module, body, annotation, public_definition, diagnostics);
        }
        Type::Tuple(elements) => {
            let element_annotations = annotation.and_then(|a| match a {
                Annotation::Tuple { elements, .. } => Some(elements),
                _ => None,
            });
            for (i, element) in elements.iter().enumerate() {
                let element_annotation =
                    element_annotations.and_then(|annotations| annotations.get(i));
                check_type_for_private_leak(
                    module,
                    element,
                    element_annotation,
                    public_definition,
                    diagnostics,
                );
            }
        }
        Type::Compound { args, .. } => {
            for a in args {
                check_type_for_private_leak(module, a, None, public_definition, diagnostics);
            }
        }
        Type::Simple(_)
        | Type::Var { .. }
        | Type::Parameter(_)
        | Type::Never
        | Type::Error
        | Type::ImportNamespace(_)
        | Type::ReceiverPlaceholder => {}
    }
}
