use rustc_hash::FxHashMap as HashMap;

use diagnostics::LisetteDiagnostic;
use syntax::ast::{Annotation, Expression, Span};
use syntax::program::File;
use syntax::program::{Module, Visibility};
use syntax::types::{Type, unqualified_name};

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

        let mut ctx = LeakCtx {
            module,
            public_definition: item_name,
            fallback_span: definition.name_span(),
            diagnostics,
        };
        ctx.check(definition.ty(), annotation.as_ref());
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

struct LeakCtx<'a> {
    module: &'a Module,
    public_definition: &'a str,
    /// Used for positions without a user-provided annotation (function parameters,
    /// tuple elements). Without it, those diagnostics are spanless and the cache
    /// cannot attribute them to a module.
    fallback_span: Option<Span>,
    diagnostics: &'a mut Vec<LisetteDiagnostic>,
}

impl LeakCtx<'_> {
    fn check(&mut self, ty: &Type, annotation: Option<&Annotation>) {
        match ty {
            Type::Nominal { id, params, .. } => {
                if let Some(definition) = self.module.definitions.get(id.as_str())
                    && definition.visibility() == &Visibility::Private
                {
                    let span = annotation.map(|ann| ann.get_span()).or(self.fallback_span);
                    let type_name = unqualified_name(id);
                    self.diagnostics
                        .push(diagnostics::lint::private_type_in_public_api(
                            span.as_ref(),
                            type_name,
                            self.public_definition,
                        ));
                }
                for (i, param) in params.iter().enumerate() {
                    let param_ann = annotation.and_then(|a| match a {
                        Annotation::Constructor { params, .. } => params.get(i),
                        _ => None,
                    });
                    self.check(param, param_ann);
                }
            }
            Type::Function(f) => {
                let return_ann = match annotation {
                    Some(Annotation::Function { return_type, .. }) => Some(return_type.as_ref()),
                    Some(ann @ (Annotation::Constructor { .. } | Annotation::Tuple { .. })) => {
                        Some(ann)
                    }
                    _ => None,
                };
                for param in &f.params {
                    self.check(param, None);
                }
                self.check(&f.return_type, return_ann);
            }
            Type::Forall { body, .. } => {
                self.check(body, annotation);
            }
            Type::Tuple(elements) => {
                let element_annotations = annotation.and_then(|a| match a {
                    Annotation::Tuple { elements, .. } => Some(elements),
                    _ => None,
                });
                for (i, element) in elements.iter().enumerate() {
                    let element_annotation =
                        element_annotations.and_then(|annotations| annotations.get(i));
                    self.check(element, element_annotation);
                }
            }
            Type::Compound { args, .. } => {
                for a in args {
                    self.check(a, None);
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
}
