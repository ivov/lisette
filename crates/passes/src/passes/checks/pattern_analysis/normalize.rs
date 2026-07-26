use semantics::store::Store;
use syntax::ast::{
    ConstructorPatternResolution, EnumVariant, Generic, Literal, MatchArm, Pattern,
    RecordPatternResolution, SequencePatternResolution, StructFieldPattern,
};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Type, build_substitution_map, substitute, unqualified_name};

use super::NormalizedPattern::Wildcard;
use super::inhabitance::{InhabitanceCache, is_inhabited, is_variant_inhabited};
use super::types::Row;
use super::types::*;

fn make_type_key(name: &str, type_args: &[Type]) -> String {
    if type_args.is_empty() {
        name.to_string()
    } else {
        let args = type_args
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}<{}>", name, args)
    }
}

fn pattern_type_args(ctx: &NormalizationContext, ty: &Type) -> Vec<Type> {
    match ctx.store.peel_alias(ty) {
        Type::Nominal { params, .. } => params,
        _ => vec![],
    }
}

pub struct NormalizationContext<'a> {
    pub store: &'a Store,
    pub scrutinee_type: Option<Type>,
}

impl<'a> NormalizationContext<'a> {
    fn at_position(&self, scrutinee_type: Option<Type>) -> NormalizationContext<'a> {
        NormalizationContext {
            store: self.store,
            scrutinee_type,
        }
    }
}

fn try_normalize_interface_implementer(
    ctx: &NormalizationContext,
    struct_name: &str,
    arity: usize,
    args: Vec<NormalizedPattern>,
    unions: &mut UnionTable,
) -> Option<NormalizedPattern> {
    let scrutinee_ty = ctx.scrutinee_type.as_ref()?;
    let peeled = ctx.store.peel_alias(scrutinee_ty);
    let Type::Nominal {
        id: interface_id,
        params: interface_params,
        ..
    } = &peeled
    else {
        return None;
    };
    ctx.store.get_interface(interface_id)?;

    let interface_type_name = make_type_key(interface_id, interface_params);
    let struct_ctor = Constructor {
        tag_id: struct_name.to_string(),
        arity,
    };

    if let Some(union) = unions.get_mut(&interface_type_name) {
        let mut found = false;
        let mut unknown_pos = union.len();
        for (i, c) in union.iter().enumerate() {
            if c.tag_id == struct_name {
                found = true;
                break;
            }
            if c.tag_id == INTERFACE_UNKNOWN_TAG {
                unknown_pos = i;
            }
        }
        if !found {
            union.insert(unknown_pos, struct_ctor);
        }
    } else {
        unions.insert(
            interface_type_name.clone(),
            vec![
                struct_ctor,
                Constructor {
                    tag_id: INTERFACE_UNKNOWN_TAG.to_string(),
                    arity: 0,
                },
            ],
        );
    }

    Some(NormalizedPattern::Constructor {
        type_name: interface_type_name,
        tag: struct_name.to_string(),
        args,
    })
}

pub fn normalize_arm(
    arm: &MatchArm,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> Vec<Row> {
    match &arm.pattern {
        Pattern::Or { patterns, .. } => patterns
            .iter()
            .map(|alt| vec![normalize_pattern(alt, unions, ctx, cache)])
            .collect(),
        pattern => vec![vec![normalize_pattern(pattern, unions, ctx, cache)]],
    }
}

pub fn normalize_pattern(
    pattern: &Pattern,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    match pattern {
        Pattern::Identifier { .. } | Pattern::WildCard { .. } | Pattern::Unit { .. } => Wildcard,

        Pattern::AsBinding { pattern, .. } => normalize_pattern(pattern, unions, ctx, cache),

        Pattern::Literal { literal, .. } => {
            if let Literal::Boolean(b) = literal {
                return normalize_boolean(*b, unions);
            }

            NormalizedPattern::Literal(literal.clone())
        }

        Pattern::EnumVariant { .. } => normalize_enum_variant_pattern(pattern, unions, ctx, cache),

        Pattern::Struct {
            fields,
            ty,
            resolution,
            ..
        } => normalize_record_pattern(fields, ty, resolution, unions, ctx, cache),

        Pattern::Slice {
            prefix,
            rest,
            resolution: SequencePatternResolution::Slice { element_type },
            ..
        } => normalize_slice(prefix, rest.is_present(), element_type, unions, ctx, cache),

        Pattern::Slice {
            prefix,
            resolution:
                SequencePatternResolution::Array {
                    element_type,
                    length,
                },
            ..
        } => normalize_array(prefix, element_type, *length, unions, ctx, cache),

        Pattern::Slice {
            resolution: SequencePatternResolution::Unresolved,
            ..
        } => Wildcard,

        Pattern::Tuple { elements, .. } => normalize_tuple(elements, unions, ctx, cache),

        Pattern::Or { .. } => {
            unreachable!("Or-pattern should be handled by normalize_arm")
        }
    }
}

fn normalize_enum_variant_pattern(
    pattern: &Pattern,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    let Pattern::EnumVariant {
        fields,
        rest,
        resolution,
        ty,
        ..
    } = pattern
    else {
        unreachable!("normalize_enum_variant_pattern called with non-EnumVariant pattern");
    };
    let rest = *rest;
    match resolution {
        ConstructorPatternResolution::Unresolved => Wildcard,
        ConstructorPatternResolution::Const { qualified_name } => {
            NormalizedPattern::OpaqueConst(qualified_name.to_string())
        }
        ConstructorPatternResolution::ConstValue { value, .. } => match value {
            Literal::Boolean(b) => normalize_boolean(*b, unions),
            literal => NormalizedPattern::Literal(literal.clone()),
        },
        ConstructorPatternResolution::EnumVariant {
            enum_name,
            variant_name,
        } => {
            let type_args = pattern_type_args(ctx, ty);
            let enum_def = ctx.store.get_definition(enum_name);
            let field_types = match enum_def.map(|definition| &definition.body) {
                Some(DefinitionBody::Struct {
                    fields, generics, ..
                }) => substituted_field_types(
                    generics,
                    fields.iter().map(|field| &field.ty),
                    &type_args,
                ),
                Some(DefinitionBody::Enum {
                    variants, generics, ..
                }) => {
                    let Some(variant) = find_variant(variants, variant_name) else {
                        return Wildcard;
                    };
                    substituted_field_types(
                        generics,
                        variant.fields.iter().map(|field| &field.ty),
                        &type_args,
                    )
                }
                _ => return Wildcard,
            };
            let patterns: Vec<NormalizedPattern> = fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let child = ctx.at_position(field_types.get(i).cloned());
                    normalize_pattern(f, unions, &child, cache)
                })
                .collect();

            let mut patterns = patterns;
            if rest && patterns.len() < field_types.len() {
                patterns.resize(field_types.len(), Wildcard);
            }

            if let Some(Definition {
                body:
                    DefinitionBody::Struct {
                        fields: struct_fields,
                        ..
                    },
                ..
            }) = enum_def
            {
                let arity = struct_fields.len();
                let mut args = patterns.clone();
                while args.len() < arity {
                    args.push(Wildcard);
                }
                if let Some(normalized) =
                    try_normalize_interface_implementer(ctx, enum_name, arity, args, unions)
                {
                    return normalized;
                }
            }

            let type_name = make_type_key(enum_name, &type_args);
            let enum_body = enum_def.map(|d| &d.body);

            // Tuple struct / newtype tags are the bare type name (like record
            // structs); enum variants are `Type.Variant`.
            let is_struct_def = matches!(enum_body, Some(DefinitionBody::Struct { .. }));
            let tag = if is_struct_def {
                enum_name.to_string()
            } else {
                format!("{}.{}", enum_name, unqualified_name(variant_name))
            };

            register_union(unions, &type_name, || match enum_body {
                Some(DefinitionBody::Enum {
                    variants, generics, ..
                }) => variants
                    .iter()
                    .filter(|v| is_variant_inhabited(v, &type_args, generics, ctx.store, cache))
                    .map(|v| Constructor {
                        tag_id: format!("{}.{}", enum_name, v.name),
                        arity: v.fields.len(),
                    })
                    .collect(),
                Some(DefinitionBody::Struct {
                    fields: struct_fields,
                    generics,
                    ..
                }) if super::inhabitance::is_struct_inhabited(
                    struct_fields,
                    &type_args,
                    generics,
                    ctx.store,
                    cache,
                ) =>
                {
                    vec![Constructor {
                        tag_id: tag.clone(),
                        arity: struct_fields.len(),
                    }]
                }
                _ => vec![],
            });

            NormalizedPattern::Constructor {
                type_name,
                tag,
                args: patterns,
            }
        }
    }
}

fn normalize_record_pattern(
    fields: &[StructFieldPattern],
    ty: &Type,
    resolution: &RecordPatternResolution,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    match resolution {
        RecordPatternResolution::Unresolved => Wildcard,

        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
        } => {
            let type_args = pattern_type_args(ctx, ty);
            let Some(Definition {
                body:
                    DefinitionBody::Enum {
                        variants, generics, ..
                    },
                ..
            }) = ctx.store.get_definition(enum_name)
            else {
                return Wildcard;
            };
            let Some(variant) = find_variant(variants, variant_name) else {
                return Wildcard;
            };
            let field_types =
                substituted_field_types(generics, variant.fields.iter().map(|f| &f.ty), &type_args);
            let patterns = variant
                .fields
                .iter()
                .zip(field_types)
                .map(|(f, field_type)| {
                    field_pattern_or_wildcard(fields, &f.name, field_type, unions, ctx, cache)
                })
                .collect();

            let type_name = make_type_key(enum_name, &type_args);

            register_union(unions, &type_name, || {
                variants
                    .iter()
                    .filter(|v| is_variant_inhabited(v, &type_args, generics, ctx.store, cache))
                    .map(|v| Constructor {
                        tag_id: format!("{}.{}", enum_name, v.name),
                        arity: v.fields.len(),
                    })
                    .collect()
            });

            let tag = format!("{}.{}", enum_name, unqualified_name(variant_name));

            NormalizedPattern::Constructor {
                type_name,
                tag,
                args: patterns,
            }
        }

        RecordPatternResolution::Struct { struct_name } => {
            let type_args = pattern_type_args(ctx, ty);
            let Some(Definition {
                body:
                    DefinitionBody::Struct {
                        fields: struct_fields,
                        generics,
                        ..
                    },
                ..
            }) = ctx.store.get_definition(struct_name)
            else {
                return Wildcard;
            };
            let field_types =
                substituted_field_types(generics, struct_fields.iter().map(|f| &f.ty), &type_args);
            let patterns: Vec<NormalizedPattern> = struct_fields
                .iter()
                .zip(field_types)
                .map(|(f, field_type)| {
                    field_pattern_or_wildcard(fields, &f.name, field_type, unions, ctx, cache)
                })
                .collect();

            if let Some(normalized) = try_normalize_interface_implementer(
                ctx,
                struct_name,
                struct_fields.len(),
                patterns.clone(),
                unions,
            ) {
                return normalized;
            }

            let type_name = make_type_key(struct_name, &type_args);

            register_union(unions, &type_name, || {
                if super::inhabitance::is_struct_inhabited(
                    struct_fields,
                    &type_args,
                    generics,
                    ctx.store,
                    cache,
                ) {
                    vec![Constructor {
                        tag_id: struct_name.to_string(),
                        arity: struct_fields.len(),
                    }]
                } else {
                    vec![]
                }
            });

            NormalizedPattern::Constructor {
                type_name,
                tag: struct_name.to_string(),
                args: patterns,
            }
        }
    }
}

fn field_pattern_or_wildcard(
    written_fields: &[StructFieldPattern],
    field_name: &str,
    field_type: Type,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    written_fields
        .iter()
        .find_map(|field| {
            if field.name == field_name {
                let child = ctx.at_position(Some(field_type.clone()));
                Some(normalize_pattern(&field.value, unions, &child, cache))
            } else {
                None
            }
        })
        .unwrap_or(Wildcard)
}

fn substituted_field_types<'a>(
    generics: &[Generic],
    field_types: impl Iterator<Item = &'a Type>,
    type_args: &[Type],
) -> Vec<Type> {
    let substitution = build_substitution_map(generics, type_args);
    field_types
        .map(|ty| substitute(ty, &substitution))
        .collect()
}

fn find_variant<'a>(variants: &'a [EnumVariant], variant_name: &str) -> Option<&'a EnumVariant> {
    variants
        .iter()
        .find(|variant| variant.name == unqualified_name(variant_name))
}

fn register_union(unions: &mut UnionTable, type_name: &str, alternatives: impl FnOnce() -> Union) {
    if unions.get(type_name).is_none() {
        unions.insert(type_name.to_string(), alternatives());
    }
}

fn normalize_slice(
    prefix: &[Pattern],
    has_rest: bool,
    element_type: &Type,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    let type_name = make_type_key("Slice", std::slice::from_ref(element_type));
    register_union(unions, &type_name, || {
        let mut constructors = vec![Constructor {
            tag_id: "EmptySlice".to_string(),
            arity: 0,
        }];

        if is_inhabited(element_type, ctx.store, cache) {
            constructors.push(Constructor {
                tag_id: "NonEmptySlice".to_string(),
                arity: 2, // head and tail
            });
        }

        constructors
    });

    if prefix.is_empty() && has_rest {
        return Wildcard;
    }

    if prefix.is_empty() && !has_rest {
        return NormalizedPattern::Constructor {
            type_name,
            tag: "EmptySlice".to_string(),
            args: vec![],
        };
    }

    let tail = if has_rest {
        Wildcard
    } else {
        NormalizedPattern::Constructor {
            type_name: type_name.clone(),
            tag: "EmptySlice".to_string(),
            args: vec![],
        }
    };

    let element_ctx = ctx.at_position(Some(element_type.clone()));
    let mut result = tail;
    for element in prefix.iter().rev() {
        let head = normalize_pattern(element, unions, &element_ctx, cache);
        result = NormalizedPattern::Constructor {
            type_name: type_name.clone(),
            tag: "NonEmptySlice".to_string(),
            args: vec![head, result],
        };
    }

    result
}

fn normalize_tuple(
    elements: &[Pattern],
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    let arity = elements.len();
    let type_name = format!("Tuple{}", arity);

    register_union(unions, &type_name, || {
        vec![Constructor {
            tag_id: type_name.clone(),
            arity,
        }]
    });

    let element_types = match ctx.scrutinee_type.as_ref().map(|t| ctx.store.peel_alias(t)) {
        Some(Type::Tuple(types)) => Some(types),
        _ => None,
    };

    let patterns = elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let child = ctx.at_position(element_types.as_ref().and_then(|ts| ts.get(i).cloned()));
            normalize_pattern(e, unions, &child, cache)
        })
        .collect();

    NormalizedPattern::Constructor {
        type_name: type_name.clone(),
        tag: type_name,
        args: patterns,
    }
}

fn normalize_array(
    prefix: &[Pattern],
    element_type: &Type,
    length: u64,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
    cache: &mut InhabitanceCache,
) -> NormalizedPattern {
    let type_name = make_type_key(
        &format!("Array{length}"),
        std::slice::from_ref(element_type),
    );

    if length == 0 {
        register_union(unions, &type_name, || {
            vec![Constructor {
                tag_id: "ArrayNil".to_string(),
                arity: 0,
            }]
        });
        return NormalizedPattern::Constructor {
            type_name,
            tag: "ArrayNil".to_string(),
            args: vec![],
        };
    }

    register_union(unions, &type_name, || {
        if is_inhabited(element_type, ctx.store, cache) {
            vec![Constructor {
                tag_id: "ArrayCons".to_string(),
                arity: 2,
            }]
        } else {
            vec![]
        }
    });

    let Some((first, rest)) = prefix.split_first() else {
        return Wildcard;
    };

    let element_ctx = ctx.at_position(Some(element_type.clone()));
    let head = normalize_pattern(first, unions, &element_ctx, cache);
    let tail = normalize_array(rest, element_type, length - 1, unions, ctx, cache);

    NormalizedPattern::Constructor {
        type_name,
        tag: "ArrayCons".to_string(),
        args: vec![head, tail],
    }
}

fn normalize_boolean(boolean: bool, unions: &mut UnionTable) -> NormalizedPattern {
    let type_name = "Bool".to_string();

    register_union(unions, &type_name, || {
        let make_alt = |b: bool| Constructor {
            tag_id: b.to_string(),
            arity: 0,
        };

        vec![make_alt(true), make_alt(false)]
    });

    NormalizedPattern::Constructor {
        type_name,
        tag: boolean.to_string(),
        args: vec![],
    }
}
