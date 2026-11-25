use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::emit_facts::ref_stripped_type_parameter;
use crate::lookup::facts::value_layout::{FunctionLayout, ValueLayout};
use crate::lookup::state::bindings::BindingValue;
use crate::lowering::abi::coercion_plan::{CoercionPlan, LayoutChange};
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::expressions::access::dot_access_site::DotAccessSite;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::Statement;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{GoExpression, Stability};
use crate::target::names::go_name::{self, GoExport, MemberSpelling};
use crate::target::names::packages::PackageUse;
use crate::target::types::return_abi::CallableReturnAbi;
use syntax::ast::Expression;
use syntax::constant;
use syntax::program::{DefinitionBody, DotAccessResolution, ReceiverCoercion};
use syntax::program::{SlotOrigin, is_go_import};
use syntax::types::Type;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NullableField {
    WrapInOption,
    Raw,
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_dot_access(
        &mut self,
        dot_access: DotAccessSite<'_>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        self.plan_dot_access_with(dot_access, ctx, NullableField::WrapInOption)
    }

    pub(in crate::lowering) fn plan_raw_nullable_field(
        &mut self,
        dot_access: DotAccessSite<'_>,
    ) -> ValuePlan {
        self.plan_dot_access_with(dot_access, ExpressionContext::value(), NullableField::Raw)
    }

    fn plan_dot_access_with(
        &mut self,
        site: DotAccessSite<'_>,
        ctx: ExpressionContext<'_>,
        nullable: NullableField,
    ) -> ValuePlan {
        let dot_access = site.expression;
        let expression = site.receiver;
        let member = site.member;
        let resolution = site.resolution;

        let spelling = self.queries().member_spelling(dot_access);

        if let Some(plan) = self.plan_pre_receiver_dot(site, spelling, ctx) {
            return plan;
        }

        let expression_ty = expression.get_type();
        if let Some(component) = self.queries().tuple_component_read(expression, member) {
            return self.plan_tuple_component_read(site, component, nullable, ctx);
        }

        let (package, base_plan) = self.plan_member_base(site, &expression_ty, ctx);
        let stability =
            dot_member_stability(&self.facts, site, &expression_ty, base_plan.stability());

        let (mut setup, base) = base_plan.into_parts();
        let base = self.plan_bound_dispatch_receiver(&mut setup, dot_access, base, &expression_ty);
        if let Some(member_access) =
            self.queries()
                .try_plan_tuple_member_dot(&base, &expression_ty, member, resolution)
        {
            if let Some(wrapped) = self.try_plan_nullable_field_access(
                &mut setup,
                site,
                member_access.clone(),
                nullable,
                ctx,
            ) {
                return ValuePlan::computed(setup, wrapped);
            }
            return ValuePlan::computed(setup, member_access).with_stability(stability);
        }

        let selector = self.member_selector(site, package, base, &expression_ty, spelling);

        if let Some(wrapped) =
            self.try_plan_nullable_field_access(&mut setup, site, selector.clone(), nullable, ctx)
        {
            return ValuePlan::computed(setup, wrapped);
        }
        let expression =
            self.finish_member_selector(&mut setup, site, &expression_ty, selector, ctx);
        ValuePlan::computed(setup, expression).with_stability(stability)
    }

    fn finish_member_selector(
        &mut self,
        setup: &mut Vec<Statement>,
        site: DotAccessSite<'_>,
        expression_ty: &Type,
        selector: GoExpression,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let expression = self.queries().append_cross_package_type_args(
            selector,
            site.expression,
            expression_ty,
            ctx,
        );
        let expression = if let Some(package) = expression_ty.as_import_namespace()
            && go_constant_exceeds_int(&self.facts, package, site.member, site.ty)
        {
            GoExpression::conversion(self.queries().go_type(site.ty), expression)
        } else {
            expression
        };

        match self
            .queries()
            .prelude_receiver_method_layouts(site.expression)
        {
            Some((source, target)) => {
                self.plan_function_layout_change(setup, expression, &source, &target)
            }
            None => expression,
        }
    }

    fn member_selector(
        &self,
        site: DotAccessSite<'_>,
        package: Option<PackageUse>,
        base: GoExpression,
        expression_ty: &Type,
        spelling: MemberSpelling,
    ) -> GoExpression {
        let field = self.facts.dot_member_go_name(
            site.receiver,
            expression_ty,
            site.member,
            site.resolution,
            spelling,
        );
        match package {
            Some(package) => GoExpression::qualified(package, field),
            None if site.resolution.is_field_read() => {
                self.facts.field_access(base, expression_ty, field)
            }
            None => GoExpression::selector(base, field),
        }
    }

    fn plan_pre_receiver_dot(
        &self,
        site: DotAccessSite<'_>,
        spelling: MemberSpelling,
        ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        let expression =
            self.queries()
                .try_plan_pre_receiver_dot(site.expression, spelling, ctx)?;
        let plan = ValuePlan::computed(Vec::new(), expression);
        if matches!(site.resolution, DotAccessResolution::EnumVariant { .. }) {
            return Some(plan.with_stability(Stability::Fixed));
        }
        Some(plan)
    }

    fn plan_member_base(
        &mut self,
        site: DotAccessSite<'_>,
        expression_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> (Option<PackageUse>, ValuePlan) {
        let package = expression_ty
            .as_import_namespace()
            .map(|package| self.queries().package_use_for_package(package));
        let base_plan = match &package {
            Some(package) => ValuePlan::captured(
                Vec::new(),
                GoIdentifier::external(package.qualifier().to_string()),
            ),
            None => {
                let coercion = self.facts.dot_receiver_coercion(site.receiver, site.member);
                self.plan_coerced_expression(site.receiver, coercion, ctx)
            }
        };
        (package, base_plan)
    }

    fn plan_tuple_component_read(
        &mut self,
        site: DotAccessSite<'_>,
        component: GoIdentifier,
        nullable: NullableField,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let mut setup = Vec::new();
        if let Some(wrapped) = self.try_plan_nullable_field_access(
            &mut setup,
            site,
            GoExpression::identifier(component.clone()),
            nullable,
            ctx,
        ) {
            return ValuePlan::computed(setup, wrapped);
        }
        ValuePlan::captured(Vec::new(), component)
    }

    fn plan_bound_dispatch_receiver(
        &mut self,
        setup: &mut Vec<Statement>,
        dot_access: &Expression,
        base: GoExpression,
        expression_ty: &Type,
    ) -> GoExpression {
        let Some((bound, _)) = self.queries().bound_dispatch_requirement(dot_access) else {
            return base;
        };
        let receiver_type = ref_stripped_type_parameter(expression_ty)
            .map(Type::Parameter)
            .expect("a dispatched bound has a type parameter receiver");
        let adapter = self
            .queries()
            .dynamic_interface_adapter(&receiver_type, &bound, None)
            .expect("a dispatched bound has an adapter");
        let (adapter_setup, adapted) = CoercionPlan::WrapAsInterface(adapter).lower(self, base);
        setup.extend(adapter_setup);
        adapted
    }

    fn try_plan_nullable_field_access(
        &mut self,
        setup: &mut Vec<Statement>,
        site: DotAccessSite<'_>,
        value: GoExpression,
        nullable: NullableField,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        if nullable == NullableField::Raw {
            return None;
        }
        let dot_access = site.expression;
        let expression = site.receiver;
        let member = site.member;
        let result_ty = site.ty;

        let expression_ty = &expression.get_type();
        let callee = ctx.is_callee();
        let source_layout = self.facts.field_slot_layout(
            expression_ty,
            self.facts.checked(dot_access).field_selection(),
            member,
            result_ty,
        )?;

        let target_layout = self.facts.value_layout(result_ty, SlotOrigin::Lisette);
        if callee
            && let (
                ValueLayout::Function { layout: source, .. },
                ValueLayout::Function { layout: target, .. },
            ) = (&source_layout, &target_layout)
            && source.return_abi() == CallableReturnAbi::Value
            && target.return_abi() == CallableReturnAbi::Void
            && source.same_parameter_representation(target)
        {
            return None;
        }
        let coercion = CoercionPlan::change(&self.queries(), &source_layout, &target_layout);
        if coercion.is_identity() {
            return None;
        }
        let raw_var = self.hoist_temp(setup, "raw", value);
        let (coercion_setup, coerced) = coercion.lower(self, GoExpression::identifier(raw_var));
        setup.extend(coercion_setup);
        Some(coerced)
    }

    fn plan_coerced_expression(
        &mut self,
        expression: &Expression,
        coercion: Option<ReceiverCoercion>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let elided_deref = expression
            .deref_inner()
            .filter(|inner| !self.facts.points_to_ref(&inner.get_type()));
        let (operand, had_explicit_deref) = if let Some(inner) = elided_deref {
            (self.plan_operand(inner, ctx), true)
        } else {
            (self.plan_operand(expression, ctx), false)
        };
        let receiver = elided_deref.unwrap_or(expression);
        let needs_deref = self.facts.is_ref_to_type_parameter(&receiver.get_type());
        operand.map_expression(|setup, base| {
            if needs_deref {
                return GoExpression::dereference(base);
            }
            if coercion != Some(ReceiverCoercion::AutoAddress) || had_explicit_deref {
                return base;
            }
            match expression {
                Expression::Call { .. } => {
                    GoExpression::identifier(self.hoist_temp(setup, "ref", base))
                }
                Expression::StructCall { .. } => GoExpression::address_of(base),
                _ => base,
            }
        })
    }
}

fn dot_member_stability(
    facts: &EmitFacts<'_>,
    site: DotAccessSite<'_>,
    expression_ty: &Type,
    base_stability: Stability,
) -> Stability {
    let expression = site.receiver;
    let member = site.member;
    let resolution = site.resolution;

    if let Some(package) = expression_ty.as_import_namespace()
        && package_member_is_fixed(facts, package, member)
    {
        Stability::Fixed
    } else if facts.reads_value_member(resolution, expression, expression_ty) {
        base_stability
    } else {
        Stability::Observable
    }
}

fn go_constant_exceeds_int(facts: &EmitFacts<'_>, package: &str, member: &str, ty: &Type) -> bool {
    if !is_go_import(package) {
        return false;
    }
    let qualified_name = format!("{}.{}", package, member);
    facts
        .definition(qualified_name.as_str())
        .and_then(|definition| definition.const_value()?.value())
        .is_some_and(|value| constant::converts_go_constant(&value, ty))
}

fn package_member_is_fixed(facts: &EmitFacts<'_>, package: &str, member: &str) -> bool {
    let qualified_name = format!("{}.{}", package, member);
    facts
        .definition(qualified_name.as_str())
        .is_some_and(|definition| {
            matches!(definition.body, DefinitionBody::Value { .. }) && !definition.is_variable()
        })
}

impl PlannerQueries<'_, '_> {
    fn prelude_receiver_method_layouts(
        &self,
        expression: &Expression,
    ) -> Option<(FunctionLayout, FunctionLayout)> {
        let Expression::DotAccess {
            expression: receiver,
            resolution: DotAccessResolution::InstanceMethod { .. },
            ..
        } = expression
        else {
            return None;
        };

        let callee = self.resolve_callable_value(expression)?;
        if !callee.is_prelude_dispatch {
            return None;
        }
        self.facts.prelude_receiver_method_layouts(
            &receiver.get_type(),
            callee.declared_type()?.as_function_type()?,
            callee.instantiated.as_function_type()?,
            callee.function_layout(self.facts),
        )
    }

    fn tuple_component_read(&self, expression: &Expression, member: &str) -> Option<GoIdentifier> {
        let Expression::Identifier {
            value, resolution, ..
        } = expression
        else {
            return None;
        };

        let Some(BindingValue::TupleComponents(tuple)) = self
            .scope
            .resolve_identifier_with_resolution(value, resolution)
        else {
            return None;
        };

        let index = member.parse::<usize>().ok()?;
        tuple.names.get(index).cloned()
    }

    pub(in crate::lowering) fn try_plan_tuple_member_dot(
        &self,
        base: &GoExpression,
        expression_ty: &Type,
        member: &str,
        resolution: &DotAccessResolution,
    ) -> Option<GoExpression> {
        let Ok(index) = member.parse::<usize>() else {
            return None;
        };
        match resolution {
            DotAccessResolution::TupleElement => {
                let field = go_name::tuple_field(index).to_string();
                Some(self.facts.field_access(base.clone(), expression_ty, field))
            }
            DotAccessResolution::TupleStructField { is_newtype } => {
                if *is_newtype && let Some(cast) = self.try_plan_newtype_cast(expression_ty, base) {
                    return Some(cast);
                }
                Some(self.facts.field_access(
                    base.clone(),
                    expression_ty,
                    go_name::tuple_struct_field(index),
                ))
            }
            _ => None,
        }
    }

    pub(in crate::lowering) fn nullable_field_guard(
        &self,
        site: DotAccessSite<'_>,
    ) -> Option<NilGuard> {
        let subject = site.expression;
        let expression = site.receiver;
        let member = site.member;
        let result_ty = site.ty;

        let expression_ty = expression.get_type();
        let source_layout = self.facts.field_slot_layout(
            &expression_ty,
            self.facts.checked(subject).field_selection(),
            member,
            result_ty,
        )?;

        let target_layout = self.facts.value_layout(result_ty, SlotOrigin::Lisette);
        let CoercionPlan::Layout(LayoutChange::WrapNullableOption { payload, .. }) =
            CoercionPlan::change(self, &source_layout, &target_layout)
        else {
            return None;
        };
        if payload.changes_layout() {
            return None;
        }
        self.layout_nil_guard(&source_layout)
    }

    fn append_cross_package_type_args(
        &self,
        base_access: GoExpression,
        dot_access: &Expression,
        expression_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        if ctx.is_callee() || expression_ty.as_import_namespace().is_none() {
            return base_access;
        }
        match self.format_value_type_args(dot_access) {
            Some(type_args) => GoExpression::instantiation(base_access, type_args),
            None => base_access,
        }
    }

    fn try_plan_newtype_cast(
        &self,
        expression_ty: &Type,
        base: &GoExpression,
    ) -> Option<GoExpression> {
        let field_ty = self.facts.newtype_underlying(expression_ty)?;
        let go_type = self.go_type(&field_ty);
        let operand = if self.facts.is_reference(expression_ty) {
            GoExpression::dereference(base.clone())
        } else {
            base.clone()
        };
        Some(GoExpression::conversion(go_type, operand))
    }

    fn try_plan_instance_method_value_dot(
        &self,
        member: &str,
        result_ty: &Type,
        export: GoExport,
        spelling: MemberSpelling,
    ) -> Option<GoExpression> {
        let receiver_param = &result_ty.as_function_type()?.params.first()?.ty;
        let go_method =
            self.facts
                .receiver_method_go_name(receiver_param, member, export, spelling);
        Some(self.method_expression(receiver_param, go_method))
    }

    fn try_plan_pre_receiver_dot(
        &self,
        dot_access: &Expression,
        spelling: MemberSpelling,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        let Expression::DotAccess {
            member,
            ty: result_ty,
            resolution,
            ..
        } = dot_access
        else {
            return None;
        };

        match resolution {
            DotAccessResolution::EnumVariant { definition, .. } => {
                self.try_plan_enum_variant_dot(definition, result_ty, ctx)
            }
            DotAccessResolution::StaticMethod { definition, .. } => {
                Some(self.plan_static_method_dot(dot_access, definition, ctx))
            }
            DotAccessResolution::InstanceMethodValue { is_exported, .. } => {
                let export = if *is_exported {
                    GoExport::Exported
                } else {
                    GoExport::Unexported
                };
                self.try_plan_instance_method_value_dot(member, result_ty, export, spelling)
            }
            _ => None,
        }
    }

    fn try_plan_enum_variant_dot(
        &self,
        definition: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        let make_fn = self.variant_make_reference(definition)?;

        match result_ty {
            Type::Function(f) => {
                let Type::Nominal {
                    params: return_type_arguments,
                    ..
                } = f.return_type.as_ref()
                else {
                    return None;
                };

                let type_args = self.constructor_reference_type_arguments(
                    &f.params,
                    return_type_arguments,
                    ctx,
                );
                Some(GoExpression::instantiation(make_fn, type_args))
            }
            Type::Nominal { params, .. } => {
                let type_args = self.variant_value_type_args(params, ctx);
                Some(GoExpression::pure_call(
                    GoExpression::instantiation(make_fn, type_args),
                    Vec::new(),
                ))
            }
            _ => None,
        }
    }

    fn plan_static_method_dot(
        &self,
        dot_access: &Expression,
        definition: &str,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let function = self.definition_reference(definition);
        if ctx.is_callee() {
            return function;
        }
        let type_args = self.format_value_type_args(dot_access);
        GoExpression::instantiation(function, type_args.unwrap_or_default())
    }
}
