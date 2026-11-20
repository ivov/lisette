use super::*;

pub(super) fn collect_checks_and_bindings(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    path_layout: Option<&ValueLayout>,
    collector: &mut PatternInfo,
) {
    match pattern {
        Pattern::WildCard { .. } | Pattern::Unit { .. } => {}

        Pattern::Identifier {
            identifier,
            binding,
            ..
        } => {
            let go_name = planner.facts.go_name_for_binding(pattern);
            collector.bindings.push(PatternBinding {
                lisette_name: identifier.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name, path_ty),
                path: path.clone(),
                coercion: path_layout.map_or(CoercionPlan::Identity, |source| {
                    CoercionPlan::bridge(
                        planner,
                        source,
                        &planner.value_layout(path_ty, SlotOrigin::Lisette),
                    )
                }),
            });
        }

        Pattern::Literal { literal, .. } => {
            collector.checks.push(Check::Literal {
                path: path.clone(),
                go_literal: GoExpression::literal(emit_pattern_literal(literal)),
                value: exact_switch_value(planner, path_ty, literal),
            });
        }

        Pattern::EnumVariant { resolution, ty, .. } => {
            let can_split_tuple =
                matches!(
                    resolution,
                    ConstructorPatternResolution::Const { .. }
                        | ConstructorPatternResolution::ConstValue { .. }
                ) || matches!(resolution, ConstructorPatternResolution::EnumVariant { .. })
                    && !planner.facts.is_tuple_struct_type(ty);
            collector.requires_materialized_subject |= !can_split_tuple;
            collect_enum_variant_checks(planner, path, pattern, path_ty, path_layout, collector);
        }

        Pattern::Struct { .. } => {
            collector.requires_materialized_subject = true;
            collect_struct_checks(planner, path, pattern, path_ty, collector);
        }

        Pattern::Tuple { elements, .. } => {
            collect_tuple_checks(planner, path, elements, path_ty, path_layout, collector);
        }

        Pattern::Slice { .. } => {
            collector.requires_materialized_subject = true;
            collect_slice_checks(planner, path, pattern, path_ty, path_layout, collector);
        }

        Pattern::Or { .. } => {
            collect_or_pattern_checks(planner, path, pattern, path_ty, path_layout, collector);
        }

        p @ Pattern::AsBinding {
            pattern: inner,
            name,
            binding,
            ..
        } => {
            collect_checks_and_bindings(planner, path, inner, path_ty, path_layout, collector);
            let go_name = planner.facts.go_name_for_binding(p);
            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name, path_ty),
                path: path.clone(),
                coercion: path_layout.map_or(CoercionPlan::Identity, |source| {
                    CoercionPlan::bridge(
                        planner,
                        source,
                        &planner.value_layout(path_ty, SlotOrigin::Lisette),
                    )
                }),
            });
        }
    }
}

fn collect_tuple_checks(
    planner: &Planner,
    path: &AccessPath,
    elements: &[Pattern],
    path_ty: &Type,
    path_layout: Option<&ValueLayout>,
    collector: &mut PatternInfo,
) {
    let Type::Tuple(element_tys) = planner.facts.strip_and_peel(path_ty) else {
        unreachable!("tuple pattern on a non-tuple type: {path_ty:?}");
    };

    for (i, element) in elements.iter().enumerate() {
        let field_name = TUPLE_FIELDS.get(i).expect("oversize tuple arity");
        let field_path = path.push(field_segment(planner, path_ty, field_name.to_string()));
        collect_checks_and_bindings(
            planner,
            &field_path,
            element,
            &element_tys[i],
            path_layout
                .and_then(ValueLayout::tuple_elements)
                .and_then(|elements| elements.get(i)),
            collector,
        );
    }
}

fn collect_slice_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    path_layout: Option<&ValueLayout>,
    collector: &mut PatternInfo,
) {
    let Pattern::Slice {
        prefix,
        rest,
        resolution,
        ..
    } = pattern
    else {
        return;
    };
    let array_info = match resolution {
        SequencePatternResolution::Array {
            length,
            element_type,
        } => Some((*length, element_type)),
        _ => None,
    };

    if array_info.is_none() {
        if !rest.is_present() {
            collector.checks.push(Check::SliceLenEq {
                path: path.clone(),
                length: prefix.len(),
            });
        } else if !prefix.is_empty() {
            collector.checks.push(Check::SliceLenGe {
                path: path.clone(),
                min_length: prefix.len(),
            });
        }
    }

    let element_type = match resolution {
        SequencePatternResolution::Slice { element_type }
        | SequencePatternResolution::Array { element_type, .. } => element_type,
        SequencePatternResolution::Unresolved => {
            unreachable!("slice pattern without a resolved element type")
        }
    };

    for (i, element) in prefix.iter().enumerate() {
        let element_path = path.push(PathSegment::Index(i));
        collect_checks_and_bindings(
            planner,
            &element_path,
            element,
            element_type,
            path_layout.and_then(ValueLayout::element),
            collector,
        );
    }

    if let RestPattern::Bind { name, binding, .. } = rest {
        let go_name = planner.facts.go_name_for_rest_binding(rest);
        let (segment, rest_ty) = match &array_info {
            Some((length, element_type)) => {
                let sub_length = length.saturating_sub(prefix.len() as u64);
                let sub_ty = Type::Array {
                    length: sub_length,
                    element: Box::new((*element_type).clone()),
                };
                let go_type = planner.go_type(&sub_ty);
                (
                    PathSegment::ArraySliceFrom {
                        offset: prefix.len(),
                        go_type,
                    },
                    sub_ty,
                )
            }
            None => (PathSegment::SliceFrom(prefix.len()), path_ty.clone()),
        };
        collector.bindings.push(PatternBinding {
            lisette_name: name.to_string(),
            binding_ids: binding.as_slice().to_vec(),
            target: BindingTarget::from_go_name(go_name, &rest_ty),
            path: path.push(segment),
            coercion: match (&array_info, path_layout) {
                (None, Some(source)) => CoercionPlan::bridge(
                    planner,
                    source,
                    &planner.value_layout(&rest_ty, SlotOrigin::Lisette),
                ),
                _ => CoercionPlan::Identity,
            },
        });
    }
}

fn collect_or_pattern_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    path_layout: Option<&ValueLayout>,
    collector: &mut PatternInfo,
) {
    let Pattern::Or { patterns, .. } = pattern else {
        unreachable!("or-pattern collection requires an or-pattern");
    };
    assert!(
        !pattern_has_bindings(pattern),
        "split or-patterns that bind names with `split_binding_or_patterns` before collection"
    );
    let alt_collectors: Vec<PatternInfo> = patterns
        .iter()
        .map(|p| {
            let mut alt_collector = PatternInfo::new();
            collect_checks_and_bindings(planner, path, p, path_ty, path_layout, &mut alt_collector);
            alt_collector
        })
        .collect();

    if alt_collectors.iter().any(|c| c.checks.is_empty()) {
        return;
    }

    collector.checks.push(Check::Or {
        alternatives: alt_collectors.into_iter().map(|c| c.checks).collect(),
    });
}

fn compute_struct_field_path(
    planner: &Planner,
    parent_path: &AccessPath,
    field: &StructFieldPattern,
    ty: &Type,
    enum_info: Option<&(String, String)>,
) -> AccessPath {
    let go_field_name = if let Some((enum_id, variant_name)) = enum_info {
        planner
            .facts
            .enum_struct_field_name(enum_id, variant_name, &field.name)
            .unwrap_or_else(|| {
                panic!(
                    "enum layout not found: {}.{}.{}",
                    enum_id, variant_name, field.name
                )
            })
    } else {
        planner.struct_field_go_name(ty, &field.name, false)
    };

    if let Some((_, variant_name)) = enum_info
        && let Some(field_index) =
            planner
                .facts
                .get_enum_struct_field_index(ty, variant_name, &field.name)
        && planner
            .facts
            .is_enum_field_recursive(ty, variant_name, field_index)
    {
        return parent_path
            .push(field_segment(planner, ty, go_field_name))
            .push(PathSegment::Deref);
    }

    parent_path.push(field_segment(planner, ty, go_field_name))
}

fn interface_assert_child_path(
    planner: &Planner,
    path: &AccessPath,
    pattern_ty: &Type,
    path_ty: &Type,
    collector: &mut PatternInfo,
) -> Option<AccessPath> {
    planner.facts.as_interface(path_ty)?;
    let go_type = planner.go_type(pattern_ty);
    collector.checks.push(Check::TypeAssert {
        path: path.clone(),
        go_type: go_type.clone(),
    });
    Some(path.push(PathSegment::AssertedAs(go_type)))
}

fn collect_enum_variant_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    path_layout: Option<&ValueLayout>,
    collector: &mut PatternInfo,
) {
    let path_layout = path_layout.map(ValueLayout::underlying);
    let Pattern::EnumVariant {
        identifier,
        fields,
        resolution,
        ty,
        ..
    } = pattern
    else {
        return;
    };

    match resolution {
        ConstructorPatternResolution::Const { qualified_name } => {
            collect_const_pattern_check(planner, path, qualified_name, None, collector);
            return;
        }
        ConstructorPatternResolution::ConstValue {
            qualified_name,
            value,
        } => {
            let value = (!matches!(value, Literal::String { .. }))
                .then(|| exact_switch_value(planner, path_ty, value))
                .flatten();
            collect_const_pattern_check(planner, path, qualified_name, value, collector);
            return;
        }
        _ => {}
    }

    let ConstructorPatternResolution::EnumVariant {
        enum_name,
        variant_name,
    } = resolution
    else {
        return;
    };
    if let Some(
        layout @ (ValueLayout::NullableOption { payload, .. }
        | ValueLayout::PointerOption { payload, .. }),
    ) = path_layout
    {
        let some = unqualified_name(variant_name) == "Some";
        assert!(
            some || unqualified_name(variant_name) == "None",
            "nilable option patterns require Some or None"
        );
        let pointer = matches!(layout, ValueLayout::PointerOption { .. });
        collector.checks.push(Check::Nilable {
            path: path.clone(),
            guard: if pointer {
                NilGuard::Pointer
            } else {
                planner.facts.option_nil_guard(ty)
            },
            some,
        });
        if some {
            let [field] = fields.as_slice() else {
                unreachable!("Some pattern has one payload");
            };
            let child_path = if pointer {
                path.push(PathSegment::Deref)
            } else {
                path.clone()
            };
            collect_checks_and_bindings(
                planner,
                &child_path,
                field,
                &ty.ok_type(),
                Some(payload),
                collector,
            );
        }
        return;
    }
    let params = pattern_type_args(planner, ty);
    let Some(definition) = planner.facts.definition(enum_name) else {
        return;
    };
    let (declared_fields, generics) = match &definition.body {
        DefinitionBody::Struct {
            fields, generics, ..
        } => (
            fields.iter().map(|field| &field.ty).collect::<Vec<_>>(),
            generics,
        ),
        DefinitionBody::Enum {
            variants, generics, ..
        } => {
            let Some(variant) = variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))
            else {
                return;
            };
            (
                variant
                    .fields
                    .iter()
                    .map(|field| &field.ty)
                    .collect::<Vec<_>>(),
                generics,
            )
        }
        _ => return,
    };

    let field_types: Vec<Type> = declared_fields
        .iter()
        .map(|ty| generics::resolve_field_type(generics, &params, ty))
        .collect();
    let field_layouts: Vec<ValueLayout> = declared_fields
        .iter()
        .zip(&field_types)
        .enumerate()
        .map(|(index, (declared, ty))| {
            let inherited = path_layout.and_then(|layout| match layout {
                ValueLayout::TaggedOption { payload, .. }
                | ValueLayout::NullableOption { payload, .. }
                | ValueLayout::PointerOption { payload, .. } => Some(payload.as_ref()),
                ValueLayout::Fallible { payload, error, .. } => {
                    Some(if unqualified_name(variant_name) == "Err" || index == 1 {
                        error.as_ref()
                    } else {
                        payload.as_ref()
                    })
                }
                _ => None,
            });
            inherited.cloned().unwrap_or_else(|| {
                planner.value_layout_with_declaration(ty, SlotOrigin::Lisette, declared)
            })
        })
        .collect();

    let variant_data = EnumVariantData {
        identifier,
        fields,
        ty,
        field_types: &field_types,
        field_layouts: &field_layouts,
    };

    if planner.facts.is_tuple_struct_type(ty) {
        let child_path = interface_assert_child_path(planner, path, ty, path_ty, collector)
            .unwrap_or_else(|| path.clone());
        if planner.facts.is_newtype_struct(ty) {
            collect_newtype_checks(planner, &child_path, &variant_data, collector);
        } else {
            collect_tuple_struct_checks(
                planner,
                &child_path,
                fields,
                &field_types,
                &field_layouts,
                collector,
            );
        }
        return;
    }

    if handle_foreign_variant_literal(planner, path, ty, identifier, collector) {
        return;
    }

    collect_tagged_enum_checks(planner, path, &variant_data, collector);
}

fn collect_const_pattern_check(
    planner: &Planner,
    path: &AccessPath,
    qualified_name: &str,
    value: Option<Literal>,
    collector: &mut PatternInfo,
) {
    collector.checks.push(Check::Literal {
        path: path.clone(),
        go_literal: planner.definition_reference(qualified_name),
        value,
    });
}

fn exact_switch_value(planner: &Planner, path_ty: &Type, value: &Literal) -> Option<Literal> {
    let kind = planner.facts.underlying_simple_kind(path_ty)?;
    let exact = kind.is_integer() || matches!(kind, SimpleKind::Bool | SimpleKind::String);
    (exact && !matches!(value, Literal::Float { .. })).then(|| value.clone())
}

fn handle_foreign_variant_literal(
    planner: &Planner,
    path: &AccessPath,
    ty: &Type,
    identifier: &str,
    collector: &mut PatternInfo,
) -> bool {
    if planner.facts.as_enum(ty).is_some() || !identifier.contains('.') {
        return false;
    }
    let go_literal = match identifier.split_once('.') {
        Some((package, member)) if planner.facts.is_foreign_package(package) => {
            GoExpression::qualified(
                planner.package_use_for_package(&planner.canonical_package(package)),
                member,
            )
        }
        _ => GoExpression::external_name(identifier.to_string()),
    };
    collector.checks.push(Check::Literal {
        path: path.clone(),
        go_literal,
        value: None,
    });
    true
}

fn collect_newtype_checks(
    planner: &Planner,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternInfo,
) {
    let Some(underlying_ty) = planner.facts.get_newtype_underlying(variant.ty) else {
        return;
    };
    let go_underlying = planner.go_type(&underlying_ty);
    let field_path = path.push(PathSegment::NewtypeCast(go_underlying));
    if let Some(field) = variant.fields.first() {
        collect_checks_and_bindings(
            planner,
            &field_path,
            field,
            &variant.field_types[0],
            variant.field_layouts.first(),
            collector,
        );
    }
}

fn collect_tuple_struct_checks(
    planner: &Planner,
    path: &AccessPath,
    fields: &[Pattern],
    field_types: &[Type],
    field_layouts: &[ValueLayout],
    collector: &mut PatternInfo,
) {
    for (i, field) in fields.iter().enumerate() {
        let field_path = path.push(PathSegment::Field(go_name::tuple_struct_field(i)));
        collect_checks_and_bindings(
            planner,
            &field_path,
            field,
            &field_types[i],
            field_layouts.get(i),
            collector,
        );
    }
}

struct EnumVariantData<'a> {
    identifier: &'a str,
    fields: &'a [Pattern],
    ty: &'a Type,
    field_types: &'a [Type],
    field_layouts: &'a [ValueLayout],
}

fn enum_tag_constant(planner: &Planner, identifier: &str, ty: &Type) -> GoExpression {
    match ty {
        Type::Nominal { id, .. } => planner.resolve_variant(identifier, id),
        _ => GoExpression::external_name(identifier.replace('.', "_")),
    }
}

fn collect_tagged_enum_checks(
    planner: &Planner,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternInfo,
) {
    collector.checks.push(Check::EnumTag {
        path: path.clone(),
        tag_constant: enum_tag_constant(planner, variant.identifier, variant.ty),
    });

    let variant_name = variant
        .identifier
        .split('.')
        .next_back()
        .unwrap_or(variant.identifier);
    for (i, field) in variant.fields.iter().enumerate() {
        let field_name = planner
            .facts
            .get_enum_tuple_field_name(variant.ty, variant_name, i);

        let is_unit = planner
            .facts
            .is_enum_field_unit(variant.ty, variant_name, i);

        let field_path = if planner
            .facts
            .is_enum_field_recursive(variant.ty, variant_name, i)
        {
            path.push(field_segment(planner, variant.ty, field_name))
                .push(PathSegment::Deref)
        } else {
            path.push(field_segment(planner, variant.ty, field_name))
        };

        if is_unit {
            if let Pattern::Identifier {
                identifier,
                binding,
                ..
            } = field
            {
                collector.bindings.push(PatternBinding {
                    lisette_name: identifier.to_string(),
                    binding_ids: binding.as_slice().to_vec(),
                    target: BindingTarget::Unit,
                    path: field_path,
                    coercion: CoercionPlan::Identity,
                });
            }
        } else {
            collect_checks_and_bindings(
                planner,
                &field_path,
                field,
                &variant.field_types[i],
                variant.field_layouts.get(i),
                collector,
            );
        }
    }
}

fn detect_enum_info(resolution: &RecordPatternResolution) -> Option<(String, String)> {
    match resolution {
        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
            ..
        } => Some((
            enum_name.to_string(),
            unqualified_name(variant_name).to_string(),
        )),
        RecordPatternResolution::Struct { .. } | RecordPatternResolution::Unresolved => None,
    }
}

fn collect_struct_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    let Pattern::Struct {
        fields,
        ty,
        resolution,
        ..
    } = pattern
    else {
        return;
    };

    let (enum_info, child_path) =
        resolve_struct_child_path(planner, path, pattern, path_ty, collector);
    let variant = match resolution {
        RecordPatternResolution::EnumVariant { variant_name, .. } => Some(variant_name.as_str()),
        RecordPatternResolution::Struct { .. } | RecordPatternResolution::Unresolved => None,
    };

    for field in fields {
        let field_path =
            compute_struct_field_path(planner, &child_path, field, ty, enum_info.as_ref());
        let field_ty = planner
            .lookup_field_ty(ty, variant, &field.name)
            .unwrap_or_else(|| panic!("record pattern field not resolved: {}", field.name));
        let layout = planner.field_slot_layout(ty, None, variant, &field.name, &field_ty);
        collect_checks_and_bindings(
            planner,
            &field_path,
            &field.value,
            &field_ty,
            layout.as_ref(),
            collector,
        );
    }
}

fn resolve_struct_child_path(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) -> (Option<(String, String)>, AccessPath) {
    let Pattern::Struct {
        ty,
        identifier,
        resolution,
        ..
    } = pattern
    else {
        unreachable!("resolve_struct_child_path requires a Struct pattern");
    };
    let enum_info = detect_enum_info(resolution);
    let child_path = interface_assert_child_path(planner, path, ty, path_ty, collector)
        .unwrap_or_else(|| path.clone());
    if enum_info.is_some() {
        collector.checks.push(Check::EnumTag {
            path: child_path.clone(),
            tag_constant: enum_tag_constant(planner, identifier, ty),
        });
    }
    (enum_info, child_path)
}

fn pattern_type_args(planner: &Planner, ty: &Type) -> Vec<Type> {
    match planner.facts.peel_alias(ty) {
        Type::Nominal { params, .. } => params,
        _ => vec![],
    }
}
