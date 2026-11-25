use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::expressions::zero_values::ZeroSpelling;
use crate::target::names::go_name::GoExport;
use rustc_hash::FxHashSet;

use syntax::ast::{Expression, StructFieldAssignment, StructSpread};
use syntax::types::{Type, unqualified_name};

use crate::Planner;
use crate::lookup::facts::queries::is_order_sensitive;
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::{Statement, assign, discard};
use crate::target::ir::go_expression::Position;
use crate::target::ir::go_expression::{CompositeKey, CompositeLayout};
use crate::target::ir::prelude::{PreludeVariant, prelude_constructor};
use crate::target::ir::values::{CaptureBoundary, GoExpression, Stability};
use crate::target::types::go_type::GoType;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::program::{CheckedExpression, CheckedStructLiteral};

struct SpreadInput<'a> {
    base: &'a Expression,
    base_operand: ValuePlan,
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
    pointer_fields: FxHashSet<String>,
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

        let is_go_struct = self.facts.is_go_abi_type(ty);
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
            return ValuePlan::new(setup, value, fields_stability);
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
        let is_go_struct = self.facts.is_go_abi_type(ty);

        let kept_fields: Vec<(&StructFieldAssignment, Type, Option<ValueLayout>)> =
            field_assignments
                .iter()
                .filter_map(|f| {
                    let field_ty = self.facts.checked(&f.value).value_use().slot.clone();
                    if omits_none_field(&self.facts, f, &field_ty, ctx, spread, is_go_struct) {
                        return None;
                    }
                    let layout = self.facts.field_slot_layout(
                        ty,
                        Some(ctx.checked.literal_field(&f.name)),
                        &f.name,
                        &field_ty,
                    );

                    Some((f, field_ty, layout))
                })
                .collect();

        let mut literal_slots = vec![false; kept_fields.len()];
        let (operands, source_layouts): (Vec<ValuePlan>, Vec<ValueLayout>) = kept_fields
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

        let field_evaluations: Vec<bool> = operands
            .iter()
            .map(|value| value.must_evaluate_now())
            .collect();
        let sequenced = self.sequence_values(operands, CaptureBoundary::SiblingSequence, "field");
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
        let base_operand = self.plan_operand(base, ExpressionContext::value());
        let (spread_setup, value) = if ctx.enum_ctx.is_some() {
            let field_pairs = self.hoist_observable_fields(setup, fields);
            self.plan_enum_variant_spread(
                SpreadInput {
                    base,
                    base_operand,
                    field_pairs,
                },
                ctx,
            )
        } else {
            let field_pairs = if base_operand.setup().is_empty() && base_operand.can_delay() {
                fields.into_iter().map(StructCallField::into_pair).collect()
            } else {
                self.hoist_observable_fields(setup, fields)
            };
            self.plan_struct_update(base_operand, &field_pairs)
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
                let Some(zero) = self.lisette_zero_expression(field_ty, ZeroSpelling::OmitGoZero)
                else {
                    continue;
                };
                zero
            };

            let go_field_name = self.resolve_struct_call_field_name(field_name, ctx);
            field_pairs.push((go_field_name, zero));
        }
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
        let temp = self.hoist_temp(statements, "ptr", value);
        GoExpression::address_of(GoExpression::identifier(temp))
    }

    fn analyze_struct_call<'t>(
        &mut self,
        name: &'t str,
        ty: &'t Type,
        checked: CheckedExpression<'t, 't>,
    ) -> StructCallContext<'t> {
        let enum_id = self.facts.as_enum(ty);

        let go_type = self.compute_struct_call_go_type(
            name,
            ty,
            checked.struct_literal(),
            enum_id.as_deref(),
        );

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
        literal: &CheckedStructLiteral,
        enum_id: Option<&str>,
    ) -> GoType {
        if let Type::Nominal { id, .. } = ty
            && self
                .facts
                .definition(id)
                .is_some_and(|definition| definition.is_anon_struct())
        {
            return self.go_type(ty);
        }

        if !self.facts.is_prelude_nominal(ty) {
            let parts: Vec<&str> = name.split('.').collect();
            let qualified = match (enum_id, parts.as_slice()) {
                (Some(_), [package, type_name, _]) | (None, [package, type_name]) => {
                    Some((*package, *type_name))
                }
                _ => None,
            };

            if let Some((package, type_name)) = qualified
                && !self.facts.is_current_package(package)
            {
                let type_args = if literal.via_alias {
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
                FxHashSet::default()
            }
        } else {
            FxHashSet::default()
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
                .unwrap_or_else(|| {
                    self.facts
                        .struct_field_go_name(ctx.ty, field_name, GoExport::Exported)
                })
        } else {
            self.facts
                .struct_field_go_name(ctx.ty, field_name, GoExport::Unexported)
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
                    let temp = self.hoist_temp(statements, "field", value);
                    (name, GoExpression::identifier(temp))
                } else {
                    (name, value)
                }
            })
            .collect()
    }

    fn plan_struct_update(
        &mut self,
        base_operand: ValuePlan,
        fields: &[(String, GoExpression)],
    ) -> (Vec<Statement>, GoExpression) {
        if fields.is_empty() {
            return base_operand.into_parts();
        }

        let (mut statements, base_value) = base_operand.into_parts();
        let tmp = self.hoist_temp(&mut statements, "copy", base_value);
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
            base_operand,
            mut field_pairs,
        } = input;
        let carried = &ctx.checked.struct_literal().unset_fields;

        let (mut statements, base_value) = base_operand.into_parts();

        if carried.is_empty() {
            statements.push(discard(base_value));
            return (statements, plan_struct_literal(&ctx.go_type, field_pairs));
        }

        let source = if is_order_sensitive(base) {
            GoExpression::identifier(self.hoist_temp(&mut statements, "spread", base_value))
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
