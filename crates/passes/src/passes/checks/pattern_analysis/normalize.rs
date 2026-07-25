use semantics::store::Store;
use syntax::ast::{
    ConstructorPatternResolution, Literal, MatchArm, Pattern, RecordPatternResolution,
    SequencePatternResolution,
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
    pub cache: &'a InhabitanceCache,
    pub scrutinee_type: Option<Type>,
}

impl<'a> NormalizationContext<'a> {
    /// Child context for a nested field/element, carrying that position's type
    /// as the scrutinee so the interface-implementer path can fire there.
    fn at_position(&self, scrutinee_type: Option<Type>) -> NormalizationContext<'a> {
        NormalizationContext {
            store: self.store,
            cache: self.cache,
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
) -> Vec<Row> {
    match &arm.pattern {
        Pattern::Or { patterns, .. } => patterns
            .iter()
            .map(|alt| vec![normalize_pattern(alt, unions, ctx)])
            .collect(),
        pattern => vec![vec![normalize_pattern(pattern, unions, ctx)]],
    }
}

pub fn normalize_pattern(
    pattern: &Pattern,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
) -> NormalizedPattern {
    match pattern {
        Pattern::Identifier { .. } | Pattern::WildCard { .. } | Pattern::Unit { .. } => Wildcard,

        Pattern::AsBinding { pattern, .. } => normalize_pattern(pattern, unions, ctx),

        Pattern::Literal { literal, .. } => {
            if let Literal::Boolean(b) = literal {
                return normalize_boolean(*b, unions);
            }

            NormalizedPattern::Literal(literal.clone())
        }

        Pattern::EnumVariant {
            fields,
            rest,
            resolution,
            ty,
            ..
        } => match resolution {
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
                    }) => {
                        let substitution = build_substitution_map(generics, &type_args);
                        fields
                            .iter()
                            .map(|field| substitute(&field.ty, &substitution))
                            .collect::<Vec<_>>()
                    }
                    Some(DefinitionBody::Enum {
                        variants, generics, ..
                    }) => {
                        let Some(variant) = variants
                            .iter()
                            .find(|variant| variant.name == unqualified_name(variant_name))
                        else {
                            return Wildcard;
                        };
                        let substitution = build_substitution_map(generics, &type_args);
                        variant
                            .fields
                            .iter()
                            .map(|field| substitute(&field.ty, &substitution))
                            .collect()
                    }
                    _ => return Wildcard,
                };
                let patterns: Vec<NormalizedPattern> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let child = ctx.at_position(field_types.get(i).cloned());
                        normalize_pattern(f, unions, &child)
                    })
                    .collect();

                let mut patterns = patterns;
                if *rest && patterns.len() < field_types.len() {
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

                if unions.get(&type_name).is_none() {
                    let alternatives = match enum_body {
                        Some(DefinitionBody::Enum {
                            variants, generics, ..
                        }) => variants
                            .iter()
                            .filter(|v| {
                                is_variant_inhabited(v, &type_args, generics, ctx.store, ctx.cache)
                            })
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
                            ctx.cache,
                        ) =>
                        {
                            vec![Constructor {
                                tag_id: tag.clone(),
                                arity: struct_fields.len(),
                            }]
                        }
                        _ => vec![],
                    };

                    unions.insert(type_name.clone(), alternatives);
                }

                NormalizedPattern::Constructor {
                    type_name,
                    tag,
                    args: patterns,
                }
            }
        },

        Pattern::Struct {
            fields,
            ty,
            resolution:
                RecordPatternResolution::EnumVariant {
                    enum_name,
                    variant_name,
                },
            ..
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
            let Some(variant) = variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))
            else {
                return Wildcard;
            };
            let substitution = build_substitution_map(generics, &type_args);
            let patterns = variant
                .fields
                .iter()
                .map(|f| {
                    fields
                        .iter()
                        .find_map(|field| {
                            if field.name == f.name {
                                let child = ctx.at_position(Some(substitute(&f.ty, &substitution)));
                                Some(normalize_pattern(&field.value, unions, &child))
                            } else {
                                None
                            }
                        })
                        .unwrap_or(Wildcard)
                })
                .collect();

            let type_name = make_type_key(enum_name, &type_args);

            if unions.get(&type_name).is_none() {
                let alternatives = variants
                    .iter()
                    .filter(|v| is_variant_inhabited(v, &type_args, generics, ctx.store, ctx.cache))
                    .map(|v| Constructor {
                        tag_id: format!("{}.{}", enum_name, v.name),
                        arity: v.fields.len(),
                    })
                    .collect();

                unions.insert(type_name.clone(), alternatives);
            }

            let variant_name = unqualified_name(variant_name);
            let tag = format!("{}.{}", enum_name, variant_name);

            NormalizedPattern::Constructor {
                type_name,
                tag,
                args: patterns,
            }
        }

        Pattern::Struct {
            fields,
            ty,
            resolution: RecordPatternResolution::Struct { struct_name },
            ..
        } => {
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
            let substitution = build_substitution_map(generics, &type_args);
            let patterns: Vec<NormalizedPattern> = struct_fields
                .iter()
                .map(|f| {
                    fields
                        .iter()
                        .find_map(|field| {
                            if field.name == f.name {
                                let child = ctx.at_position(Some(substitute(&f.ty, &substitution)));
                                Some(normalize_pattern(&field.value, unions, &child))
                            } else {
                                None
                            }
                        })
                        .unwrap_or(Wildcard)
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

            if unions.get(&type_name).is_none() {
                let is_inhabited = super::inhabitance::is_struct_inhabited(
                    struct_fields,
                    &type_args,
                    generics,
                    ctx.store,
                    ctx.cache,
                );

                if is_inhabited {
                    let constructor = Constructor {
                        tag_id: struct_name.to_string(),
                        arity: struct_fields.len(),
                    };
                    unions.insert(type_name.clone(), vec![constructor]);
                } else {
                    unions.insert(type_name.clone(), vec![]);
                }
            }

            NormalizedPattern::Constructor {
                type_name,
                tag: struct_name.to_string(),
                args: patterns,
            }
        }

        Pattern::Struct {
            resolution: RecordPatternResolution::Unresolved,
            ..
        } => Wildcard,

        Pattern::Slice {
            prefix,
            rest,
            resolution: SequencePatternResolution::Slice { element_type },
            ..
        } => normalize_slice(prefix, rest.is_present(), element_type, unions, ctx),

        Pattern::Slice {
            prefix,
            resolution:
                SequencePatternResolution::Array {
                    element_type,
                    length,
                },
            ..
        } => normalize_array(prefix, element_type, *length, unions, ctx),

        Pattern::Slice {
            resolution: SequencePatternResolution::Unresolved,
            ..
        } => Wildcard,

        Pattern::Tuple { elements, .. } => normalize_tuple(elements, unions, ctx),

        Pattern::Or { .. } => {
            unreachable!("Or-pattern should be handled by normalize_arm")
        }
    }
}

/// Normalize a slice pattern into nested EmptySlice/NonEmptySlice constructors.
///
/// Slice is modeled as a 2-variant type:
/// - EmptySlice: represents []
/// - NonEmptySlice(head, tail): represents [head, ..tail]
///
/// Examples:
/// - [] → EmptySlice
/// - [a] → NonEmptySlice(a, EmptySlice)
/// - [a, b] → NonEmptySlice(a, NonEmptySlice(b, EmptySlice))
/// - [a, ..rest] → NonEmptySlice(a, Wildcard)
/// - [..] → Wildcard (matches any slice)
fn normalize_slice(
    prefix: &[Pattern],
    has_rest: bool,
    element_type: &Type,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
) -> NormalizedPattern {
    let type_name = make_type_key("Slice", std::slice::from_ref(element_type));
    if unions.get(&type_name).is_none() {
        let element_inhabited = is_inhabited(element_type, ctx.store, ctx.cache);

        let mut constructors = vec![Constructor {
            tag_id: "EmptySlice".to_string(),
            arity: 0,
        }];

        if element_inhabited {
            constructors.push(Constructor {
                tag_id: "NonEmptySlice".to_string(),
                arity: 2, // head and tail
            });
        }

        unions.insert(type_name.clone(), constructors);
    }

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
        let head = normalize_pattern(element, unions, &element_ctx);
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
) -> NormalizedPattern {
    let arity = elements.len();
    let type_name = format!("Tuple{}", arity);

    if unions.get(&type_name).is_none() {
        let constructor = Constructor {
            tag_id: type_name.clone(),
            arity,
        };
        unions.insert(type_name.clone(), vec![constructor]);
    }

    let element_types = match ctx.scrutinee_type.as_ref().map(|t| ctx.store.peel_alias(t)) {
        Some(Type::Tuple(types)) => Some(types),
        _ => None,
    };

    let patterns = elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let child = ctx.at_position(element_types.as_ref().and_then(|ts| ts.get(i).cloned()));
            normalize_pattern(e, unions, &child)
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
) -> NormalizedPattern {
    let type_name = make_type_key(
        &format!("Array{length}"),
        std::slice::from_ref(element_type),
    );

    if length == 0 {
        if unions.get(&type_name).is_none() {
            unions.insert(
                type_name.clone(),
                vec![Constructor {
                    tag_id: "ArrayNil".to_string(),
                    arity: 0,
                }],
            );
        }
        return NormalizedPattern::Constructor {
            type_name,
            tag: "ArrayNil".to_string(),
            args: vec![],
        };
    }

    if unions.get(&type_name).is_none() {
        let constructors = if is_inhabited(element_type, ctx.store, ctx.cache) {
            vec![Constructor {
                tag_id: "ArrayCons".to_string(),
                arity: 2,
            }]
        } else {
            vec![]
        };
        unions.insert(type_name.clone(), constructors);
    }

    let Some((first, rest)) = prefix.split_first() else {
        return Wildcard;
    };

    let element_ctx = ctx.at_position(Some(element_type.clone()));
    let head = normalize_pattern(first, unions, &element_ctx);
    let tail = normalize_array(rest, element_type, length - 1, unions, ctx);

    NormalizedPattern::Constructor {
        type_name,
        tag: "ArrayCons".to_string(),
        args: vec![head, tail],
    }
}

fn normalize_boolean(boolean: bool, unions: &mut UnionTable) -> NormalizedPattern {
    let type_name = "Bool".to_string();

    if unions.get(&type_name).is_none() {
        let make_alt = |b: bool| Constructor {
            tag_id: b.to_string(),
            arity: 0,
        };

        unions.insert(type_name.clone(), vec![make_alt(true), make_alt(false)]);
    }

    NormalizedPattern::Constructor {
        type_name,
        tag: boolean.to_string(),
        args: vec![],
    }
}
