use crate::EmitFacts;
use crate::lowering::abi::layout::is_go_abi_type;
use crate::output::finish::function_literals::immediate_call;
use crate::target::ir::prelude::option_variant;
use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, StructFieldAssignment, StructSpread};
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, SimpleKind, Type, unqualified_name};

use crate::Planner;
use crate::go_name;
use crate::lookup::facts::queries::is_order_sensitive;
use crate::lowering::abi::layout::ValueLayout;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::ValuePlan;
use crate::target::ir::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, Statement, assign, declare,
    discard,
};
use crate::target::ir::go_expression::Position;
use crate::target::ir::go_expression::{CompositeKey, CompositeLayout, FunctionLiteralLayout};
use crate::target::ir::prelude::{PreludeVariant, prelude_constructor};
use crate::target::ir::values::{CaptureBoundary, ConstantKind, GoExpression, Stability};
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::program::SlotOrigin;
use syntax::program::{AliasKind, CheckedExpression, FieldSelection};

struct SpreadInput<'a> {
    base: &'a Expression,
    base_staged: ValuePlan,
    field_pairs: Vec<(String, GoExpression)>,
}

struct StructCallContext<'a> {
    ty: &'a Type,
    checked: CheckedExpression<'a, 'a>,
    go_type: GoType,
    enum_ctx: Option<EnumCallContext>,
}

struct EnumCallContext {
    enum_id: String,
    variant_name: String,
    tag_constant: GoExpression,
    pointer_fields: HashSet<String>,
}

struct PlannedStructFields {
    setup: Vec<Statement>,
    fields: Vec<StructCallField>,
    positions: Vec<(String, Position)>,
    stability: Stability,
}

struct StructCallField {
    name: String,
    value: GoExpression,
    has_observable_evaluation: bool,
}

impl StructCallField {
    fn into_pair(self) -> (String, GoExpression) {
        (self.name, self.value)
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_struct_call(&mut self, expression: &Expression) -> ValuePlan {
        let Expression::StructCall {
            name,
            field_assignments,
            spread,
            ty,
            ..
        } = expression
        else {
            unreachable!("plan_struct_call called with a non-StructCall expression")
        };
        let checked = self.facts.checked(expression);
        let ctx = self.analyze_struct_call(name, ty, checked);

        let is_go_struct = is_go_abi_type(&self.facts, ty);
        let PlannedStructFields {
            mut setup,
            fields,
            positions,
            stability: fields_stability,
        } = self.planned_struct_fields(field_assignments, spread, ty, &ctx);

        let built_from_fields = !matches!(spread, StructSpread::From(_));
        let value = match spread {
            StructSpread::From(base) => self.plan_struct_spread(&mut setup, base, fields, &ctx),
            StructSpread::Autofill { .. } => match self.go_imported_newtype_zero(ty) {
                Some(zero) => zero,
                None => {
                    let mut field_pairs =
                        fields.into_iter().map(StructCallField::into_pair).collect();
                    self.append_autofills(&mut field_pairs, &ctx, is_go_struct);
                    plan_struct_literal(&ctx.go_type, field_pairs)
                }
            },
            StructSpread::None => {
                let field_pairs: Vec<_> =
                    fields.into_iter().map(StructCallField::into_pair).collect();
                plan_struct_literal(&ctx.go_type, field_pairs)
            }
        };

        let value = value.position_elements(|_, element| match &element.key {
            Some(CompositeKey::Field(key)) => positions
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, position)| position.clone()),
            _ => None,
        });
        if built_from_fields {
            return ValuePlan::built_from(setup, value, fields_stability);
        }
        ValuePlan::computed(setup, value)
    }

    fn planned_struct_fields(
        &mut self,
        field_assignments: &[StructFieldAssignment],
        spread: &StructSpread,
        ty: &Type,
        ctx: &StructCallContext<'_>,
    ) -> PlannedStructFields {
        let is_go_struct = is_go_abi_type(&self.facts, ty);

        let kept_fields: Vec<(&StructFieldAssignment, Type, Option<ValueLayout>)> =
            field_assignments
                .iter()
                .filter_map(|f| {
                    let field_ty = self.facts.checked(&f.value).value_use().slot.clone();
                    if omits_none_field(&self.facts, f, &field_ty, ctx, spread, is_go_struct) {
                        return None;
                    }
                    let layout = self.field_slot_layout(
                        ty,
                        Some(ctx.checked.literal_field(&f.name)),
                        &f.name,
                        &field_ty,
                    );
                    Some((f, field_ty, layout))
                })
                .collect();
        let mut literal_slots = vec![false; kept_fields.len()];
        let (stages, source_layouts): (Vec<ValuePlan>, Vec<ValueLayout>) = kept_fields
            .iter()
            .zip(&mut literal_slots)
            .map(|((f, field_ty, layout), literal_slot)| {
                if let Some(layout) = layout
                    && let Some(literal) =
                        self.try_plan_option_literal_into_layout(&f.value, layout)
                {
                    *literal_slot = true;
                    return (literal, layout.clone());
                }
                let declared = &ctx.checked.literal_field(&f.name).declared_ty;
                let field_ctx = ExpressionContext::value()
                    .with_generic_result_slot(Some(declared), Some(field_ty));
                let source_layout = self.emitted_value_layout(&f.value, field_ctx);
                (
                    self.plan_composite_value(&f.value, field_ctx),
                    source_layout,
                )
            })
            .unzip();
        let field_evaluations: Vec<bool> = stages.iter().map(|value| !value.can_delay()).collect();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "field");
        let sequenced_stability = sequenced.stability;
        let mut setup = sequenced.setup;
        let emitted_values = sequenced.values;

        let mut fields =
            Vec::with_capacity(kept_fields.len() + usize::from(ctx.enum_ctx.is_some()));
        if let Some(enum_ctx) = &ctx.enum_ctx {
            fields.push(StructCallField {
                name: ENUM_TAG_FIELD.to_string(),
                value: enum_ctx.tag_constant.clone(),
                has_observable_evaluation: false,
            });
        }
        let mut positions = Vec::new();
        for (
            (((f, field_ty, field_layout), has_observable_evaluation), (mut value, source_layout)),
            literal_slot,
        ) in kept_fields
            .into_iter()
            .zip(field_evaluations)
            .zip(emitted_values.into_iter().zip(source_layouts))
            .zip(literal_slots)
        {
            let field_name = self.resolve_struct_call_field_name(&f.name, ctx);
            if let Some(position) = self.facts.source_position(&f.name_span) {
                positions.push((field_name.clone(), position));
            }
            value = self.wrap_recursive_enum_field(&mut setup, value, f, ctx);
            value = self.coerce_into_slot(
                &mut setup,
                (&f.value, value),
                &field_ty,
                field_layout
                    .as_ref()
                    .filter(|_| !literal_slot)
                    .map(|target| (&source_layout, target)),
            );
            fields.push(StructCallField {
                name: field_name,
                value,
                has_observable_evaluation,
            });
        }

        PlannedStructFields {
            setup,
            fields,
            positions,
            stability: sequenced_stability,
        }
    }

    fn plan_struct_spread(
        &mut self,
        setup: &mut Vec<Statement>,
        base: &Expression,
        fields: Vec<StructCallField>,
        ctx: &StructCallContext<'_>,
    ) -> GoExpression {
        let base_staged = self.plan_operand(base, ExpressionContext::value());
        let (spread_setup, value) = if ctx.enum_ctx.is_some() {
            let field_pairs = self.hoist_observable_fields(setup, fields);
            self.plan_enum_variant_spread(
                SpreadInput {
                    base,
                    base_staged,
                    field_pairs,
                },
                ctx,
            )
        } else {
            let field_pairs = if base_staged.setup().is_empty() && base_staged.can_delay() {
                fields.into_iter().map(StructCallField::into_pair).collect()
            } else {
                self.hoist_observable_fields(setup, fields)
            };
            self.plan_struct_update(base_staged, &field_pairs)
        };
        setup.extend(spread_setup);
        value
    }

    fn append_autofills(
        &mut self,
        field_pairs: &mut Vec<(String, GoExpression)>,
        ctx: &StructCallContext<'_>,
        is_go_struct: bool,
    ) {
        for (field_name, field_ty) in &ctx.checked.struct_literal().unset_fields {
            let zero = if is_go_struct {
                let selection = ctx.checked.literal_field(field_name);
                match self.go_field_map_zero(ctx.ty, selection, field_name, field_ty) {
                    Some(zero) => zero,
                    None => continue,
                }
            } else {
                let Some(zero) = self.lisette_zero_expression(field_ty, true) else {
                    continue;
                };
                zero
            };
            let go_field_name = self.resolve_struct_call_field_name(field_name, ctx);
            field_pairs.push((go_field_name, zero));
        }
    }

    fn go_field_map_zero(
        &mut self,
        owner: &Type,
        selection: &FieldSelection,
        field: &str,
        field_ty: &Type,
    ) -> Option<GoExpression> {
        let layout = self.field_slot_layout(owner, Some(selection), field, field_ty)?;
        if !layout_is_map(&layout) {
            return None;
        }
        Some(GoExpression::empty_composite(layout.go_type(self)))
    }

    fn go_imported_newtype_zero(&mut self, ty: &Type) -> Option<GoExpression> {
        let Type::Nominal { id, .. } = ty else {
            return None;
        };
        if !go_name::is_go_import(id.as_str()) {
            return None;
        }
        let underlying = self.facts.newtype_underlying(ty)?;
        let go_ty = self.go_type(ty);
        self.go_newtype_zero(go_ty, &underlying)
    }

    fn go_newtype_zero(&mut self, go_ty: GoType, underlying: &Type) -> Option<GoExpression> {
        let layout = self.value_layout(underlying, SlotOrigin::Lisette);
        match layout.underlying() {
            ValueLayout::Map { .. } | ValueLayout::Array { .. } => {
                Some(GoExpression::empty_composite(go_ty))
            }
            ValueLayout::Slice { .. } => Some(GoExpression::conversion(go_ty, GoExpression::nil())),
            ValueLayout::Plain(simple @ Type::Simple(_)) => {
                let inner = self.lisette_zero(simple);
                Some(GoExpression::conversion(go_ty, inner))
            }
            ValueLayout::Plain(nominal @ Type::Nominal { id, .. })
                if go_name::is_go_import(id.as_str())
                    && self
                        .facts
                        .definition(id.as_str())
                        .is_some_and(|definition| {
                            definition.is_newtype()
                                || matches!(definition.body, DefinitionBody::Struct { .. })
                        }) =>
            {
                let inner = self.lisette_zero(nominal);
                Some(GoExpression::conversion(go_ty, inner))
            }
            _ => None,
        }
    }

    fn go_imported_zero(&mut self, ty: &Type, id: &str) -> GoExpression {
        let go_ty = self.go_type(ty);
        let is_struct = matches!(
            self.facts.definition(id).map(|d| &d.body),
            Some(DefinitionBody::Struct { .. })
        );
        let is_opaque = matches!(
            self.facts.definition(id).map(|d| &d.body),
            Some(DefinitionBody::TypeAlias {
                alias: AliasKind::Opaque(_),
                ..
            })
        );
        if !is_struct && !is_opaque {
            return GoExpression::new_zero(go_ty);
        }
        if is_struct && let Some(fields) = self.facts.struct_fields(ty) {
            let mut pairs: Vec<(String, GoExpression)> = Vec::new();
            for (name, selection) in fields {
                let field_ty = selection.field_ty();
                let Some(zero) = self.go_field_map_zero(ty, &selection, &name, &field_ty) else {
                    continue;
                };
                pairs.push((self.facts.struct_field_go_name(ty, &name, false), zero));
            }
            return plan_struct_literal(&go_ty, pairs);
        }
        GoExpression::empty_composite(go_ty)
    }

    pub(in crate::lowering) fn lisette_zero(&mut self, ty: &Type) -> GoExpression {
        self.lisette_zero_expression(ty, false)
            .expect("required zero expression")
    }

    fn lisette_zero_expression(&mut self, ty: &Type, omit_go_zero: bool) -> Option<GoExpression> {
        if omit_go_zero && self.facts.go_zero_is_lisette_zero(ty) {
            return None;
        }
        let layout = self.value_layout(ty, SlotOrigin::Lisette);
        let zero = match (ty, &layout) {
            (Type::Simple(kind), _) => {
                let zero = match kind {
                    SimpleKind::Bool => {
                        GoExpression::constant("false".to_string(), ConstantKind::Bool)
                    }
                    SimpleKind::String => {
                        GoExpression::constant("\"\"".to_string(), ConstantKind::String)
                    }
                    SimpleKind::Unit => self.unit_value(None, None),
                    _ => GoExpression::constant("0".to_string(), ConstantKind::Int),
                };
                return Some(zero);
            }
            (
                Type::Compound {
                    kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice,
                    ..
                },
                ValueLayout::Slice { element, .. },
            ) => {
                let element = element.go_type(self);
                let go_type = GoType::slice_of(&element);
                GoExpression::conversion(go_type, GoExpression::nil())
            }
            (
                Type::Compound {
                    kind: CompoundKind::Map,
                    ..
                },
                ValueLayout::Map { key, value, .. },
            ) => {
                let key = key.go_type(self);
                let value = value.go_type(self);
                GoExpression::empty_composite(GoType::map_of(&key, &value))
            }
            (Type::Compound { .. }, _) => GoExpression::empty_composite(self.go_type(ty)),
            (Type::Nominal { id, params, .. }, _) => {
                return self.lisette_zero_nominal(ty, id.as_str(), params, omit_go_zero);
            }
            (Type::Tuple(slots), ValueLayout::Tuple { elements, .. }) => {
                let parts: Vec<GoExpression> = elements
                    .iter()
                    .map(|element| self.lisette_zero(element.logical_type()))
                    .collect();
                let callee = self.make_tuple_callee(slots, parts.len());
                GoExpression::pure_call(callee, parts)
            }
            (
                Type::Array { .. },
                ValueLayout::Array {
                    length, element, ..
                },
            ) => self.array_zero(*length, element.logical_type()),
            (Type::Parameter(name), _) => {
                let go_name = self.generic_go_name(name);
                GoExpression::new_zero(GoType::new(go_name.to_string()))
            }
            _ => GoExpression::empty_composite(self.go_type(ty)),
        };
        Some(zero)
    }

    fn lisette_zero_nominal(
        &mut self,
        ty: &Type,
        id: &str,
        params: &[Type],
        omit_go_zero: bool,
    ) -> Option<GoExpression> {
        if id == "prelude.Option" {
            let inner = params
                .first()
                .map(|a| self.go_type(a))
                .unwrap_or_else(|| GoType::new("any"));
            let zero = GoExpression::call(
                GoExpression::instantiation(
                    GoExpression::generated(
                        GeneratedPackage::Prelude,
                        option_variant("None").constructor,
                    ),
                    GoType::type_arguments(&[inner]),
                ),
                Vec::new(),
            );
            return Some(zero);
        }
        let go_import = go_name::is_go_import(id);
        if go_import
            && (self.facts.is_interface(ty) || self.facts.resolve_to_function_type(ty).is_some())
        {
            return Some(self.go_zero(ty));
        }
        if let Some(underlying) = self.facts.newtype_underlying(ty) {
            let go_ty = self.go_type(ty);
            if go_import {
                let zero = self
                    .go_newtype_zero(go_ty.clone(), &underlying)
                    .unwrap_or_else(|| GoExpression::new_zero(go_ty));
                return Some(zero);
            }
            let inner = self.lisette_zero(&underlying);
            return Some(GoExpression::conversion(go_ty, inner));
        }
        if go_import {
            return Some(self.go_imported_zero(ty, id));
        }
        if let Some(fields) = self.facts.struct_fields(ty) {
            let is_tuple = self.facts.is_tuple_struct_type(ty);
            let pairs: Vec<(String, GoExpression)> = fields
                .into_iter()
                .enumerate()
                .map(|(index, (name, selection))| (index, name, selection.field_ty()))
                .filter_map(|(index, name, field_ty)| {
                    let zero = self.lisette_zero_expression(&field_ty, !is_tuple)?;
                    let go_name = if is_tuple {
                        go_name::tuple_struct_field(index)
                    } else {
                        self.facts.struct_field_go_name(ty, &name, false)
                    };
                    Some((go_name, zero))
                })
                .collect();
            return Some(plan_struct_literal(&self.go_type(ty), pairs));
        }
        if let Some(underlying) = self.facts.underlying_type(ty) {
            return self.lisette_zero_expression(&underlying, omit_go_zero);
        }
        Some(GoExpression::empty_composite(self.go_type(ty)))
    }

    fn array_zero(&mut self, len: u64, elem: &Type) -> GoExpression {
        let elem_go = self.go_type(elem);
        let array_type = GoType::array_of(len, &elem_go);
        if len == 0 || self.facts.go_zero_is_lisette_zero(elem) {
            return GoExpression::empty_composite(array_type);
        }
        let zero = self.lisette_zero(elem);
        let array_name = self.scope.helper_local("arr", self.names);
        let index_name = self.scope.helper_local("i", self.names);
        let arr = || GoExpression::identifier(array_name.clone());
        let index = || GoExpression::identifier(index_name.clone());
        immediate_call(
            array_type.clone(),
            LoweredBlock {
                statements: vec![
                    declare(array_name.clone(), array_type),
                    LoweredStatement::Loop(LoopPlan {
                        kind: LoopKind::Generated { label: None },
                        header: LoopHeader::Range {
                            key: Some(index_name.clone()),
                            value: None,
                            iterable: arr(),
                        },
                        body: LoweredBlock {
                            statements: vec![assign(GoExpression::index(arr(), index()), zero)],
                        },
                    })
                    .into(),
                    plain_return(arr()),
                ],
            },
            FunctionLiteralLayout::MultiLine,
        )
    }

    fn wrap_recursive_enum_field(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        field: &StructFieldAssignment,
        ctx: &StructCallContext<'_>,
    ) -> GoExpression {
        let needs_pointer = ctx
            .enum_ctx
            .as_ref()
            .is_some_and(|e| e.pointer_fields.contains(field.name.as_str()));
        if !needs_pointer {
            return value;
        }
        let temp = self.hoist_tmp_value_statement(statements, "ptr", value);
        GoExpression::address_of(GoExpression::identifier(temp))
    }

    fn analyze_struct_call<'t>(
        &mut self,
        name: &'t str,
        ty: &'t Type,
        checked: CheckedExpression<'t, 't>,
    ) -> StructCallContext<'t> {
        let via_alias = checked.struct_literal().via_alias;
        let is_prelude = self.facts.is_prelude_nominal(ty);
        let enum_id = self.facts.as_enum(ty);

        let go_type =
            self.compute_struct_call_go_type(name, ty, via_alias, is_prelude, enum_id.is_some());

        let enum_ctx = enum_id.map(|id| self.compute_enum_call_context(name, &id));

        StructCallContext {
            ty,
            checked,
            go_type,
            enum_ctx,
        }
    }

    fn compute_struct_call_go_type(
        &mut self,
        name: &str,
        ty: &Type,
        via_alias: bool,
        is_prelude: bool,
        is_enum: bool,
    ) -> GoType {
        if let Type::Nominal { id, .. } = ty
            && self
                .facts
                .definition(id)
                .is_some_and(|definition| definition.is_anon_struct())
        {
            return self.go_type(ty);
        }

        if !is_prelude {
            let parts: Vec<&str> = name.split('.').collect();
            let qualified = match (is_enum, parts.as_slice()) {
                (true, [package, type_name, _]) | (false, [package, type_name]) => {
                    Some((*package, *type_name))
                }
                _ => None,
            };
            if let Some((package, type_name)) = qualified
                && !self.facts.is_current_package(package)
            {
                let type_args = if via_alias {
                    GoType::default()
                } else if let Type::Nominal { params, .. } = ty {
                    self.format_type_args(params)
                } else {
                    GoType::default()
                };
                let canonical = self.canonical_package(package);
                let member = self.facts.type_member_go_name(&canonical, type_name);
                let package = self.package_use_for_package(&canonical);
                return GoType::named(&member, type_args.arguments(), Some(package));
            }
        }

        self.go_type(ty)
    }

    fn compute_enum_call_context(&mut self, name: &str, enum_id: &str) -> EnumCallContext {
        let variant_name = unqualified_name(name).to_string();

        let tag_constant = self.resolve_variant(name, enum_id);

        let pointer_fields = if let Some(layout) = self.facts.enum_layout(enum_id) {
            if let Some(variant) = layout.variant(&variant_name) {
                variant
                    .fields
                    .iter()
                    .filter(|f| f.is_recursive())
                    .map(|f| f.source_name.clone())
                    .collect()
            } else {
                HashSet::default()
            }
        } else {
            HashSet::default()
        };

        EnumCallContext {
            enum_id: enum_id.to_string(),
            variant_name,
            tag_constant,
            pointer_fields,
        }
    }

    fn resolve_struct_call_field_name(
        &mut self,
        field_name: &str,
        ctx: &StructCallContext<'_>,
    ) -> String {
        if let Some(ref enum_ctx) = ctx.enum_ctx {
            self.facts
                .enum_struct_field_name(&enum_ctx.enum_id, &enum_ctx.variant_name, field_name)
                .unwrap_or_else(|| self.facts.struct_field_go_name(ctx.ty, field_name, true))
        } else {
            self.facts.struct_field_go_name(ctx.ty, field_name, false)
        }
    }

    fn hoist_observable_fields(
        &mut self,
        statements: &mut Vec<Statement>,
        fields: Vec<StructCallField>,
    ) -> Vec<(String, GoExpression)> {
        fields
            .into_iter()
            .map(|field| {
                let has_observable_evaluation = field.has_observable_evaluation;
                let (name, value) = field.into_pair();
                if has_observable_evaluation {
                    let temp = self.hoist_tmp_value_statement(statements, "field", value);
                    (name, GoExpression::identifier(temp))
                } else {
                    (name, value)
                }
            })
            .collect()
    }

    fn plan_struct_update(
        &mut self,
        base_staged: ValuePlan,
        fields: &[(String, GoExpression)],
    ) -> (Vec<Statement>, GoExpression) {
        if fields.is_empty() {
            return base_staged.into_parts();
        }

        let (mut statements, base_value) = base_staged.into_parts();
        let tmp = self.hoist_tmp_value_statement(&mut statements, "copy", base_value);
        if let Some(id) = tmp.id() {
            self.scope.mark_struct_update_copy(id);
        }

        for (name, value) in fields {
            statements.push(assign(
                GoExpression::selector(GoExpression::identifier(tmp.clone()), name.clone()),
                value.clone(),
            ));
        }

        (statements, GoExpression::identifier(tmp))
    }

    fn plan_enum_variant_spread(
        &mut self,
        input: SpreadInput<'_>,
        ctx: &StructCallContext<'_>,
    ) -> (Vec<Statement>, GoExpression) {
        let SpreadInput {
            base,
            base_staged,
            mut field_pairs,
        } = input;
        let carried = &ctx.checked.struct_literal().unset_fields;

        let (mut statements, base_value) = base_staged.into_parts();

        if carried.is_empty() {
            statements.push(discard(base_value));
            return (statements, plan_struct_literal(&ctx.go_type, field_pairs));
        }

        let source = if is_order_sensitive(base) {
            GoExpression::identifier(self.hoist_tmp_value_statement(
                &mut statements,
                "spread",
                base_value,
            ))
        } else {
            base_value
        };
        for (field_name, _) in carried {
            let slot = self.resolve_struct_call_field_name(field_name, ctx);
            field_pairs.push((slot.clone(), GoExpression::selector(source.clone(), slot)));
        }
        (statements, plan_struct_literal(&ctx.go_type, field_pairs))
    }
}

fn layout_is_map(layout: &ValueLayout) -> bool {
    match layout {
        ValueLayout::Map { .. } => true,
        ValueLayout::Named { underlying, .. } => layout_is_map(underlying),
        _ => false,
    }
}

pub(in crate::lowering) fn plan_struct_literal(
    ty: &GoType,
    fields: Vec<(String, GoExpression)>,
) -> GoExpression {
    let layout = if fields.len() > 1 {
        CompositeLayout::MultiLine { indented: false }
    } else {
        CompositeLayout::Inline { padded: true }
    };
    GoExpression::struct_literal(ty.clone(), fields, layout)
}

fn omits_none_field(
    facts: &EmitFacts<'_>,
    field: &StructFieldAssignment,
    field_ty: &Type,
    ctx: &StructCallContext<'_>,
    spread: &StructSpread,
    is_go_struct: bool,
) -> bool {
    !is_go_struct
        && ctx.enum_ctx.is_none()
        && matches!(spread, StructSpread::None | StructSpread::Autofill { .. })
        && prelude_constructor(&field.value) == Some(PreludeVariant::None)
        && facts.go_zero_is_lisette_zero(field_ty)
}
