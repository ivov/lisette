use syntax::EcoString;
use syntax::ast::{Attribute, Expression, Span, VariantFields};

use super::TaskState;
use crate::store::Store;

#[derive(Debug, Clone)]
pub(crate) enum DerivedAttributeTarget {
    Struct {
        name: EcoString,
    },
    Enum {
        name: EcoString,
        name_span: Span,
        is_generic: bool,
        payload_variant_span: Option<Span>,
    },
    Misplaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DerivedAttributeKind {
    Display,
    Equality,
    Iterate,
}

/// One source occurrence of an attribute that synthesizes a type capability.
///
/// Placement is classified once for all three attributes. This keeps additions to
/// the AST from being accepted or rejected differently by parallel scanners.
#[derive(Debug, Clone)]
pub(crate) struct DerivedAttribute {
    pub(crate) span: Span,
    pub(crate) has_args: bool,
    pub(crate) target: DerivedAttributeTarget,
    kind: DerivedAttributeKind,
}

pub(crate) struct DerivedAttributeContext {
    pub(crate) package_id: String,
    pub(crate) is_d_lis: bool,
}

pub(crate) struct DerivedAttributes {
    pub(crate) context: DerivedAttributeContext,
    pub(crate) candidates: Vec<DerivedAttribute>,
}

impl TaskState {
    pub(super) fn register_item_derived_attributes(
        &mut self,
        store: &mut Store,
        items: &[Expression],
    ) {
        let candidates = collect_derived_attributes(
            DerivedAttributeContext {
                package_id: self.cursor.package_id.clone(),
                is_d_lis: self.is_d_lis(store),
            },
            items,
        );
        self.register_derived_attributes(store, candidates);
    }

    pub(super) fn register_package_derived_attributes(
        &mut self,
        store: &mut Store,
        package_id: &str,
    ) {
        let candidates = {
            let package = store.get_package(package_id).expect("package must exist");
            package
                .files
                .values()
                .map(|file| {
                    collect_derived_attributes(
                        DerivedAttributeContext {
                            package_id: package_id.to_string(),
                            is_d_lis: file.is_d_lis(),
                        },
                        &file.items,
                    )
                })
                .collect::<Vec<_>>()
        };
        for candidates in candidates {
            self.register_derived_attributes(store, candidates);
        }
    }

    fn register_derived_attributes(&mut self, store: &mut Store, candidates: DerivedAttributes) {
        let DerivedAttributes {
            context,
            candidates,
        } = candidates;
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.kind == DerivedAttributeKind::Iterate)
        {
            self.process_iterate_candidate(store, &context, candidate);
        }
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.kind == DerivedAttributeKind::Display)
        {
            self.process_display_candidate(store, &context, candidate);
        }
        let equality: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| candidate.kind == DerivedAttributeKind::Equality)
            .collect();
        if !equality.is_empty() {
            self.pending_equality_attributes.push(DerivedAttributes {
                context,
                candidates: equality,
            });
        }
    }
}

fn collect_derived_attributes(
    context: DerivedAttributeContext,
    items: &[Expression],
) -> DerivedAttributes {
    let mut out = DerivedAttributes {
        context,
        candidates: Vec::new(),
    };
    for item in items {
        match item {
            Expression::Struct {
                attributes,
                name,
                fields,
                ..
            } => {
                collect_attributes(
                    attributes,
                    DerivedAttributeTarget::Struct { name: name.clone() },
                    &mut out,
                );
                for field in fields {
                    collect_attributes(
                        field.attributes(),
                        DerivedAttributeTarget::Misplaced,
                        &mut out,
                    );
                }
            }
            Expression::Enum {
                attributes,
                name,
                name_span,
                generics,
                variants,
                ..
            } => collect_attributes(
                attributes,
                DerivedAttributeTarget::Enum {
                    name: name.clone(),
                    name_span: *name_span,
                    is_generic: !generics.is_empty(),
                    payload_variant_span: variants
                        .iter()
                        .find(|variant| !matches!(variant.fields, VariantFields::Unit))
                        .map(|variant| variant.name_span),
                },
                &mut out,
            ),
            Expression::Function { attributes, .. } | Expression::TypeAlias { attributes, .. } => {
                collect_attributes(attributes, DerivedAttributeTarget::Misplaced, &mut out)
            }
            Expression::ImplBlock { methods, .. }
            | Expression::Interface {
                method_signatures: methods,
                ..
            } => {
                for method in methods {
                    if let Expression::Function { attributes, .. } = method {
                        collect_attributes(attributes, DerivedAttributeTarget::Misplaced, &mut out);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn collect_attributes(
    attributes: &[Attribute],
    target: DerivedAttributeTarget,
    out: &mut DerivedAttributes,
) {
    let mut collect = |name: &str, kind, target| {
        if let Some(attribute) = attributes.iter().find(|attribute| attribute.name == name) {
            out.candidates.push(DerivedAttribute {
                span: attribute.span,
                has_args: !attribute.args.is_empty(),
                target,
                kind,
            });
        }
    };
    collect("display", DerivedAttributeKind::Display, target.clone());
    collect("equality", DerivedAttributeKind::Equality, target.clone());
    collect("iterate", DerivedAttributeKind::Iterate, target);
}
