use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::LisetteDiagnostic;
use syntax::ast::{Annotation, Expression, Span};
use syntax::program::File;
use syntax::program::{Package, Visibility};
use syntax::types::{Type, unqualified_name};

pub fn check_visibility_constraints(
    package: &Package,
    files: &HashMap<u32, File>,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let public_definitions: Vec<_> = package
        .definitions
        .iter()
        .filter(|(_, definition)| definition.visibility == Visibility::Public)
        .collect();
    let function_annotations = index_function_annotations(
        files,
        public_definitions
            .iter()
            .map(|(qualified_name, _)| item_name(qualified_name)),
    );
    for (qualified_name, definition) in public_definitions {
        let item_name = item_name(qualified_name);
        let annotation = function_annotations.get(item_name).copied();

        let mut ctx = LeakCtx {
            package,
            public_definition: item_name,
            fallback_span: definition.name_span,
            diagnostics,
        };
        ctx.check(&definition.ty, annotation);
    }
}

fn item_name(qualified_name: &str) -> &str {
    qualified_name
        .split('.')
        .next_back()
        .unwrap_or(qualified_name)
}

fn index_function_annotations<'files, 'names>(
    files: &'files HashMap<u32, File>,
    names: impl IntoIterator<Item = &'names str>,
) -> HashMap<&'files str, &'files Annotation> {
    let mut annotations = HashMap::default();
    let mut pending: HashSet<&str> = names.into_iter().collect();
    if pending.is_empty() {
        return annotations;
    }
    for file in files.values() {
        for item in &file.items {
            if let Expression::Function {
                name: fn_name,
                return_annotation,
                ..
            } = item
                && pending.remove(fn_name.as_str())
            {
                annotations.insert(fn_name.as_str(), return_annotation);
                if pending.is_empty() {
                    return annotations;
                }
            }
        }
    }
    annotations
}

struct LeakCtx<'a> {
    package: &'a Package,
    public_definition: &'a str,
    /// Used for positions without a user-provided annotation (function parameters,
    /// tuple elements). Without it, those diagnostics are spanless and the cache
    /// cannot attribute them to a package.
    fallback_span: Option<Span>,
    diagnostics: &'a mut Vec<LisetteDiagnostic>,
}

impl LeakCtx<'_> {
    fn check(&mut self, ty: &Type, annotation: Option<&Annotation>) {
        match ty {
            Type::Nominal { id, params, .. } => {
                if let Some(definition) = self.package.definitions.get(id.as_str())
                    && definition.visibility == Visibility::Private
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
                    self.check(&param.ty, None);
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
            Type::Array { element, .. } => {
                // The element is the first arg of `Array<T, N>`.
                let elem_ann = annotation.and_then(|a| match a {
                    Annotation::Constructor { params, .. } => params.first(),
                    _ => None,
                });
                self.check(element, elem_ann);
            }
            Type::Simple(_)
            | Type::Var { .. }
            | Type::Uninferred
            | Type::Ignored
            | Type::Parameter(_)
            | Type::Never
            | Type::Error
            | Type::ImportNamespace(_)
            | Type::ReceiverPlaceholder => {}
        }
    }
}
