use syntax::ast::{Attribute, Expression, Span};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type};

use super::{TaskState, wrap_with_impl_generics};
use crate::call_classification::is_ufcs_method_type;
use crate::store::Store;

impl TaskState<'_> {
    pub(super) fn register_displayable(&mut self, store: &mut Store, items: &[Expression]) {
        let module_id = self.cursor.module_id.clone();
        let is_d_lis = self.is_d_lis(store);
        let mut candidates = Vec::new();
        for item in items {
            collect_displayable_candidates(item, is_d_lis, &mut candidates);
        }
        for candidate in candidates {
            self.process_displayable_candidate(store, &module_id, candidate);
        }
    }

    pub(super) fn register_module_displayable(&mut self, store: &mut Store, module_id: &str) {
        let candidates = {
            let module = store.get_module(module_id).expect("module must exist");
            let mut candidates = Vec::new();
            for file in module.files.values() {
                let is_d_lis = file.is_d_lis();
                for item in &file.items {
                    collect_displayable_candidates(item, is_d_lis, &mut candidates);
                }
            }
            candidates
        };

        for candidate in candidates {
            self.process_displayable_candidate(store, module_id, candidate);
        }
    }

    fn process_displayable_candidate(
        &mut self,
        store: &mut Store,
        module_id: &str,
        candidate: DisplayableCandidate,
    ) {
        let DisplayableCandidate {
            attribute_span,
            kind,
        } = candidate;
        let TypeCandidate {
            name,
            is_struct,
            is_d_lis,
            has_args,
        } = match kind {
            CandidateKind::Misplaced => {
                self.sink
                    .push(diagnostics::attribute::displayable_not_a_struct_or_enum(
                        &attribute_span,
                    ));
                return;
            }
            CandidateKind::Type(type_candidate) => type_candidate,
        };

        if has_args {
            self.sink
                .push(diagnostics::attribute::displayable_with_arguments(
                    &attribute_span,
                ));
            return;
        }
        if is_d_lis {
            self.sink
                .push(diagnostics::attribute::displayable_in_typedef(
                    &attribute_span,
                ));
            return;
        }

        let qualified = Symbol::from_parts(module_id, &name);
        if is_struct
            && let Some(definition) = store.get_definition(qualified.as_str())
            && is_pointer_backed_newtype(definition)
        {
            self.sink
                .push(diagnostics::attribute::displayable_on_pointer_newtype(
                    &attribute_span,
                ));
            return;
        }

        self.synthesize_to_string(store, module_id, &attribute_span, &qualified);
    }

    fn synthesize_to_string(
        &mut self,
        store: &mut Store,
        module_id: &str,
        attribute_span: &Span,
        qualified: &Symbol,
    ) {
        let Some(scheme) = store.get_type(qualified.as_str()).cloned() else {
            return;
        };
        let Some(generics) = store
            .get_definition(qualified.as_str())
            .and_then(type_generics)
        else {
            return;
        };

        if let Some(user_ty) = user_to_string_type(store, qualified) {
            if is_ufcs_method_type(&user_ty, generics.len()) {
                self.sink
                    .push(diagnostics::attribute::displayable_specialized_to_string(
                        attribute_span,
                    ));
                return;
            }
            if user_ty.is_stringer_signature() {
                return;
            }
        }

        let receiver_ty = match scheme {
            Type::Forall { body, .. } => *body,
            other => other,
        };
        let fn_ty = Type::function(
            vec![receiver_ty],
            vec![false],
            Default::default(),
            Box::new(Type::string()),
        );
        let method_ty = wrap_with_impl_generics(&fn_ty, &generics, &[]);

        let module = store.get_module_mut(module_id).expect("module must exist");
        if let Some(methods) = module
            .definitions
            .get_mut(qualified.as_str())
            .and_then(Definition::methods_mut)
        {
            methods.insert("to_string".into(), method_ty);
        }
    }
}

fn user_to_string_type(store: &Store, qualified: &Symbol) -> Option<Type> {
    match &store.get_definition(qualified.as_str())?.body {
        DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. } => {
            methods.get("to_string").cloned()
        }
        _ => None,
    }
}

fn type_generics(definition: &Definition) -> Option<Vec<syntax::ast::Generic>> {
    match &definition.body {
        DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. } => {
            Some(generics.clone())
        }
        _ => None,
    }
}

struct DisplayableCandidate {
    attribute_span: Span,
    kind: CandidateKind,
}

enum CandidateKind {
    Type(TypeCandidate),
    Misplaced,
}

struct TypeCandidate {
    name: String,
    is_struct: bool,
    is_d_lis: bool,
    has_args: bool,
}

fn displayable_attribute(attributes: &[Attribute]) -> Option<&Attribute> {
    attributes.iter().find(|a| a.name == "displayable")
}

fn misplaced_candidate(attribute: &Attribute) -> DisplayableCandidate {
    DisplayableCandidate {
        attribute_span: attribute.span,
        kind: CandidateKind::Misplaced,
    }
}

fn collect_method_attributes(methods: &[Expression], out: &mut Vec<DisplayableCandidate>) {
    for method in methods {
        if let Expression::Function { attributes, .. } = method {
            out.extend(displayable_attribute(attributes).map(misplaced_candidate));
        }
    }
}

fn collect_displayable_candidates(
    item: &Expression,
    is_d_lis: bool,
    out: &mut Vec<DisplayableCandidate>,
) {
    match item {
        Expression::Struct {
            attributes,
            name,
            fields,
            ..
        } => {
            if let Some(attribute) = displayable_attribute(attributes) {
                out.push(DisplayableCandidate {
                    attribute_span: attribute.span,
                    kind: CandidateKind::Type(TypeCandidate {
                        name: name.to_string(),
                        is_struct: true,
                        is_d_lis,
                        has_args: !attribute.args.is_empty(),
                    }),
                });
            }
            for field in fields {
                out.extend(displayable_attribute(&field.attributes).map(misplaced_candidate));
            }
        }
        Expression::Enum {
            attributes, name, ..
        } => {
            if let Some(attribute) = displayable_attribute(attributes) {
                out.push(DisplayableCandidate {
                    attribute_span: attribute.span,
                    kind: CandidateKind::Type(TypeCandidate {
                        name: name.to_string(),
                        is_struct: false,
                        is_d_lis,
                        has_args: !attribute.args.is_empty(),
                    }),
                });
            }
        }
        Expression::Function { attributes, .. } => {
            out.extend(displayable_attribute(attributes).map(misplaced_candidate));
        }
        Expression::ImplBlock { methods, .. } => collect_method_attributes(methods, out),
        Expression::Interface {
            method_signatures, ..
        } => collect_method_attributes(method_signatures, out),
        _ => {}
    }
}

fn is_pointer_backed_newtype(definition: &Definition) -> bool {
    definition.is_newtype()
        && matches!(
            &definition.body,
            DefinitionBody::Struct { fields, .. } if fields[0].ty.is_ref()
        )
}
