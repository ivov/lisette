//! Pattern-to-type resolution shared by hover and inlay hints.

use syntax::ast::{
    ConstructorPatternResolution, Pattern, RecordPatternResolution, RestPattern,
    SequencePatternResolution, Span,
};
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, Type, build_substitution_map, substitute, unqualified_name};

use crate::snapshot::AnalysisSnapshot;

/// Resolve the type and span of the pattern element at `offset`.
pub(crate) fn get_pattern_element_type(
    snapshot: &AnalysisSnapshot,
    pattern: &Pattern,
    fallback_ty: &Type,
    offset: u32,
) -> Option<(Type, Span)> {
    let span = pattern.get_span();
    if offset < span.byte_offset || offset >= span.byte_offset + span.byte_length {
        return None;
    }

    match pattern {
        Pattern::Identifier { .. } => Some((fallback_ty.clone(), span)),

        Pattern::Tuple { elements, .. } => {
            let type_elements = match fallback_ty {
                Type::Tuple(elems) => elems,
                _ => return None,
            };
            elements.iter().enumerate().find_map(|(i, element)| {
                let elem_ty = type_elements.get(i)?;
                get_pattern_element_type(snapshot, element, elem_ty, offset)
            })
        }

        Pattern::EnumVariant {
            fields,
            resolution,
            ty,
            ..
        } => {
            let field_types = constructor_field_types(snapshot, resolution, ty);
            fields
                .iter()
                .enumerate()
                .find_map(|(i, field)| {
                    let field_ty = field_types.get(i).unwrap_or(fallback_ty);
                    get_pattern_element_type(snapshot, field, field_ty, offset)
                })
                .or_else(|| Some((fallback_ty.clone(), span)))
        }

        Pattern::Struct {
            fields,
            resolution,
            ty,
            ..
        } => fields.iter().find_map(|field| {
            let field_ty = record_field_type(snapshot, resolution, ty, &field.name);
            get_pattern_element_type(
                snapshot,
                &field.value,
                field_ty.as_ref().unwrap_or(fallback_ty),
                offset,
            )
        }),

        Pattern::Slice {
            prefix,
            rest,
            resolution,
            ..
        } => {
            let (element_type, array_length) = match resolution {
                SequencePatternResolution::Slice { element_type } => (element_type, None),
                SequencePatternResolution::Array {
                    element_type,
                    length,
                } => (element_type, Some(*length)),
                SequencePatternResolution::Unresolved => return None,
            };

            prefix
                .iter()
                .find_map(|element| {
                    get_pattern_element_type(snapshot, element, element_type, offset)
                })
                .or_else(|| {
                    if let RestPattern::Bind { span, .. } = rest
                        && offset >= span.byte_offset
                        && offset < span.byte_offset + span.byte_length
                    {
                        let rest_ty = match array_length {
                            Some(length) => Type::Array {
                                length: length.saturating_sub(prefix.len() as u64),
                                element: Box::new(element_type.clone()),
                            },
                            None => Type::compound(CompoundKind::Slice, vec![element_type.clone()]),
                        };
                        Some((rest_ty, *span))
                    } else {
                        None
                    }
                })
        }

        Pattern::Or { patterns, .. } => patterns.iter().find_map(|alternative| {
            get_pattern_element_type(snapshot, alternative, fallback_ty, offset)
        }),

        Pattern::AsBinding {
            pattern: inner,
            name,
            ..
        } => get_pattern_element_type(snapshot, inner, fallback_ty, offset).or_else(|| {
            let binding_ty = inner.get_type().unwrap_or_else(|| fallback_ty.clone());
            let name_span = Span::new(
                span.file_id,
                span.byte_offset + span.byte_length - name.len() as u32,
                name.len() as u32,
            );
            Some((binding_ty, name_span))
        }),

        Pattern::Literal { .. } | Pattern::WildCard { .. } | Pattern::Unit { .. } => {
            Some((fallback_ty.clone(), span))
        }
    }
}

fn pattern_type_args(snapshot: &AnalysisSnapshot, ty: &Type) -> Vec<Type> {
    match syntax::types::peel_alias(ty, |id| snapshot.definitions().get(id)) {
        Type::Nominal { params, .. } => params,
        _ => vec![],
    }
}

fn constructor_field_types(
    snapshot: &AnalysisSnapshot,
    resolution: &ConstructorPatternResolution,
    ty: &Type,
) -> Vec<Type> {
    let ConstructorPatternResolution::EnumVariant {
        enum_name,
        variant_name,
    } = resolution
    else {
        return vec![];
    };
    let params = pattern_type_args(snapshot, ty);
    let Some(definition) = snapshot.definitions().get(enum_name.as_str()) else {
        return vec![];
    };
    match &definition.body {
        DefinitionBody::Struct {
            fields, generics, ..
        } => {
            let substitution = build_substitution_map(generics, &params);
            fields
                .iter()
                .map(|field| substitute(&field.ty, &substitution))
                .collect()
        }
        DefinitionBody::Enum {
            variants, generics, ..
        } => {
            let Some(variant) = variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))
            else {
                return vec![];
            };
            let substitution = build_substitution_map(generics, &params);
            variant
                .fields
                .iter()
                .map(|field| substitute(&field.ty, &substitution))
                .collect()
        }
        _ => vec![],
    }
}

fn record_field_type(
    snapshot: &AnalysisSnapshot,
    resolution: &RecordPatternResolution,
    pattern_ty: &Type,
    field_name: &str,
) -> Option<Type> {
    let params = pattern_type_args(snapshot, pattern_ty);
    match resolution {
        RecordPatternResolution::Struct { struct_name } => {
            let DefinitionBody::Struct {
                fields, generics, ..
            } = &snapshot.definitions().get(struct_name.as_str())?.body
            else {
                return None;
            };
            let field = fields.iter().find(|field| field.name == field_name)?;
            Some(substitute(
                &field.ty,
                &build_substitution_map(generics, &params),
            ))
        }
        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
        } => {
            let DefinitionBody::Enum {
                variants, generics, ..
            } = &snapshot.definitions().get(enum_name.as_str())?.body
            else {
                return None;
            };
            let variant = variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))?;
            let field = variant
                .fields
                .iter()
                .find(|field| field.name == field_name)?;
            Some(substitute(
                &field.ty,
                &build_substitution_map(generics, &params),
            ))
        }
        RecordPatternResolution::Unresolved => None,
    }
}
