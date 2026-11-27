use super::*;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::member_slot::MemberSlot;
use crate::lookup::facts::queries::go_name_for_binding;
use crate::lookup::facts::queries::go_name_for_rest_binding;
use crate::lowering::abi::go_result_wrapping::option_nil_guard;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::names::go_name::GoExport;
use syntax::ast::Span;
use syntax::program::FieldSelection;
use syntax::types::CompoundKind;
use syntax::types::Symbol;

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

pub(super) struct Narrowed {
    path: ConcretePath,
    ty: Type,
}

fn detect_enum_info(resolution: &RecordPatternResolution) -> Option<(Symbol, String)> {
    match resolution {
        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
            ..
        } => Some((
            enum_name.clone(),
            unqualified_name(variant_name).to_string(),
        )),
        RecordPatternResolution::Struct { .. } | RecordPatternResolution::Unresolved => None,
    }
}

fn slice_length_check(path: &ConcretePath, slice: SlicePattern<'_>) -> Option<Check> {
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
        place: &Place,
        pattern: &Pattern,
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) -> Option<Narrowed> {
        let narrowed = match pattern {
            Pattern::WildCard { .. } | Pattern::Unit { .. } | Pattern::Identifier { .. } => None,

            Pattern::Literal { literal, .. } => {
                collector.checks.push(place.literal_check(
                    GoExpression::literal(pattern_literal(literal)),
                    exact_switch_value(&self.facts, path_ty, literal),
                ));
                None
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
                self.collect_enum_variant_checks(place, variant, path_ty, path_layout, collector)
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
                self.collect_struct_checks(place, pattern, collector)
            }

            Pattern::Tuple { elements, .. } => {
                self.collect_tuple_checks(place, elements, path_ty, path_layout, collector);
                None
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
                self.collect_slice_checks(place, slice, path_ty, path_layout, collector)
            }

            Pattern::Or { patterns, .. } => {
                self.collect_or_pattern_checks(place, patterns, path_ty, path_layout, collector);
                None
            }

            Pattern::AsBinding { pattern: inner, .. } => {
                self.collect_checks_and_bindings(place, inner, path_ty, path_layout, collector)
            }
        };

        if let Pattern::Identifier {
            identifier: name,
            binding,
            ..
        }
        | Pattern::AsBinding { name, binding, .. } = pattern
        {
            let narrowed_layout;
            let (path, ty, layout) = match &narrowed {
                Some(Narrowed { path, ty }) => {
                    narrowed_layout = self.facts.value_layout(ty, SlotOrigin::Lisette);
                    (path.path().clone(), ty, Some(&narrowed_layout))
                }
                None => (place.path().clone(), path_ty, path_layout),
            };
            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name_for_binding(pattern), ty),
                path,
                coercion: self.binding_coercion(ty, layout),
            });
        }
        narrowed
    }

    fn child_place(&self, path: AccessPath, ty: &Type) -> Place {
        Place::new(&self.facts, path, ty)
    }

    fn narrow(&self, place: &Place, ty: &Type, collector: &mut PatternInfo) -> ConcretePath {
        place.concrete(|| self.queries().go_type(ty), &mut collector.checks)
    }

    fn narrowed(place: &Place, path: &ConcretePath, ty: &Type) -> Option<Narrowed> {
        place.is_interface().then(|| Narrowed {
            path: path.clone(),
            ty: ty.clone(),
        })
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
        place: &Place,
        elements: &[Pattern],
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) {
        let element_tys = self
            .facts
            .receiver_tuple_elements(path_ty)
            .unwrap_or_else(|| unreachable!("tuple pattern on a non-tuple type: {path_ty:?}"));
        let path = self.narrow(place, path_ty, collector);

        for (i, element) in elements.iter().enumerate() {
            let field_path = path.tuple_element(&self.facts, path_ty, i);
            self.collect_checks_and_bindings(
                &self.child_place(field_path, &element_tys[i]),
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
        place: &Place,
        slice: SlicePattern<'_>,
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) -> Option<Narrowed> {
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

        let slice_ty = match array_length {
            Some(length) => Type::Array {
                length,
                element: Box::new(element_type.clone()),
            },
            None => Type::compound(CompoundKind::Slice, vec![element_type.clone()]),
        };
        let path = self.narrow(place, &slice_ty, collector);
        if array_length.is_none() {
            collector.checks.extend(slice_length_check(&path, slice));
        }

        for (i, element) in prefix.iter().enumerate() {
            let element_path = path.index(i);
            self.collect_checks_and_bindings(
                &self.child_place(element_path, element_type),
                element,
                element_type,
                path_layout.and_then(ValueLayout::element),
                collector,
            );
        }

        if let RestPattern::Bind { name, binding, .. } = rest {
            let go_name = go_name_for_rest_binding(rest);
            let (rest_path, rest_ty, rest_layout) = match array_length {
                Some(length) => {
                    self.array_rest_parts(&path, prefix.len(), length, element_type, path_layout)
                }
                None => (
                    path.slice_from(prefix.len()),
                    path_ty.clone(),
                    path_layout.cloned(),
                ),
            };

            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name, &rest_ty),
                path: rest_path,
                coercion: self.binding_coercion(&rest_ty, rest_layout.as_ref()),
            });
        }
        Self::narrowed(place, &path, &slice_ty)
    }

    fn array_rest_parts(
        &self,
        path: &ConcretePath,
        offset: usize,
        length: u64,
        element_type: &Type,
        path_layout: Option<&ValueLayout>,
    ) -> (AccessPath, Type, Option<ValueLayout>) {
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
        (path.array_slice_from(offset, go_type), sub_ty, sub_layout)
    }

    fn collect_or_pattern_checks(
        &self,
        place: &Place,
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
                    place,
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
        place: &Place,
        variant: EnumVariantPattern<'_>,
        path_ty: &Type,
        path_layout: Option<&ValueLayout>,
        collector: &mut PatternInfo,
    ) -> Option<Narrowed> {
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
                    .collect_const_pattern_check(place, qualified_name, None, collector);
                return None;
            }
            ConstructorPatternResolution::ConstValue {
                qualified_name,
                value,
            } => {
                let value = const_switch_value(&self.facts, path_ty, value);
                self.queries()
                    .collect_const_pattern_check(place, qualified_name, value, collector);
                return None;
            }
            ConstructorPatternResolution::EnumVariant {
                enum_name,
                variant_name,
            } => (enum_name, variant_name),
            ConstructorPatternResolution::Unresolved => return None,
        };

        if let Some((guard, payload, through_pointer)) =
            path_layout.and_then(|layout| self.nilable_option(layout))
        {
            let path = self.narrow(place, ty, collector);
            let payload_path = if through_pointer {
                path.deref()
            } else {
                path.path().clone()
            };
            let nilable = NilableOption {
                guard,
                payload,
                payload_path,
            };
            let variant = OptionVariantPattern {
                variant_name,
                fields,
                ty,
            };
            self.collect_nilable_option_checks(&path, variant, nilable, collector);
            return Self::narrowed(place, &path, ty);
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
        self.collect_variant_data_checks(place, &variant_data, collector)
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
        place: &Place,
        variant: &EnumVariantData,
        collector: &mut PatternInfo,
    ) -> Option<Narrowed> {
        let EnumVariantData { identifier, ty, .. } = *variant;
        let is_tuple_struct = self.facts.is_tuple_struct_type(ty);
        if !is_tuple_struct && self.handle_foreign_variant_literal(place, ty, identifier, collector)
        {
            return None;
        }

        let path = self.narrow(place, ty, collector);
        if !is_tuple_struct {
            self.collect_tagged_enum_checks(&path, variant, collector);
        } else if self.facts.is_newtype_struct(ty) {
            self.collect_newtype_checks(&path, variant, collector);
        } else {
            self.collect_tuple_struct_checks(&path, variant, collector);
        }
        Self::narrowed(place, &path, ty)
    }

    fn nilable_option<'l>(
        &self,
        layout: &'l ValueLayout,
    ) -> Option<(NilGuard, &'l ValueLayout, bool)> {
        match layout {
            ValueLayout::PointerOption { payload, .. } => Some((NilGuard::Pointer, payload, true)),
            ValueLayout::NullableOption {
                option_type,
                payload,
                ..
            } => Some((option_nil_guard(&self.facts, option_type), payload, false)),
            ValueLayout::Plain(_)
            | ValueLayout::Fallible { .. }
            | ValueLayout::TaggedOption { .. }
            | ValueLayout::Reference { .. }
            | ValueLayout::Slice { .. }
            | ValueLayout::Map { .. }
            | ValueLayout::Array { .. }
            | ValueLayout::Function { .. }
            | ValueLayout::Tuple { .. }
            | ValueLayout::Named { .. } => None,
        }
    }

    fn collect_nilable_option_checks(
        &self,
        path: &ConcretePath,
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
            let payload_ty = ty.ok_type();
            self.collect_checks_and_bindings(
                &self.child_place(payload_path, &payload_ty),
                field,
                &payload_ty,
                Some(payload),
                collector,
            );
        }
    }

    fn handle_foreign_variant_literal(
        &self,
        place: &Place,
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

        collector.checks.push(place.literal_check(go_literal, None));
        true
    }

    fn collect_newtype_checks(
        &self,
        path: &ConcretePath,
        variant: &EnumVariantData,
        collector: &mut PatternInfo,
    ) {
        let Some(underlying_ty) = self.facts.newtype_underlying(variant.ty) else {
            return;
        };
        let go_underlying = self.queries().go_type(&underlying_ty);
        let field_path = path.newtype_cast(go_underlying);
        if let Some(field) = variant.fields.first() {
            self.collect_checks_and_bindings(
                &self.child_place(field_path, &variant.field_types[0]),
                field,
                &variant.field_types[0],
                variant.field_layouts.first(),
                collector,
            );
        }
    }

    fn collect_tuple_struct_checks(
        &self,
        path: &ConcretePath,
        variant: &EnumVariantData,
        collector: &mut PatternInfo,
    ) {
        for (i, field) in variant.fields.iter().enumerate() {
            let field_path = path.raw_field(go_name::tuple_struct_field(i));
            self.collect_checks_and_bindings(
                &self.child_place(field_path, &variant.field_types[i]),
                field,
                &variant.field_types[i],
                variant.field_layouts.get(i),
                collector,
            );
        }
    }

    fn collect_tagged_enum_checks(
        &self,
        path: &ConcretePath,
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
                    &self.child_place(field_path, field_ty),
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
        path: &ConcretePath,
        enum_ty: &Type,
        variant_name: &str,
        index: usize,
    ) -> AccessPath {
        let field_name = self
            .facts
            .enum_tuple_field_name(enum_ty, variant_name, index);
        if self
            .facts
            .is_enum_field_recursive(enum_ty, variant_name, index)
        {
            path.boxed_field(&self.facts, enum_ty, field_name)
        } else {
            path.field(&self.facts, enum_ty, field_name)
        }
    }

    fn collect_struct_checks(
        &self,
        place: &Place,
        pattern: StructPattern<'_>,
        collector: &mut PatternInfo,
    ) -> Option<Narrowed> {
        let StructPattern {
            identifier,
            fields,
            resolution,
            ty,
        } = pattern;

        let enum_info = detect_enum_info(resolution);
        let child_path = self.narrow(place, ty, collector);
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
            let layout = self.facts.field_slot_layout(
                ty,
                Some(selection),
                MemberSlot::Named(&field.name),
                &field_ty,
            );
            self.collect_checks_and_bindings(
                &self.child_place(field_path, &field_ty),
                &field.value,
                &field_ty,
                layout.as_ref(),
                collector,
            );
        }
        Self::narrowed(place, &child_path, ty)
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
        parent_path: &ConcretePath,
        field: &StructFieldPattern,
        ty: &Type,
        enum_info: Option<&(Symbol, String)>,
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
            return parent_path.boxed_field(self.facts, ty, go_field_name);
        }

        parent_path.field(self.facts, ty, go_field_name)
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
        place: &Place,
        qualified_name: &Symbol,
        value: Option<Literal>,
        collector: &mut PatternInfo,
    ) {
        collector
            .checks
            .push(place.literal_check(self.definition_reference(qualified_name), value));
    }
}
