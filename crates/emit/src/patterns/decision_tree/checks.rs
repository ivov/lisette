use super::*;
use crate::EmitFacts;

struct EnumVariantData<'a> {
    identifier: &'a str,
    fields: &'a [Pattern],
    ty: &'a Type,
    field_types: &'a [Type],
    field_layouts: &'a [ValueLayout],
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

impl Planner<'_> {
    pub(super) fn collect_checks_and_bindings(
        &self,
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
                let go_name = self.facts.go_name_for_binding(pattern);
                collector.bindings.push(PatternBinding {
                    lisette_name: identifier.to_string(),
                    binding_ids: binding.as_slice().to_vec(),
                    target: BindingTarget::from_go_name(go_name, path_ty),
                    path: path.clone(),
                    coercion: path_layout.map_or(CoercionPlan::Identity, |source| {
                        CoercionPlan::bridge(
                            self,
                            source,
                            &self.value_layout(path_ty, SlotOrigin::Lisette),
                        )
                    }),
                });
            }

            Pattern::Literal { literal, .. } => {
                collector.checks.push(Check::Literal {
                    path: path.clone(),
                    go_literal: GoExpression::literal(pattern_literal_text(literal)),
                    value: self.facts.exact_switch_value(path_ty, literal),
                });
            }

            Pattern::EnumVariant { resolution, ty, .. } => {
                let can_split_tuple =
                    matches!(
                        resolution,
                        ConstructorPatternResolution::Const { .. }
                            | ConstructorPatternResolution::ConstValue { .. }
                    ) || matches!(resolution, ConstructorPatternResolution::EnumVariant { .. })
                        && !self.facts.is_tuple_struct_type(ty);
                collector.requires_materialized_subject |= !can_split_tuple;
                self.collect_enum_variant_checks(path, pattern, path_ty, path_layout, collector);
            }

            Pattern::Struct { .. } => {
                collector.requires_materialized_subject = true;
                self.collect_struct_checks(path, pattern, path_ty, collector);
            }

            Pattern::Tuple { elements, .. } => {
                self.collect_tuple_checks(path, elements, path_ty, path_layout, collector);
            }

            Pattern::Slice { .. } => {
                collector.requires_materialized_subject = true;
                self.collect_slice_checks(path, pattern, path_ty, path_layout, collector);
            }

            Pattern::Or { .. } => {
                self.collect_or_pattern_checks(path, pattern, path_ty, path_layout, collector);
            }

            p @ Pattern::AsBinding {
                pattern: inner,
                name,
                binding,
                ..
            } => {
                self.collect_checks_and_bindings(path, inner, path_ty, path_layout, collector);
                let go_name = self.facts.go_name_for_binding(p);
                collector.bindings.push(PatternBinding {
                    lisette_name: name.to_string(),
                    binding_ids: binding.as_slice().to_vec(),
                    target: BindingTarget::from_go_name(go_name, path_ty),
                    path: path.clone(),
                    coercion: path_layout.map_or(CoercionPlan::Identity, |source| {
                        CoercionPlan::bridge(
                            self,
                            source,
                            &self.value_layout(path_ty, SlotOrigin::Lisette),
                        )
                    }),
                });
            }
        }
    }

    fn collect_tuple_checks(
        &self,
        path: &AccessPath,
        elements: &[Pattern],
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) {
        let Type::Tuple(element_tys) = self.facts.strip_and_peel(path_ty) else {
            unreachable!("tuple pattern on a non-tuple type: {path_ty:?}");
        };

        for (i, element) in elements.iter().enumerate() {
            let field_name = TUPLE_FIELDS.get(i).expect("oversize tuple arity");
            let field_path = path.push(self.facts.field_segment(path_ty, field_name.to_string()));
            self.collect_checks_and_bindings(
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
        &self,
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
            self.collect_checks_and_bindings(
                &element_path,
                element,
                element_type,
                path_layout.and_then(ValueLayout::element),
                collector,
            );
        }

        if let RestPattern::Bind { name, binding, .. } = rest {
            let go_name = self.facts.go_name_for_rest_binding(rest);
            let (segment, rest_ty) = match &array_info {
                Some((length, element_type)) => {
                    let sub_length = length.saturating_sub(prefix.len() as u64);
                    let sub_ty = Type::Array {
                        length: sub_length,
                        element: Box::new((*element_type).clone()),
                    };
                    let go_type = self.go_type(&sub_ty);
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
                        self,
                        source,
                        &self.value_layout(&rest_ty, SlotOrigin::Lisette),
                    ),
                    _ => CoercionPlan::Identity,
                },
            });
        }
    }

    fn collect_or_pattern_checks(
        &self,
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
                self.collect_checks_and_bindings(path, p, path_ty, path_layout, &mut alt_collector);
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
        &self,
        parent_path: &AccessPath,
        field: &StructFieldPattern,
        ty: &Type,
        enum_info: Option<&(String, String)>,
    ) -> AccessPath {
        let go_field_name = if let Some((enum_id, variant_name)) = enum_info {
            self.facts
                .enum_struct_field_name(enum_id, variant_name, &field.name)
                .unwrap_or_else(|| {
                    panic!(
                        "enum layout not found: {}.{}.{}",
                        enum_id, variant_name, field.name
                    )
                })
        } else {
            self.struct_field_go_name(ty, &field.name, false)
        };

        if let Some((_, variant_name)) = enum_info
            && let Some(field_index) =
                self.facts
                    .enum_struct_field_index(ty, variant_name, &field.name)
            && self
                .facts
                .is_enum_field_recursive(ty, variant_name, field_index)
        {
            return parent_path
                .push(self.facts.field_segment(ty, go_field_name))
                .push(PathSegment::Deref);
        }

        parent_path.push(self.facts.field_segment(ty, go_field_name))
    }

    fn interface_assert_child_path(
        &self,
        path: &AccessPath,
        pattern_ty: &Type,
        path_ty: &Type,
        collector: &mut PatternInfo,
    ) -> Option<AccessPath> {
        self.facts.as_interface(path_ty)?;
        let go_type = self.go_type(pattern_ty);
        collector.checks.push(Check::TypeAssert {
            path: path.clone(),
            go_type: go_type.clone(),
        });
        Some(path.push(PathSegment::AssertedAs(go_type)))
    }

    fn collect_enum_variant_checks(
        &self,
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
                self.collect_const_pattern_check(path, qualified_name, None, collector);
                return;
            }
            ConstructorPatternResolution::ConstValue {
                qualified_name,
                value,
            } => {
                let value = (!matches!(value, Literal::String { .. }))
                    .then(|| self.facts.exact_switch_value(path_ty, value))
                    .flatten();
                self.collect_const_pattern_check(path, qualified_name, value, collector);
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
                    self.facts.option_nil_guard(ty)
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
                self.collect_checks_and_bindings(
                    &child_path,
                    field,
                    &ty.ok_type(),
                    Some(payload),
                    collector,
                );
            }
            return;
        }
        let params = self.facts.pattern_type_args(ty);
        let Some(definition) = self.facts.definition(enum_name) else {
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
                    self.value_layout_with_declaration(ty, SlotOrigin::Lisette, declared)
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

        if self.facts.is_tuple_struct_type(ty) {
            let child_path = self
                .interface_assert_child_path(path, ty, path_ty, collector)
                .unwrap_or_else(|| path.clone());
            if self.facts.is_newtype_struct(ty) {
                self.collect_newtype_checks(&child_path, &variant_data, collector);
            } else {
                self.collect_tuple_struct_checks(
                    &child_path,
                    fields,
                    &field_types,
                    &field_layouts,
                    collector,
                );
            }
            return;
        }

        if self.handle_foreign_variant_literal(path, ty, identifier, collector) {
            return;
        }

        self.collect_tagged_enum_checks(path, &variant_data, collector);
    }

    fn collect_const_pattern_check(
        &self,
        path: &AccessPath,
        qualified_name: &str,
        value: Option<Literal>,
        collector: &mut PatternInfo,
    ) {
        collector.checks.push(Check::Literal {
            path: path.clone(),
            go_literal: self.definition_reference(qualified_name),
            value,
        });
    }

    fn handle_foreign_variant_literal(
        &self,
        path: &AccessPath,
        ty: &Type,
        identifier: &str,
        collector: &mut PatternInfo,
    ) -> bool {
        if self.facts.as_enum(ty).is_some() || !identifier.contains('.') {
            return false;
        }
        let go_literal = match identifier.split_once('.') {
            Some((package, member)) if self.facts.is_foreign_package(package) => {
                GoExpression::qualified(
                    self.package_use_for_package(&self.canonical_package(package)),
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
        &self,
        path: &AccessPath,
        variant: &EnumVariantData,
        collector: &mut PatternInfo,
    ) {
        let Some(underlying_ty) = self.facts.newtype_underlying(variant.ty) else {
            return;
        };
        let go_underlying = self.go_type(&underlying_ty);
        let field_path = path.push(PathSegment::NewtypeCast(go_underlying));
        if let Some(field) = variant.fields.first() {
            self.collect_checks_and_bindings(
                &field_path,
                field,
                &variant.field_types[0],
                variant.field_layouts.first(),
                collector,
            );
        }
    }

    fn collect_tuple_struct_checks(
        &self,
        path: &AccessPath,
        fields: &[Pattern],
        field_types: &[Type],
        field_layouts: &[ValueLayout],
        collector: &mut PatternInfo,
    ) {
        for (i, field) in fields.iter().enumerate() {
            let field_path = path.push(PathSegment::Field(go_name::tuple_struct_field(i)));
            self.collect_checks_and_bindings(
                &field_path,
                field,
                &field_types[i],
                field_layouts.get(i),
                collector,
            );
        }
    }

    fn enum_tag_constant(&self, identifier: &str, ty: &Type) -> GoExpression {
        match ty {
            Type::Nominal { id, .. } => self.resolve_variant(identifier, id),
            _ => GoExpression::external_name(identifier.replace('.', "_")),
        }
    }

    fn collect_tagged_enum_checks(
        &self,
        path: &AccessPath,
        variant: &EnumVariantData,
        collector: &mut PatternInfo,
    ) {
        collector.checks.push(Check::EnumTag {
            path: path.clone(),
            tag_constant: self.enum_tag_constant(variant.identifier, variant.ty),
        });

        let variant_name = variant
            .identifier
            .split('.')
            .next_back()
            .unwrap_or(variant.identifier);
        for (i, field) in variant.fields.iter().enumerate() {
            let field_name = self
                .facts
                .enum_tuple_field_name(variant.ty, variant_name, i);

            let is_unit = self.facts.is_enum_field_unit(variant.ty, variant_name, i);

            let field_path = if self
                .facts
                .is_enum_field_recursive(variant.ty, variant_name, i)
            {
                path.push(self.facts.field_segment(variant.ty, field_name))
                    .push(PathSegment::Deref)
            } else {
                path.push(self.facts.field_segment(variant.ty, field_name))
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
                self.collect_checks_and_bindings(
                    &field_path,
                    field,
                    &variant.field_types[i],
                    variant.field_layouts.get(i),
                    collector,
                );
            }
        }
    }

    fn collect_struct_checks(
        &self,
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
            self.resolve_struct_child_path(path, pattern, path_ty, collector);
        let variant = match resolution {
            RecordPatternResolution::EnumVariant { variant_name, .. } => {
                Some(variant_name.as_str())
            }
            RecordPatternResolution::Struct { .. } | RecordPatternResolution::Unresolved => None,
        };

        for field in fields {
            let field_path =
                self.compute_struct_field_path(&child_path, field, ty, enum_info.as_ref());
            let field_ty = self
                .lookup_field_ty(ty, variant, &field.name)
                .unwrap_or_else(|| panic!("record pattern field not resolved: {}", field.name));
            let layout = self.field_slot_layout(ty, None, variant, &field.name, &field_ty);
            self.collect_checks_and_bindings(
                &field_path,
                &field.value,
                &field_ty,
                layout.as_ref(),
                collector,
            );
        }
    }

    fn resolve_struct_child_path(
        &self,
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
        let child_path = self
            .interface_assert_child_path(path, ty, path_ty, collector)
            .unwrap_or_else(|| path.clone());
        if enum_info.is_some() {
            collector.checks.push(Check::EnumTag {
                path: child_path.clone(),
                tag_constant: self.enum_tag_constant(identifier, ty),
            });
        }
        (enum_info, child_path)
    }
}

impl EmitFacts<'_> {
    fn exact_switch_value(&self, path_ty: &Type, value: &Literal) -> Option<Literal> {
        let kind = self.underlying_simple_kind(path_ty)?;
        let exact = kind.is_integer() || matches!(kind, SimpleKind::Bool | SimpleKind::String);
        (exact && !matches!(value, Literal::Float { .. })).then(|| value.clone())
    }

    fn pattern_type_args(&self, ty: &Type) -> Vec<Type> {
        match self.peel_alias(ty) {
            Type::Nominal { params, .. } => params,
            _ => vec![],
        }
    }
}
