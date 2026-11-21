use crate::EmitFacts;
use crate::Planner;
use crate::abi::callable::CallableReturnAbi;
use crate::abi::coercion::{CoercionPlan, LayoutBridge};
use crate::abi::layout::ValueLayout;
use crate::abi::wrapping::NilGuard;
use crate::ir::bodies::Statement;
use crate::ir::local::GoIdentifier;
use crate::ir::values::{GoExpression, Stability, ValuePlan};
use crate::names::go_name;
use crate::state::bindings::BindingValue;
use crate::state::expression::ExpressionContext;
use syntax::ast::Expression;
use syntax::program::SlotOrigin;
use syntax::program::{
    ConstantValue, DefinitionBody, DotAccessResolution, ReceiverCoercion, ValueKind,
};
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn plan_dot_access(
        &mut self,
        dot_access: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        self.plan_dot_access_with(dot_access, ctx, true)
    }

    pub(crate) fn plan_raw_nullable_field(&mut self, dot_access: &Expression) -> ValuePlan {
        self.plan_dot_access_with(
            dot_access.unwrap_parens(),
            ExpressionContext::value(),
            false,
        )
    }

    fn plan_dot_access_with(
        &mut self,
        dot_access: &Expression,
        ctx: ExpressionContext<'_>,
        wrap_nullable: bool,
    ) -> ValuePlan {
        let Expression::DotAccess {
            expression,
            member,
            ty: result_ty,
            resolution,
            ..
        } = dot_access
        else {
            unreachable!("plan_dot_access requires a DotAccess expression");
        };
        let receiver_coercion = self.facts.dot_receiver_coercion(expression, member);
        let tagged_twin = self.selects_tagged_twin(dot_access);

        if let Some(expression) = self.try_plan_pre_receiver_dot(dot_access, tagged_twin, ctx) {
            let plan = ValuePlan::computed(Vec::new(), expression);
            if matches!(resolution, DotAccessResolution::EnumVariant { .. }) {
                return plan.with_stability(Stability::Fixed);
            }
            return plan;
        }

        let expression_ty = expression.get_type();
        if let Some(component) = self.tuple_component_read(expression, member) {
            let mut setup = Vec::new();
            if wrap_nullable
                && let Some(wrapped) = self.try_plan_nullable_field_access(
                    &mut setup,
                    dot_access,
                    GoExpression::identifier(component.clone()),
                    ctx,
                )
            {
                return ValuePlan::computed(setup, wrapped);
            }
            return ValuePlan::captured(Vec::new(), component);
        }

        let package = expression_ty
            .as_import_namespace()
            .map(|package| self.package_use_for_package(package));
        let base_plan = match &package {
            Some(package) => ValuePlan::captured(
                Vec::new(),
                GoIdentifier::external(package.qualifier().to_string()),
            ),
            None => self.plan_coerced_expression(expression, receiver_coercion, ctx),
        };
        let stability =
            self.facts
                .dot_member_stability(dot_access, &expression_ty, base_plan.stability());
        let (mut setup, base) = base_plan.into_parts();
        let base = self.plan_bound_dispatch_receiver(&mut setup, dot_access, base, &expression_ty);
        if let Some(member_access) =
            self.try_plan_tuple_member_dot(&base, &expression_ty, member, resolution)
        {
            if wrap_nullable
                && let Some(wrapped) = self.try_plan_nullable_field_access(
                    &mut setup,
                    dot_access,
                    member_access.clone(),
                    ctx,
                )
            {
                return ValuePlan::computed(setup, wrapped);
            }
            return ValuePlan::computed(setup, member_access).with_stability(stability);
        }

        let field =
            self.dot_member_go_name(expression, &expression_ty, member, resolution, tagged_twin);

        let selector = match package {
            Some(package) => GoExpression::qualified(package, field),
            None if resolution.is_field_read() => {
                self.facts.field_access(base, &expression_ty, field)
            }
            None => GoExpression::selector(base, field),
        };
        if wrap_nullable
            && let Some(wrapped) =
                self.try_plan_nullable_field_access(&mut setup, dot_access, selector.clone(), ctx)
        {
            return ValuePlan::computed(setup, wrapped);
        }
        let expression =
            self.append_cross_package_type_args(selector, dot_access, &expression_ty, ctx);
        let expression = if let Some(package) = expression_ty.as_import_namespace()
            && self
                .facts
                .go_constant_exceeds_int(package, member, result_ty)
        {
            GoExpression::conversion(self.go_type(result_ty), expression)
        } else {
            expression
        };
        let expression =
            if let Some((source, target)) = self.prelude_receiver_method_layouts(dot_access) {
                self.plan_function_layout_bridge(&mut setup, expression, &source, &target, false)
            } else {
                expression
            };
        ValuePlan::computed(setup, expression).with_stability(stability)
    }

    fn plan_bound_dispatch_receiver(
        &mut self,
        setup: &mut Vec<Statement>,
        dot_access: &Expression,
        base: GoExpression,
        expression_ty: &Type,
    ) -> GoExpression {
        let Some((bound, _)) = self.bound_dispatch_requirement(dot_access) else {
            return base;
        };
        let receiver_type = self
            .facts
            .type_parameter_behind_refs(expression_ty)
            .map(Type::Parameter)
            .expect("a dispatched bound has a type parameter receiver");
        let adapter = self
            .dynamic_interface_adapter(&receiver_type, &bound, None)
            .expect("a dispatched bound has an adapter");
        let (adapter_setup, adapted) = CoercionPlan::WrapAsInterface(adapter).lower(self, base);
        setup.extend(adapter_setup);
        adapted
    }

    fn try_plan_pre_receiver_dot(
        &mut self,
        dot_access: &Expression,
        tagged_twin: bool,
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
            DotAccessResolution::InstanceMethodValue { is_exported, .. } => self
                .try_plan_instance_method_value_dot(member, result_ty, *is_exported, tagged_twin),
            _ => None,
        }
    }

    fn tuple_component_read(&self, expression: &Expression, member: &str) -> Option<GoIdentifier> {
        let Expression::Identifier {
            value, resolution, ..
        } = expression.unwrap_parens()
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

    pub(crate) fn dot_member_go_name(
        &self,
        expression: &Expression,
        expression_ty: &Type,
        member: &str,
        resolution: &DotAccessResolution,
        tagged_twin: bool,
    ) -> String {
        if let Some(package) = expression_ty.as_import_namespace() {
            return self.facts.package_member_go_name(package, member);
        }
        let canonical_ty = self.member_owner_type(expression_ty);
        if let DotAccessResolution::StructField { is_exported, .. } = resolution {
            return self.struct_field_go_name(&canonical_ty, member, *is_exported);
        }
        if let DotAccessResolution::InstanceMethod { is_exported, .. } = resolution {
            return self.facts.method_selector_go_name(
                canonical_ty.get_qualified_id(),
                member,
                *is_exported,
                tagged_twin,
            );
        }
        let is_exported = self.resolve_is_exported(expression, &canonical_ty, member);
        self.facts
            .member_go_name(&canonical_ty, member, is_exported)
    }

    pub(crate) fn try_plan_tuple_member_dot(
        &mut self,
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

    pub(crate) fn nullable_field_guard(&self, subject: &Expression) -> Option<NilGuard> {
        let Expression::DotAccess {
            expression,
            member,
            ty: result_ty,
            ..
        } = subject.unwrap_parens()
        else {
            return None;
        };
        let expression_ty = expression.get_type();
        let source_layout = self.field_slot_layout(
            &expression_ty,
            self.facts.checked(subject).field_selection(),
            member,
            result_ty,
        )?;
        let target_layout = self.value_layout(result_ty, SlotOrigin::Lisette);
        let CoercionPlan::Layout(LayoutBridge::WrapNullableOption { payload, .. }) =
            CoercionPlan::bridge(self, &source_layout, &target_layout)
        else {
            return None;
        };
        if !payload.is_identity() {
            return None;
        }
        self.layout_nil_guard(&source_layout)
    }

    fn try_plan_nullable_field_access(
        &mut self,
        setup: &mut Vec<Statement>,
        dot_access: &Expression,
        value: GoExpression,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        let Expression::DotAccess {
            expression,
            member,
            ty: result_ty,
            ..
        } = dot_access
        else {
            unreachable!("a nullable field access reads a DotAccess expression");
        };
        let expression_ty = &expression.get_type();
        let callee = ctx.is_callee();
        let source_layout = self.field_slot_layout(
            expression_ty,
            self.facts.checked(dot_access).field_selection(),
            member,
            result_ty,
        )?;
        let target_layout = self.value_layout(result_ty, SlotOrigin::Lisette);
        if callee
            && let (
                ValueLayout::Function { layout: source, .. },
                ValueLayout::Function { layout: target, .. },
            ) = (&source_layout, &target_layout)
            && source.return_abi == CallableReturnAbi::Value
            && target.return_abi == CallableReturnAbi::Void
            && source.same_parameter_representation(target)
        {
            return None;
        }
        let coercion = CoercionPlan::bridge(self, &source_layout, &target_layout);
        if coercion.is_identity() {
            return None;
        }
        let raw_var = self.hoist_tmp_value_statement(setup, "raw", value);
        let (coercion_setup, coerced) = coercion.lower(self, GoExpression::identifier(raw_var));
        setup.extend(coercion_setup);
        Some(coerced)
    }

    fn append_cross_package_type_args(
        &mut self,
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
        &mut self,
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

    fn plan_coerced_expression(
        &mut self,
        expression: &Expression,
        coercion: Option<ReceiverCoercion>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let elided_deref = expression
            .deref_inner()
            .filter(|inner| !self.facts.points_to_ref(&inner.get_type()));
        let (staged, had_explicit_deref) = if let Some(inner) = elided_deref {
            (self.plan_operand(inner, ctx), true)
        } else {
            (self.plan_operand(expression, ctx), false)
        };
        let receiver = elided_deref.unwrap_or(expression);
        let needs_deref = self.facts.is_ref_to_type_parameter(&receiver.get_type());
        staged.map_expression(|setup, base| {
            if needs_deref {
                return GoExpression::dereference(base);
            }
            if coercion != Some(ReceiverCoercion::AutoAddress) || had_explicit_deref {
                return base;
            }
            match expression.unwrap_parens() {
                Expression::Call { .. } => {
                    GoExpression::identifier(self.hoist_tmp_value_statement(setup, "ref", base))
                }
                Expression::StructCall { .. } => GoExpression::address_of(base),
                _ => base,
            }
        })
    }

    fn try_plan_enum_variant_dot(
        &mut self,
        definition: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        let make_fn = self.variant_make_reference(definition)?;

        match result_ty {
            Type::Function(f) => {
                let Type::Nominal {
                    params: ret_params, ..
                } = f.return_type.as_ref()
                else {
                    return None;
                };
                let type_args =
                    self.constructor_reference_type_arguments(&f.params, ret_params, ctx);
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
        &mut self,
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

    fn try_plan_instance_method_value_dot(
        &mut self,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        tagged_twin: bool,
    ) -> Option<GoExpression> {
        let receiver_param = &result_ty.as_function_type()?.params.first()?.ty;
        let owner = self.facts.without_references(receiver_param);
        let go_method = self.facts.method_selector_go_name(
            owner.get_qualified_id(),
            member,
            is_exported,
            tagged_twin,
        );
        Some(self.method_expression(receiver_param, go_method))
    }
}

impl EmitFacts<'_> {
    fn dot_member_stability(
        &self,
        dot_access: &Expression,
        expression_ty: &Type,
        base_stability: Stability,
    ) -> Stability {
        let Expression::DotAccess {
            expression,
            member,
            resolution,
            ..
        } = dot_access
        else {
            unreachable!("a member stability reads a DotAccess expression");
        };
        if let Some(package) = expression_ty.as_import_namespace()
            && self.package_member_is_fixed(package, member)
        {
            Stability::Fixed
        } else if self.reads_value_member(resolution, expression, expression_ty) {
            base_stability
        } else {
            Stability::Observable
        }
    }

    pub(crate) fn dot_receiver_coercion(
        &self,
        receiver: &Expression,
        member: &str,
    ) -> Option<ReceiverCoercion> {
        self.checked(receiver).receiver_change().filter(|coercion| {
            *coercion != ReceiverCoercion::AutoAddress
                || !self.has_go_value_receiver(&receiver.get_type(), member)
        })
    }

    fn go_constant_exceeds_int(&self, package: &str, member: &str, ty: &Type) -> bool {
        if !ty.is_unsigned_int() || !go_name::is_go_import(package) {
            return false;
        }
        let qualified_name = format!("{}.{}", package, member);
        self.definition(qualified_name.as_str())
            .is_some_and(|definition| {
                matches!(
                    &definition.body,
                    DefinitionBody::Value {
                        kind: ValueKind::Constant(ConstantValue::Integer { value, .. }),
                        ..
                    } if *value > i64::MAX as u64
                )
            })
    }

    fn package_member_is_fixed(&self, package: &str, member: &str) -> bool {
        let qualified_name = format!("{}.{}", package, member);
        self.definition(qualified_name.as_str())
            .is_some_and(|definition| {
                matches!(definition.body, DefinitionBody::Value { .. }) && !definition.is_variable()
            })
    }
}
