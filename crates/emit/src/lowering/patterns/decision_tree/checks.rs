use super::*;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::queries::go_name_for_binding;
use crate::lookup::facts::queries::go_name_for_rest_binding;
use crate::lowering::abi::go_result_wrapping::option_nil_guard;
use crate::lowering::patterns::decision_tree::field_segment;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::names::go_name::GoExport;
use syntax::ast::Span;
use syntax::program::FieldSelection;

struct EnumVariantData<'a> {
    identifier: &'a str,
    enum_id: &'a str,
    fields: &'a [Pattern],
    ty: &'a Type,
    field_types: &'a [Type],
    field_layouts: &'a [ValueLayout],
}

#[derive(Clone, Copy)]
struct EnumVariantPattern<'a> {
    identifier: &'a str,
    fields: &'a [Pattern],
    resolution: &'a ConstructorPatternResolution,
    ty: &'a Type,
    span: Span,
}

#[derive(Clone, Copy)]
struct StructPattern<'a> {
    identifier: &'a str,
    fields: &'a [StructFieldPattern],
    resolution: &'a RecordPatternResolution,
    ty: &'a Type,
}

#[derive(Clone, Copy)]
struct SlicePattern<'a> {
    prefix: &'a [Pattern],
    rest: &'a RestPattern,
    resolution: &'a SequencePatternResolution,
}

struct NilableOption<'a> {
    guard: NilGuard,
    payload: &'a ValueLayout,
    payload_path: AccessPath,
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

fn slice_length_check(path: &AccessPath, slice: SlicePattern<'_>) -> Option<Check> {
    if !slice.rest.is_present() {
        Some(Check::SliceLengthEqual {
            path: path.clone(),
            length: slice.prefix.len(),
        })
    } else if !slice.prefix.is_empty() {
        Some(Check::SliceLengthAtLeast {
            path: path.clone(),
            min_length: slice.prefix.len(),
        })
    } else {
        None
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
            Pattern::WildCard { .. } | Pattern::Unit { .. } | Pattern::Identifier { .. } => {}

            Pattern::Literal { literal, .. } => {
                collector.checks.push(Check::Literal {
                    path: path.clone(),
                    go_literal: GoExpression::literal(pattern_literal(literal)),
                    value: exact_switch_value(&self.facts, path_ty, literal),
                });
            }

            Pattern::EnumVariant {
                identifier,
                fields,
                resolution,
                ty,
                span,
                ..
            } => {
                collector.requires_materialized_subject |= !self.splits_tuple(resolution, ty);
                let variant = EnumVariantPattern {
                    identifier,
                    fields,
                    resolution,
                    ty,
                    span: *span,
                };
                self.collect_enum_variant_checks(path, variant, path_ty, path_layout, collector);
            }

            Pattern::Struct {
                identifier,
                fields,
                resolution,
                ty,
                ..
            } => {
                collector.requires_materialized_subject = true;
                let pattern = StructPattern {
                    identifier,
                    fields,
                    resolution,
                    ty,
                };
                self.collect_struct_checks(path, pattern, path_ty, collector);
            }

            Pattern::Tuple { elements, .. } => {
                self.collect_tuple_checks(path, elements, path_ty, path_layout, collector);
            }

            Pattern::Slice {
                prefix,
                rest,
                resolution,
                ..
            } => {
                collector.requires_materialized_subject = true;
                let slice = SlicePattern {
                    prefix,
                    rest,
                    resolution,
                };
                self.collect_slice_checks(path, slice, path_ty, path_layout, collector);
            }

            Pattern::Or { patterns, .. } => {
                self.collect_or_pattern_checks(path, patterns, path_ty, path_layout, collector);
            }

            Pattern::AsBinding { pattern: inner, .. } => {
                self.collect_checks_and_bindings(path, inner, path_ty, path_layout, collector);
            }
        }

        if let Pattern::Identifier {
            identifier: name,
            binding,
            ..
        }
        | Pattern::AsBinding { name, binding, .. } = pattern
        {
            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name_for_binding(pattern), path_ty),
                path: path.clone(),
                coercion: self.binding_coercion(path_ty, path_layout),
            });
        }
    }

    fn binding_coercion(&self, path_ty: &Type, path_layout: Option<&ValueLayout>) -> CoercionPlan {
        path_layout.map_or(CoercionPlan::Identity, |source| {
            CoercionPlan::change(
                &self.queries(),
                source,
                &self.facts.value_layout(path_ty, SlotOrigin::Lisette),
            )
        })
    }

    fn splits_tuple(&self, resolution: &ConstructorPatternResolution, ty: &Type) -> bool {
        match resolution {
            ConstructorPatternResolution::Const { .. }
            | ConstructorPatternResolution::ConstValue { .. } => true,
            ConstructorPatternResolution::EnumVariant { .. } => {
                !self.facts.is_tuple_struct_type(ty)
            }
            ConstructorPatternResolution::Unresolved => false,
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
        let element_tys = self
            .facts
            .receiver_tuple_elements(path_ty)
            .unwrap_or_else(|| unreachable!("tuple pattern on a non-tuple type: {path_ty:?}"));

        for (i, element) in elements.iter().enumerate() {
            let field_name = go_name::tuple_field(i);
            let field_path = path.push(field_segment(&self.facts, path_ty, field_name.to_string()));
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
        slice: SlicePattern<'_>,
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) {
        let SlicePattern {
            prefix,
            rest,
            resolution,
        } = slice;
        let (element_type, array_length) = match resolution {
            SequencePatternResolution::Slice { element_type } => (element_type, None),
            SequencePatternResolution::Array {
                element_type,
                length,
            } => (element_type, Some(*length)),
            SequencePatternResolution::Unresolved => {
                unreachable!("slice pattern without a resolved element type")
            }
        };

        if array_length.is_none() {
            collector.checks.extend(slice_length_check(path, slice));
        }

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
            let go_name = go_name_for_rest_binding(rest);
            let (segment, rest_ty, rest_layout) = match array_length {
                Some(length) => {
                    self.array_rest_parts(prefix.len(), length, element_type, path_layout)
                }
                None => (
                    PathSegment::SliceFrom(prefix.len()),
                    path_ty.clone(),
                    path_layout.cloned(),
                ),
            };

            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name, &rest_ty),
                path: path.push(segment),
                coercion: self.binding_coercion(&rest_ty, rest_layout.as_ref()),
            });
        }
    }

    fn array_rest_parts(
        &self,
        offset: usize,
        length: u64,
        element_type: &Type,
        path_layout: Option<&ValueLayout>,
    ) -> (PathSegment, Type, Option<ValueLayout>) {
        let sub_length = length.saturating_sub(offset as u64);
        let sub_ty = Type::Array {
            length: sub_length,
            element: Box::new(element_type.clone()),
        };
        let sub_layout =
            path_layout
                .and_then(ValueLayout::element)
                .map(|element| ValueLayout::Array {
                    array_type: sub_ty.clone(),
                    length: sub_length,
                    element: Box::new(element.clone()),
                });

        let go_type = sub_layout.as_ref().map_or_else(
            || self.queries().go_type(&sub_ty),
            |layout| layout.go_type(&self.queries()),
        );
        (
            PathSegment::ArraySliceFrom { offset, go_type },
            sub_ty,
            sub_layout,
        )
    }

    fn collect_or_pattern_checks(
        &self,
        path: &AccessPath,
        patterns: &[Pattern],
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) {
        assert!(
            !patterns.iter().any(pattern_has_bindings),
            "split or-patterns that bind names with `split_binding_or_patterns` before collection"
        );
        let alternative_collectors: Vec<PatternInfo> = patterns
            .iter()
            .map(|p| {
                let mut alternative_collector = PatternInfo::new();
                self.collect_checks_and_bindings(
                    path,
                    p,
                    path_ty,
                    path_layout,
                    &mut alternative_collector,
                );
                alternative_collector
            })
            .collect();

        if alternative_collectors.iter().any(|c| c.checks.is_empty()) {
            return;
        }

        collector.checks.push(Check::Or {
            alternatives: alternative_collectors
                .into_iter()
                .map(|c| c.checks)
                .collect(),
        });
    }

    fn collect_enum_variant_checks(
        &self,
        path: &AccessPath,
        variant: EnumVariantPattern<'_>,
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) {
        let path_layout = path_layout.map(ValueLayout::underlying);
        let EnumVariantPattern {
            identifier,
            fields,
            resolution,
            ty,
            span,
        } = variant;

        let (enum_name, variant_name) = match resolution {
            ConstructorPatternResolution::Const { qualified_name } => {
                self.queries()
                    .collect_const_pattern_check(path, qualified_name, None, collector);
                return;
            }
            ConstructorPatternResolution::ConstValue {
                qualified_name,
                value,
            } => {
                let value = const_switch_value(&self.facts, path_ty, value);
                self.queries()
                    .collect_const_pattern_check(path, qualified_name, value, collector);
                return;
            }
            ConstructorPatternResolution::EnumVariant {
                enum_name,
                variant_name,
            } => (enum_name, variant_name),
            ConstructorPatternResolution::Unresolved => return,
        };

        if let Some(nilable) = self.nilable_option(path, path_layout) {
            let variant = OptionVariantPattern {
                variant_name,
                fields,
                ty,
            };
            self.collect_nilable_option_checks(path, variant, nilable, collector);
            return;
        }
        let (field_types, field_layouts) = self.variant_payload(span, path_layout, variant_name);
        let variant_data = EnumVariantData {
            identifier,
            enum_id: enum_name,
            fields,
            ty,
            field_types: &field_types,
            field_layouts: &field_layouts,
        };
        self.collect_variant_data_checks(path, &variant_data, path_ty, collector);
    }

    fn variant_payload(
        &self,
        span: Span,
        path_layout: Option<&ValueLayout>,
        variant_name: &str,
    ) -> (Vec<Type>, Vec<ValueLayout>) {
        let selections = self.facts.variant_pattern_fields(span);
        let field_types: Vec<Type> = selections
            .iter()
            .map(|selection| selection.field_ty())
            .collect();
        let field_layouts = self.queries().variant_field_layouts(
            path_layout,
            variant_name,
            &selections,
            &field_types,
        );
        (field_types, field_layouts)
    }

    fn collect_variant_data_checks(
        &self,
        path: &AccessPath,
        variant: &EnumVariantData,
        path_ty: &Type,
        collector: &mut PatternInfo,
    ) {
        let EnumVariantData { identifier, ty, .. } = *variant;
        if self.facts.is_tuple_struct_type(ty) {
            let child_path = self
                .queries()
                .interface_assert_child_path(path, ty, path_ty, collector)
                .unwrap_or_else(|| path.clone());
            if self.facts.is_newtype_struct(ty) {
                self.collect_newtype_checks(&child_path, variant, collector);
            } else {
                self.collect_tuple_struct_checks(&child_path, variant, collector);
            }
            return;
        }

        if self.handle_foreign_variant_literal(path, ty, identifier, collector) {
            return;
        }

        self.collect_tagged_enum_checks(path, variant, collector);
    }

    fn nilable_option<'l>(
        &self,
        path: &AccessPath,
        layout: Option<&'l ValueLayout>,
    ) -> Option<NilableOption<'l>> {
        let (guard, payload, payload_path) = match layout? {
            ValueLayout::PointerOption { payload, .. } => {
                (NilGuard::Pointer, payload, path.push(PathSegment::Deref))
            }
            ValueLayout::NullableOption {
                option_type,
                payload,
                ..
            } => (
                option_nil_guard(&self.facts, option_type),
                payload,
                path.clone(),
            ),
            ValueLayout::Plain(_)
            | ValueLayout::Fallible { .. }
            | ValueLayout::TaggedOption { .. }
            | ValueLayout::Reference { .. }
            | ValueLayout::Slice { .. }
            | ValueLayout::Map { .. }
            | ValueLayout::Array { .. }
            | ValueLayout::Function { .. }
            | ValueLayout::Tuple { .. }
            | ValueLayout::Named { .. } => return None,
        };
        Some(NilableOption {
            guard,
            payload,
            payload_path,
        })
    }

    fn collect_nilable_option_checks(
        &self,
        path: &AccessPath,
        variant: OptionVariantPattern<'_>,
        nilable: NilableOption<'_>,
        collector: &mut PatternInfo,
    ) {
        let OptionVariantPattern {
            variant_name,
            fields,
            ty,
        } = variant;
        let NilableOption {
            guard,
            payload,
            payload_path,
        } = nilable;

        let some = unqualified_name(variant_name) == "Some";
        assert!(
            some || unqualified_name(variant_name) == "None",
            "nilable option patterns require Some or None"
        );
        collector.checks.push(Check::Nilable {
            path: path.clone(),
            guard,
            some,
        });
        if some {
            let [field] = fields else {
                unreachable!("Some pattern has one payload");
            };
            self.collect_checks_and_bindings(
                &payload_path,
                field,
                &ty.ok_type(),
                Some(payload),
                collector,
            );
        }
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
                let package = self.canonical_package(package);
                let member = self.facts.package_member_go_name(&package, member);
                self.queries().package_member_reference(&package, member)
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
        let go_underlying = self.queries().go_type(&underlying_ty);
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
        variant: &EnumVariantData,
        collector: &mut PatternInfo,
    ) {
        for (i, field) in variant.fields.iter().enumerate() {
            let field_path = path.push(PathSegment::Field(go_name::tuple_struct_field(i)));
            self.collect_checks_and_bindings(
                &field_path,
                field,
                &variant.field_types[i],
                variant.field_layouts.get(i),
                collector,
            );
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
            tag_constant: self
                .queries()
                .resolve_variant(variant.identifier, variant.enum_id),
        });

        let variant_name = variant
            .identifier
            .split('.')
            .next_back()
            .unwrap_or(variant.identifier);
        for (i, field) in variant.fields.iter().enumerate() {
            let field_ty = &variant.field_types[i];
            let is_unit = self.facts.resolves_to_unit(field_ty) || field_ty.is_never();
            let field_path = self.enum_field_path(path, variant.ty, variant_name, i);

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
                    field_ty,
                    variant.field_layouts.get(i),
                    collector,
                );
            }
        }
    }

    fn enum_field_path(
        &self,
        path: &AccessPath,
        enum_ty: &Type,
        variant_name: &str,
        index: usize,
    ) -> AccessPath {
        let field_name = self
            .facts
            .enum_tuple_field_name(enum_ty, variant_name, index);
        let field_path = path.push(field_segment(&self.facts, enum_ty, field_name));
        if self
            .facts
            .is_enum_field_recursive(enum_ty, variant_name, index)
        {
            field_path.push(PathSegment::Deref)
        } else {
            field_path
        }
    }

    fn collect_struct_checks(
        &self,
        path: &AccessPath,
        pattern: StructPattern<'_>,
        path_ty: &Type,
        collector: &mut PatternInfo,
    ) {
        let StructPattern {
            identifier,
            fields,
            resolution,
            ty,
        } = pattern;

        let enum_info = detect_enum_info(resolution);
        let child_path = self
            .queries()
            .interface_assert_child_path(path, ty, path_ty, collector)
            .unwrap_or_else(|| path.clone());
        if let Some((enum_id, _)) = &enum_info {
            collector.checks.push(Check::EnumTag {
                path: child_path.clone(),
                tag_constant: self.queries().resolve_variant(identifier, enum_id),
            });
        }
        for field in fields {
            let field_path = self.queries().compute_struct_field_path(
                &child_path,
                field,
                ty,
                enum_info.as_ref(),
            );
            let selection = self.facts.pattern_field(field.name_span);
            let field_ty = selection.field_ty();
            let layout = self
                .facts
                .field_slot_layout(ty, Some(selection), &field.name, &field_ty);
            self.collect_checks_and_bindings(
                &field_path,
                &field.value,
                &field_ty,
                layout.as_ref(),
                collector,
            );
        }
    }
}

#[derive(Clone, Copy)]
struct OptionVariantPattern<'p> {
    variant_name: &'p str,
    fields: &'p [Pattern],
    ty: &'p Type,
}

fn const_switch_value(facts: &EmitFacts<'_>, path_ty: &Type, value: &Literal) -> Option<Literal> {
    if matches!(value, Literal::String { .. }) {
        return None;
    }
    exact_switch_value(facts, path_ty, value)
}

fn exact_switch_value(facts: &EmitFacts<'_>, path_ty: &Type, value: &Literal) -> Option<Literal> {
    let kind = facts.underlying_simple_kind(path_ty)?;
    let exact = kind.is_integer() || matches!(kind, SimpleKind::Bool | SimpleKind::String);
    (exact && !matches!(value, Literal::Float { .. })).then(|| value.clone())
}

impl PlannerQueries<'_, '_> {
    fn compute_struct_field_path(
        &self,
        parent_path: &AccessPath,
        field: &StructFieldPattern,
        ty: &Type,
        enum_info: Option<&(String, String)>,
    ) -> AccessPath {
        let go_field_name = if let Some((enum_id, variant_name)) = enum_info {
            self.facts
                .enum_layout(enum_id)
                .struct_field_name(variant_name, &field.name)
        } else {
            self.facts
                .struct_field_go_name(ty, &field.name, GoExport::Unexported)
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
                .push(field_segment(self.facts, ty, go_field_name))
                .push(PathSegment::Deref);
        }

        parent_path.push(field_segment(self.facts, ty, go_field_name))
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

    fn variant_field_layouts(
        &self,
        path_layout: Option<&ValueLayout>,
        variant_name: &str,
        selections: &[&FieldSelection],
        field_types: &[Type],
    ) -> Vec<ValueLayout> {
        selections
            .iter()
            .zip(field_types)
            .enumerate()
            .map(|(index, (selection, ty))| {
                let inherited = path_layout
                    .and_then(|layout| layout.variant_field(unqualified_name(variant_name), index));
                inherited.cloned().unwrap_or_else(|| {
                    self.facts.value_layout_with_declaration(
                        ty,
                        SlotOrigin::Lisette,
                        &selection.declared_ty,
                    )
                })
            })
            .collect()
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
}
