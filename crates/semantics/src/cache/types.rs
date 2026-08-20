use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use syntax::ast::{
    Annotation, Attribute, EnumFieldDefinition, EnumVariant, Generic, Span, StructFieldDefinition,
    StructFieldKind, VariantFields,
};
use syntax::program::{AliasKind, Definition, DefinitionBody, Interface, Method, Package};
use syntax::types::Symbol;

/// A definition whose span file IDs are cache-local file indices.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct CachedDefinition(Definition);

impl CachedDefinition {
    pub fn as_definition(&self) -> &Definition {
        &self.0
    }

    pub(crate) fn from_definition(
        definition: &Definition,
        file_id_to_index: &HashMap<u32, u32>,
    ) -> Self {
        debug_assert!(
            definition
                .body
                .generics()
                .is_none_or(|generics| generics.iter().all(Generic::bounds_are_resolved)),
            "generic bounds must be resolved before caching",
        );
        let mut definition = definition.clone();
        remap_definition_spans(&mut definition, &mut |span| {
            if !span.is_dummy() {
                span.file_id = file_id_to_index.get(&span.file_id).copied().unwrap_or(0);
            }
        });
        Self(definition)
    }

    pub(crate) fn install_into(
        &self,
        package: &mut Package,
        qualified_name: Symbol,
        file_ids: &[u32],
    ) {
        package
            .definitions
            .insert(qualified_name, self.to_definition(file_ids));
    }

    pub(crate) fn to_definition(&self, file_ids: &[u32]) -> Definition {
        let mut definition = self.0.clone();
        remap_definition_spans(&mut definition, &mut |span| {
            if !span.is_dummy() {
                span.file_id = file_ids.get(span.file_id as usize).copied().unwrap_or(0);
            }
        });
        definition
    }
}

fn remap_definition_spans(definition: &mut Definition, remap: &mut impl FnMut(&mut Span)) {
    let Definition {
        visibility: _,
        ty: _,
        name_span,
        doc: _,
        body,
    } = definition;
    if let Some(span) = name_span {
        remap(span);
    }

    match body {
        DefinitionBody::TypeAlias {
            generics,
            alias,
            methods,
            attributes: _,
        } => {
            remap_generics(generics, remap);
            match alias {
                AliasKind::Opaque(annotation)
                | AliasKind::Transparent {
                    annotation,
                    target: _,
                } => remap_annotation(annotation, remap),
            }
            remap_methods(methods.values_mut(), remap);
        }
        DefinitionBody::Enum {
            generics,
            variants,
            methods,
            attributes: _,
        } => {
            remap_generics(generics, remap);
            for variant in variants {
                remap_variant(variant, remap);
            }
            remap_methods(methods.values_mut(), remap);
        }
        DefinitionBody::Struct {
            generics,
            fields,
            methods,
            attributes: _,
        } => {
            remap_generics(generics, remap);
            for field in fields {
                remap_struct_field(field, remap);
            }
            remap_methods(methods.values_mut(), remap);
        }
        DefinitionBody::Interface { definition } => remap_interface(definition, remap),
        DefinitionBody::Value {
            kind: _,
            allowed_lints: _,
            go_hints: _,
            go_name: _,
            go_type_param_recipe: _,
        } => {}
    }
}

fn remap_generics(generics: &mut [Generic], remap: &mut impl FnMut(&mut Span)) {
    for generic in generics {
        remap(&mut generic.span);
        generic.for_each_bound_annotation_mut(|annotation| remap_annotation(annotation, remap));
    }
}

fn remap_annotation(annotation: &mut Annotation, remap: &mut impl FnMut(&mut Span)) {
    match annotation {
        Annotation::Constructor {
            name: _,
            params,
            writable: _,
            span,
        } => {
            remap(span);
            for param in params {
                remap_annotation(param, remap);
            }
        }
        Annotation::Function {
            params,
            return_type,
            span,
        } => {
            remap(span);
            for param in params {
                remap_annotation(param, remap);
            }
            remap_annotation(return_type, remap);
        }
        Annotation::Tuple { elements, span } => {
            remap(span);
            for element in elements {
                remap_annotation(element, remap);
            }
        }
        Annotation::Opaque { span } | Annotation::Constant { span, .. } => remap(span),
        Annotation::Unknown => {}
    }
}

fn remap_methods<'a>(
    methods: impl Iterator<Item = &'a mut Method>,
    remap: &mut impl FnMut(&mut Span),
) {
    for method in methods {
        let Method {
            source_name: _,
            ty: _,
            visibility: _,
            origin: _,
            name_span,
            doc: _,
            allowed_lints: _,
            go_hints: _,
        } = method;
        if let Some(span) = name_span {
            remap(span);
        }
    }
}

fn remap_variant(variant: &mut EnumVariant, remap: &mut impl FnMut(&mut Span)) {
    let EnumVariant {
        doc: _,
        name: _,
        name_span,
        fields,
    } = variant;
    remap(name_span);
    match fields {
        VariantFields::Unit => {}
        VariantFields::Tuple(fields) | VariantFields::Struct(fields) => {
            for field in fields {
                let EnumFieldDefinition {
                    name: _,
                    name_span,
                    annotation,
                    ty: _,
                } = field;
                remap(name_span);
                remap_annotation(annotation, remap);
            }
        }
    }
}

fn remap_struct_field(field: &mut StructFieldDefinition, remap: &mut impl FnMut(&mut Span)) {
    let StructFieldDefinition {
        doc: _,
        name: _,
        name_span,
        annotation,
        visibility: _,
        ty: _,
        kind,
    } = field;
    remap(name_span);
    remap_annotation(annotation, remap);
    if let StructFieldKind::Named { attributes } = kind {
        for attribute in attributes {
            let Attribute {
                name: _,
                args: _,
                span,
            } = attribute;
            remap(span);
        }
    }
}

fn remap_interface(interface: &mut Interface, remap: &mut impl FnMut(&mut Span)) {
    let Interface {
        generics,
        parents: _,
        methods,
    } = interface;
    remap_generics(generics, remap);
    remap_methods(methods.values_mut(), remap);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecow::EcoString;
    use syntax::ast::{StructFields, Visibility as AstVisibility};
    use syntax::program::{MethodOrigin, Methods, ValueKind, Visibility};
    use syntax::types::Type;

    #[test]
    fn dummy_spans_are_not_remapped() {
        let mut definition = Definition {
            visibility: Visibility::Private,
            ty: Type::Error,
            name_span: Some(Span::dummy()),
            doc: None,
            body: DefinitionBody::Value {
                kind: ValueKind::Runtime,
                allowed_lints: Vec::new(),
                go_hints: Vec::new(),
                go_name: None,
                go_type_param_recipe: None,
            },
        };

        remap_definition_spans(&mut definition, &mut |span| {
            if !span.is_dummy() {
                span.file_id = 7;
            }
        });

        assert!(definition.name_span.is_some_and(|span| span.is_dummy()));
    }

    #[test]
    fn definition_round_trip_remaps_nested_spans() {
        let source_file = 17;
        let cached_file = 91;
        let span = || Span::new(source_file, 2, 3);
        let annotation = || Annotation::Constructor {
            name: EcoString::from("Item"),
            params: vec![Annotation::Opaque { span: span() }],
            writable: false,
            span: span(),
        };
        let mut methods = Methods::default();
        methods.insert(
            EcoString::from("get"),
            Method {
                source_name: EcoString::from("get"),
                ty: Type::Error,
                visibility: Visibility::Public,
                origin: MethodOrigin::Declared,
                name_span: Some(span()),
                doc: None,
                allowed_lints: Vec::new(),
                go_hints: Vec::new(),
            },
        );
        let definition = Definition {
            visibility: Visibility::Public,
            ty: Type::Error,
            name_span: Some(span()),
            doc: None,
            body: DefinitionBody::Struct {
                generics: vec![Generic::resolved(
                    "T",
                    [(annotation(), Type::Error)],
                    span(),
                )],
                fields: StructFields::Record(vec![StructFieldDefinition {
                    doc: None,
                    name: EcoString::from("field"),
                    name_span: span(),
                    annotation: annotation(),
                    visibility: AstVisibility::Private,
                    ty: Type::Error,
                    kind: StructFieldKind::Named {
                        attributes: vec![Attribute {
                            name: "go".to_string(),
                            args: Vec::new(),
                            span: span(),
                        }],
                    },
                }]),
                methods,
                attributes: Default::default(),
            },
        };
        let file_map = [(source_file, 0)].into_iter().collect();

        let cached = CachedDefinition::from_definition(&definition, &file_map);
        let bytes = bincode::serialize(&cached).unwrap();
        let cached: CachedDefinition = bincode::deserialize(&bytes).unwrap();
        let restored = cached.to_definition(&[cached_file]);
        let mut expected = definition;
        remap_definition_spans(&mut expected, &mut |span| span.file_id = cached_file);

        assert_eq!(restored, expected);
    }
}
