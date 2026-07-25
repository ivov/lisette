use syntax::EcoString;
use syntax::ast::{Attribute, Expression, Span, VariantFields};

use super::TaskState;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerivedAttributeKind {
    Display,
    Equality,
    Iterate,
}

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

/// One source occurrence of an attribute that synthesizes a type capability.
///
/// Placement is classified once for all three attributes. This keeps additions to
/// the AST from being accepted or rejected differently by parallel scanners.
#[derive(Debug, Clone)]
pub(crate) struct DerivedAttribute {
    pub(crate) module_id: String,
    pub(crate) kind: DerivedAttributeKind,
    pub(crate) span: Span,
    pub(crate) has_args: bool,
    pub(crate) is_d_lis: bool,
    pub(crate) target: DerivedAttributeTarget,
}

impl TaskState {
    pub(super) fn register_item_derived_attributes(
        &mut self,
        store: &mut Store,
        items: &[Expression],
    ) {
        let candidates =
            collect_derived_attributes(&self.cursor.module_id, self.is_d_lis(store), items);
        self.register_derived_attributes(store, candidates);
    }

    pub(super) fn register_module_derived_attributes(
        &mut self,
        store: &mut Store,
        module_id: &str,
    ) {
        let candidates = {
            let module = store.get_module(module_id).expect("module must exist");
            module
                .files
                .values()
                .flat_map(|file| {
                    collect_derived_attributes(module_id, file.is_d_lis(), &file.items)
                })
                .collect()
        };
        self.register_derived_attributes(store, candidates);
    }

    fn register_derived_attributes(
        &mut self,
        store: &mut Store,
        candidates: Vec<DerivedAttribute>,
    ) {
        self.register_iterate(store, &candidates);
        self.register_display(store, &candidates);
        self.pending_equality_attributes.extend(
            candidates
                .into_iter()
                .filter(|candidate| candidate.kind == DerivedAttributeKind::Equality),
        );
    }
}

fn collect_derived_attributes(
    module_id: &str,
    is_d_lis: bool,
    items: &[Expression],
) -> Vec<DerivedAttribute> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Expression::Struct {
                attributes,
                name,
                fields,
                ..
            } => {
                collect_attributes(
                    module_id,
                    is_d_lis,
                    attributes,
                    DerivedAttributeTarget::Struct { name: name.clone() },
                    &mut out,
                );
                for field in fields {
                    collect_attributes(
                        module_id,
                        is_d_lis,
                        &field.attributes,
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
                module_id,
                is_d_lis,
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
                collect_attributes(
                    module_id,
                    is_d_lis,
                    attributes,
                    DerivedAttributeTarget::Misplaced,
                    &mut out,
                )
            }
            Expression::ImplBlock { methods, .. }
            | Expression::Interface {
                method_signatures: methods,
                ..
            } => {
                for method in methods {
                    if let Expression::Function { attributes, .. } = method {
                        collect_attributes(
                            module_id,
                            is_d_lis,
                            attributes,
                            DerivedAttributeTarget::Misplaced,
                            &mut out,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn collect_attributes(
    module_id: &str,
    is_d_lis: bool,
    attributes: &[Attribute],
    target: DerivedAttributeTarget,
    out: &mut Vec<DerivedAttribute>,
) {
    const DERIVED: [(&str, DerivedAttributeKind); 3] = [
        ("display", DerivedAttributeKind::Display),
        ("equality", DerivedAttributeKind::Equality),
        ("iterate", DerivedAttributeKind::Iterate),
    ];

    for (name, kind) in DERIVED {
        let Some(attribute) = attributes.iter().find(|attribute| attribute.name == name) else {
            continue;
        };
        out.push(DerivedAttribute {
            module_id: module_id.to_string(),
            kind,
            span: attribute.span,
            has_args: !attribute.args.is_empty(),
            is_d_lis,
            target: target.clone(),
        });
    }
}
