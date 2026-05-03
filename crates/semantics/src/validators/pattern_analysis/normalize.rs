use crate::store::Store;
use syntax::ast::{Literal, MatchArm, TypedPattern};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Type, unqualified_name};

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

pub struct NormalizationContext<'a> {
    pub store: &'a Store,
    pub cache: &'a InhabitanceCache,
    pub scrutinee_type: Option<Type>,
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
    let typed_pattern = arm
        .typed_pattern
        .as_ref()
        .expect("typed pattern should be populated during inference");

    match typed_pattern {
        TypedPattern::Or { alternatives } => alternatives
            .iter()
            .map(|alt| vec![normalize_typed_pattern(alt, unions, ctx)])
            .collect(),
        _ => {
            vec![vec![normalize_typed_pattern(typed_pattern, unions, ctx)]]
        }
    }
}

pub fn normalize_typed_pattern(
    typed_pattern: &TypedPattern,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
) -> NormalizedPattern {
    match typed_pattern {
        TypedPattern::Wildcard => Wildcard,

        TypedPattern::Literal(literal) => {
            if let Literal::Boolean(b) = literal {
                return normalize_boolean(*b, unions);
            }

            NormalizedPattern::Literal(literal.clone())
        }

        TypedPattern::EnumVariant {
            enum_name,
            variant_name,
            fields,
            type_args,
            ..
        } => {
            let patterns: Vec<NormalizedPattern> = fields
                .iter()
                .map(|f| normalize_typed_pattern(f, unions, ctx))
                .collect();

            let enum_def = ctx.store.get_definition(enum_name);

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

            let type_name = make_type_key(enum_name, type_args);

            if unions.get(&type_name).is_none() {
                let alternatives = match enum_def.map(|d| &d.body) {
                    Some(DefinitionBody::Enum {
                        variants, generics, ..
                    }) => variants
                        .iter()
                        .filter(|v| {
                            is_variant_inhabited(v, type_args, generics, ctx.store, ctx.cache)
                        })
                        .map(|v| Constructor {
                            tag_id: format!("{}.{}", enum_name, v.name),
                            arity: v.fields.len(),
                        })
                        .collect(),
                    Some(DefinitionBody::ValueEnum { variants, .. }) => {
                        let mut alts: Vec<Constructor> = variants
                            .iter()
                            .map(|v| Constructor {
                                tag_id: format!("{}.{}", enum_name, v.name),
                                arity: 0,
                            })
                            .collect();
                        alts.push(Constructor {
                            tag_id: format!("{}.__value_enum_unknown__", enum_name),
                            arity: 0,
                        });
                        alts
                    }
                    _ => vec![],
                };

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

        TypedPattern::EnumStructVariant {
            enum_name,
            variant_name,
            variant_fields,
            pattern_fields,
            type_args,
        } => {
            let patterns = variant_fields
                .iter()
                .map(|f| {
                    pattern_fields
                        .iter()
                        .find_map(|(name, pattern)| {
                            if *name == f.name {
                                Some(normalize_typed_pattern(pattern, unions, ctx))
                            } else {
                                None
                            }
                        })
                        .unwrap_or(Wildcard)
                })
                .collect();

            let type_name = make_type_key(enum_name, type_args);

            if unions.get(&type_name).is_none() {
                let alternatives = match ctx.store.get_definition(enum_name).map(|d| &d.body) {
                    Some(DefinitionBody::Enum {
                        variants, generics, ..
                    }) => variants
                        .iter()
                        .filter(|v| {
                            is_variant_inhabited(v, type_args, generics, ctx.store, ctx.cache)
                        })
                        .map(|v| Constructor {
                            tag_id: format!("{}.{}", enum_name, v.name),
                            arity: v.fields.len(),
                        })
                        .collect(),
                    _ => vec![],
                };

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

        TypedPattern::Struct {
            struct_name,
            struct_fields,
            pattern_fields,
            type_args,
        } => {
            let patterns: Vec<NormalizedPattern> = struct_fields
                .iter()
                .map(|f| {
                    pattern_fields
                        .iter()
                        .find_map(|(name, pattern)| {
                            if *name == f.name {
                                Some(normalize_typed_pattern(pattern, unions, ctx))
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

            let type_name = make_type_key(struct_name, type_args);

            if unions.get(&type_name).is_none() {
                let is_inhabited = ctx
                    .store
                    .get_definition(struct_name)
                    .map(|definition| match &definition.body {
                        DefinitionBody::Struct {
                            generics, fields, ..
                        } => super::inhabitance::is_struct_inhabited(
                            fields, type_args, generics, ctx.store, ctx.cache,
                        ),
                        _ => true,
                    })
                    .unwrap_or(true);

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

        TypedPattern::Slice {
            prefix,
            has_rest,
            element_type,
        } => normalize_slice(prefix, *has_rest, element_type, unions, ctx),

        TypedPattern::Tuple { arity, elements } => normalize_tuple(elements, *arity, unions, ctx),

        TypedPattern::Or { .. } => {
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
    prefix: &[TypedPattern],
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

    let mut result = tail;
    for element in prefix.iter().rev() {
        let head = normalize_typed_pattern(element, unions, ctx);
        result = NormalizedPattern::Constructor {
            type_name: type_name.clone(),
            tag: "NonEmptySlice".to_string(),
            args: vec![head, result],
        };
    }

    result
}

fn normalize_tuple(
    elements: &[TypedPattern],
    arity: usize,
    unions: &mut UnionTable,
    ctx: &NormalizationContext,
) -> NormalizedPattern {
    let type_name = format!("Tuple{}", arity);

    if unions.get(&type_name).is_none() {
        let constructor = Constructor {
            tag_id: type_name.clone(),
            arity,
        };
        unions.insert(type_name.clone(), vec![constructor]);
    }

    let patterns = elements
        .iter()
        .map(|e| normalize_typed_pattern(e, unions, ctx))
        .collect();

    NormalizedPattern::Constructor {
        type_name: type_name.clone(),
        tag: type_name,
        args: patterns,
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
