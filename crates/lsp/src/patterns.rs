//! Pattern-to-type resolution shared by hover and inlay hints.

use syntax::ast::{Pattern, RestPattern, Span, TypedPattern};
use syntax::types::{CompoundKind, Type};

/// Resolve the type and span of the pattern element at `offset`.
pub(crate) fn get_pattern_element_type(
    pattern: &Pattern,
    typed_pattern: Option<&TypedPattern>,
    fallback_ty: &Type,
    offset: u32,
) -> Option<(Type, Span)> {
    let span = pattern.get_span();
    if offset < span.byte_offset || offset >= span.byte_offset + span.byte_length {
        return None;
    }

    match (pattern, typed_pattern) {
        (Pattern::Identifier { .. }, _) => Some((fallback_ty.clone(), span)),

        (Pattern::Tuple { elements, .. }, typed) => {
            // Element types come from decomposing the tuple type; the typed sub-pattern
            // only carries deeper structure.
            let typed_elements = match typed {
                Some(TypedPattern::Tuple { elements, .. }) => Some(elements),
                _ => None,
            };
            let type_elements = match fallback_ty {
                Type::Tuple(elems) => elems,
                _ => return None,
            };
            elements.iter().enumerate().find_map(|(i, elem)| {
                let elem_ty = type_elements.get(i)?;
                let typed_elem = typed_elements.and_then(|te| te.get(i));
                get_pattern_element_type(elem, typed_elem, elem_ty, offset)
            })
        }

        (
            Pattern::EnumVariant { fields, .. },
            Some(TypedPattern::EnumVariant {
                fields: typed_fields,
                field_types,
                ..
            }),
        ) => fields
            .iter()
            .enumerate()
            .find_map(|(i, field)| {
                let field_ty = field_types.get(i).unwrap_or(fallback_ty);
                get_pattern_element_type(field, typed_fields.get(i), field_ty, offset)
            })
            .or_else(|| Some((fallback_ty.clone(), span))),

        (
            Pattern::EnumVariant { fields, .. },
            Some(TypedPattern::EnumStructVariant { variant_fields, .. }),
        ) => fields
            .iter()
            .enumerate()
            .find_map(|(i, field)| {
                let field_ty = variant_fields.get(i).map(|f| &f.ty).unwrap_or(fallback_ty);
                get_pattern_element_type(field, None, field_ty, offset)
            })
            .or_else(|| Some((fallback_ty.clone(), span))),

        (Pattern::EnumVariant { .. }, _) => Some((fallback_ty.clone(), span)),

        (Pattern::Struct { fields, .. }, Some(typed)) => {
            let (field_defs, pattern_fields): (Vec<_>, _) = match typed {
                TypedPattern::Struct {
                    struct_fields,
                    pattern_fields,
                    ..
                } => (
                    struct_fields.iter().map(|f| (&f.name, &f.ty)).collect(),
                    pattern_fields,
                ),
                TypedPattern::EnumStructVariant {
                    variant_fields,
                    pattern_fields,
                    ..
                } => (
                    variant_fields.iter().map(|f| (&f.name, &f.ty)).collect(),
                    pattern_fields,
                ),
                _ => return None,
            };

            fields.iter().find_map(|field| {
                let field_ty = field_defs
                    .iter()
                    .find(|(name, _)| *name == &field.name)
                    .map(|(_, ty)| *ty)
                    .unwrap_or(fallback_ty);
                let typed_field = pattern_fields
                    .iter()
                    .find(|(name, _)| name == &field.name)
                    .map(|(_, tp)| tp);
                get_pattern_element_type(&field.value, typed_field, field_ty, offset)
            })
        }

        (
            Pattern::Slice {
                prefix,
                rest,
                element_ty,
                ..
            },
            typed,
        ) => {
            let elem_type = match typed {
                Some(TypedPattern::Slice { element_type, .. }) => element_type,
                _ => element_ty,
            };

            prefix
                .iter()
                .find_map(|elem| get_pattern_element_type(elem, None, elem_type, offset))
                .or_else(|| {
                    if let RestPattern::Bind { span, .. } = rest
                        && offset >= span.byte_offset
                        && offset < span.byte_offset + span.byte_length
                    {
                        let slice_ty = Type::compound(CompoundKind::Slice, vec![elem_type.clone()]);
                        Some((slice_ty, *span))
                    } else {
                        None
                    }
                })
        }

        (Pattern::Or { patterns, .. }, Some(TypedPattern::Or { alternatives, .. })) => {
            patterns.iter().enumerate().find_map(|(i, alt)| {
                get_pattern_element_type(alt, alternatives.get(i), fallback_ty, offset)
            })
        }

        (
            Pattern::AsBinding {
                pattern: inner,
                name,
                ..
            },
            _,
        ) => get_pattern_element_type(inner, typed_pattern, fallback_ty, offset).or_else(|| {
            let binding_ty = inner.get_type().unwrap_or_else(|| fallback_ty.clone());
            let name_span = Span::new(
                span.file_id,
                span.byte_offset + span.byte_length - name.len() as u32,
                name.len() as u32,
            );
            Some((binding_ty, name_span))
        }),

        (Pattern::Literal { .. }, _) | (Pattern::WildCard { .. }, _) => {
            Some((fallback_ty.clone(), span))
        }

        _ => None,
    }
}
