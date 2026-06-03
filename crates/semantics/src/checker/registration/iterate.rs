use syntax::ast::{Attribute, Expression, Span, VariantFields};
use syntax::program::{Definition, DefinitionBody, Visibility};
use syntax::types::{CompoundKind, Symbol, Type};

use super::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    /// Register `#[iterate]` enums from a single items list (the `register_types_and_values` path).
    pub(super) fn register_iterate(&mut self, store: &mut Store, items: &[Expression]) {
        let module_id = self.cursor.module_id.clone();
        let is_d_lis = self.is_d_lis(store);
        let mut candidates = Vec::new();
        for item in items {
            collect_iterate_candidates(item, is_d_lis, &mut candidates);
        }
        for candidate in candidates {
            self.process_iterate_candidate(store, &module_id, candidate);
        }
    }

    /// Register `#[iterate]` enums across all of the module's files (the `register_module`
    /// path), after every file is registered so cross-file collisions are visible.
    pub(super) fn register_module_iterate(&mut self, store: &mut Store, module_id: &str) {
        let candidates = {
            let module = store.get_module(module_id).expect("module must exist");
            let mut candidates = Vec::new();
            for file in module.files.values() {
                let is_d_lis = file.is_d_lis();
                for item in &file.items {
                    collect_iterate_candidates(item, is_d_lis, &mut candidates);
                }
            }
            candidates
        };

        for candidate in candidates {
            self.process_iterate_candidate(store, module_id, candidate);
        }
    }

    fn process_iterate_candidate(
        &mut self,
        store: &mut Store,
        module_id: &str,
        candidate: IterateCandidate,
    ) {
        let IterateCandidate {
            attribute_span,
            kind,
        } = candidate;
        let EnumCandidate {
            name,
            name_span,
            is_generic,
            is_d_lis,
            payload_variant_span,
        } = match kind {
            CandidateKind::NotAnEnum => {
                self.sink
                    .push(diagnostics::attribute::iterate_not_an_enum(&attribute_span));
                return;
            }
            CandidateKind::Enum(enum_candidate) => enum_candidate,
        };

        if is_d_lis {
            self.sink
                .push(diagnostics::attribute::iterate_in_typedef(&attribute_span));
            return;
        }
        if is_generic {
            self.sink.push(diagnostics::attribute::iterate_generic_enum(
                &attribute_span,
            ));
            return;
        }
        if let Some(variant_span) = payload_variant_span {
            self.sink
                .push(diagnostics::attribute::iterate_non_unit_variant(
                    &attribute_span,
                    &variant_span,
                ));
            return;
        }

        let qualified = Symbol::from_parts(module_id, &name);
        let variants_key = qualified.with_segment("variants");

        // A static method or a variant literally named `variants` both register
        // a `Value` at `Enum.variants`; an instance method lands in the enum's
        // method map. Any of the three collides.
        let existing_span = store
            .get_definition(variants_key.as_str())
            .and_then(|definition| definition.name_span);
        let (has_instance_variants, visibility) = match store.get_definition(qualified.as_str()) {
            Some(definition) => (
                matches!(&definition.body, DefinitionBody::Enum { methods, .. } if methods.contains_key("variants")),
                definition.visibility().clone(),
            ),
            None => (false, Visibility::Private),
        };
        if existing_span.is_some() || has_instance_variants {
            self.sink
                .push(diagnostics::attribute::iterate_variants_conflict(
                    &attribute_span,
                    existing_span.as_ref(),
                ));
            return;
        }

        let Some(enum_ty) = store.get_type(qualified.as_str()).cloned() else {
            return;
        };

        let slice_ty = Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![enum_ty],
        };
        let fn_ty = Type::function(vec![], vec![], Default::default(), Box::new(slice_ty));

        let module = store.get_module_mut(module_id).expect("module must exist");
        module.definitions.insert(
            variants_key,
            Definition {
                visibility,
                ty: fn_ty,
                name: None,
                name_span: Some(name_span),
                doc: None,
                body: DefinitionBody::Value {
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                },
            },
        );
    }
}

struct IterateCandidate {
    attribute_span: Span,
    kind: CandidateKind,
}

enum CandidateKind {
    Enum(EnumCandidate),
    NotAnEnum,
}

struct EnumCandidate {
    name: String,
    name_span: Span,
    is_generic: bool,
    is_d_lis: bool,
    payload_variant_span: Option<Span>,
}

fn iterate_attribute_span(attributes: &[Attribute]) -> Option<Span> {
    attributes
        .iter()
        .find(|a| a.name == "iterate")
        .map(|a| a.span)
}

fn misplaced_candidate(span: Span) -> IterateCandidate {
    IterateCandidate {
        attribute_span: span,
        kind: CandidateKind::NotAnEnum,
    }
}

/// Record any `#[iterate]` on a method (impl or interface) as misplaced.
fn collect_method_attributes(methods: &[Expression], out: &mut Vec<IterateCandidate>) {
    for method in methods {
        if let Expression::Function { attributes, .. } = method {
            out.extend(iterate_attribute_span(attributes).map(misplaced_candidate));
        }
    }
}

/// Collect every `#[iterate]` occurrence on a top-level item: an enum to
/// validate and synthesize, anything else (including fields and methods)
/// recorded as misplaced so the attribute is never silently accepted off an enum.
fn collect_iterate_candidates(item: &Expression, is_d_lis: bool, out: &mut Vec<IterateCandidate>) {
    match item {
        Expression::Enum {
            attributes,
            name,
            name_span,
            generics,
            variants,
            ..
        } => {
            if let Some(attribute_span) = iterate_attribute_span(attributes) {
                let payload_variant_span = variants
                    .iter()
                    .find(|v| !matches!(v.fields, VariantFields::Unit))
                    .map(|v| v.name_span);
                out.push(IterateCandidate {
                    attribute_span,
                    kind: CandidateKind::Enum(EnumCandidate {
                        name: name.to_string(),
                        name_span: *name_span,
                        is_generic: !generics.is_empty(),
                        is_d_lis,
                        payload_variant_span,
                    }),
                });
            }
        }
        Expression::Struct {
            attributes, fields, ..
        } => {
            out.extend(iterate_attribute_span(attributes).map(misplaced_candidate));
            for field in fields {
                out.extend(iterate_attribute_span(&field.attributes).map(misplaced_candidate));
            }
        }
        Expression::Function { attributes, .. } => {
            out.extend(iterate_attribute_span(attributes).map(misplaced_candidate));
        }
        Expression::ImplBlock { methods, .. } => collect_method_attributes(methods, out),
        Expression::Interface {
            method_signatures, ..
        } => collect_method_attributes(method_signatures, out),
        _ => {}
    }
}
