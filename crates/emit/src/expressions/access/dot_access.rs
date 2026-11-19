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
use crate::utils::reads_value_member;
use syntax::ast::Expression;
use syntax::parse;
use syntax::program::GoHint;
use syntax::program::SlotOrigin;
use syntax::program::{
    ConstantValue, DefinitionBody, DotAccessResolution, ReceiverCoercion, ValueKind,
};
use syntax::types::{CompoundKind, Symbol, Type};

struct NullableFieldAccess<'a> {
    value: GoExpression,
    member: &'a str,
    expression_ty: &'a Type,
    declaring_type: Option<&'a Symbol>,
    result_ty: &'a Type,
    callee: bool,
}

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
        let receiver_coercion =
            self.facts
                .checked(expression)
                .receiver_change()
                .filter(|coercion| {
                    *coercion != ReceiverCoercion::AutoAddress
                        || !self.has_go_value_receiver(&expression.get_type(), member)
                });
        let tagged_twin = self.selects_tagged_twin(dot_access);

        if let Some(expression) = self.try_emit_pre_receiver_dot(dot_access, tagged_twin, ctx) {
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
                && let Some(wrapped) = self.plan_nullable_field_access(
                    &mut setup,
                    NullableFieldAccess {
                        value: GoExpression::identifier(component.clone()),
                        member,
                        expression_ty: &expression_ty,
                        declaring_type: resolution.declaring_type(),
                        result_ty,
                        callee: ctx.is_callee(),
                    },
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
        let stability = if let Some(package) = expression_ty.as_import_namespace()
            && self.package_member_is_fixed(package, member)
        {
            Stability::Fixed
        } else if reads_value_member(&self.facts, resolution, expression, &expression_ty) {
            base_plan.stability()
        } else {
            Stability::Observable
        };
        let (mut setup, mut base) = base_plan.into_parts();
        if let Some((bound, _)) = self.bound_dispatch_requirement(dot_access) {
            let receiver_type = self.facts.strip_and_peel(&expression_ty);
            let adapter = self
                .dynamic_interface_adapter(&receiver_type, &bound, None)
                .expect("a dispatched bound has an adapter");
            let (adapter_setup, adapted) = CoercionPlan::WrapAsInterface(adapter).lower(self, base);
            setup.extend(adapter_setup);
            base = adapted;
        }
        if let Some(member_access) =
            self.try_emit_tuple_member_dot(&base, &expression_ty, member, resolution)
        {
            if wrap_nullable
                && let Some(wrapped) = self.plan_nullable_field_access(
                    &mut setup,
                    NullableFieldAccess {
                        value: member_access.clone(),
                        member,
                        expression_ty: &expression_ty,
                        declaring_type: resolution.declaring_type(),
                        result_ty,
                        callee: ctx.is_callee(),
                    },
                )
            {
                return ValuePlan::computed(setup, wrapped);
            }
            return ValuePlan::computed(setup, member_access).with_stability(stability);
        }

        let mut field = self.dot_member_go_name(expression, &expression_ty, member, resolution);
        if tagged_twin {
            field = go_name::tagged_twin(&field);
        }

        let selector = match package {
            Some(package) => GoExpression::qualified(package, field),
            None if resolution.is_field_read() => self.field_access(base, &expression_ty, field),
            None => GoExpression::selector(base, field),
        };
        if wrap_nullable
            && let Some(wrapped) = self.plan_nullable_field_access(
                &mut setup,
                NullableFieldAccess {
                    value: selector.clone(),
                    member,
                    expression_ty: &expression_ty,
                    declaring_type: resolution.declaring_type(),
                    result_ty,
                    callee: ctx.is_callee(),
                },
            )
        {
            return ValuePlan::computed(setup, wrapped);
        }
        let expression =
            self.append_cross_package_type_args(selector, dot_access, &expression_ty, ctx);
        let expression = if let Some(package) = expression_ty.as_import_namespace()
            && self.go_constant_exceeds_int(package, member, result_ty)
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

    fn try_emit_pre_receiver_dot(
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
                self.emit_enum_variant_dot(definition, result_ty, ctx)
            }
            DotAccessResolution::StaticMethod { definition, .. } => {
                Some(self.emit_static_method_dot(dot_access, definition, ctx))
            }
            DotAccessResolution::InstanceMethodValue { is_exported, .. } => {
                self.emit_instance_method_value_dot(member, result_ty, *is_exported, tagged_twin)
            }
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
    ) -> String {
        if let Some(package) = expression_ty.as_import_namespace() {
            return self.package_member_go_name(package, member);
        }
        let canonical_ty = self.facts.strip_and_peel(expression_ty);
        if let DotAccessResolution::StructField { is_exported, .. } = resolution {
            return self.struct_field_go_name(&canonical_ty, member, *is_exported);
        }
        let is_exported = self.resolve_is_exported(expression, &canonical_ty, member, resolution);
        let embedded = self.embedded_field(expression_ty, member);
        go_name::member_go_name(&canonical_ty, member, is_exported, embedded)
    }

    pub(crate) fn try_emit_tuple_member_dot(
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
                let field = parse::TUPLE_FIELDS
                    .get(index)
                    .expect("oversize tuple arity");
                Some(self.field_access(base.clone(), expression_ty, field.to_string()))
            }
            DotAccessResolution::TupleStructField { is_newtype } => {
                if *is_newtype && let Some(cast) = self.try_emit_newtype_cast(expression_ty, base) {
                    return Some(cast);
                }
                Some(self.field_access(
                    base.clone(),
                    expression_ty,
                    go_name::tuple_struct_field(index),
                ))
            }
            _ => None,
        }
    }

    fn resolve_is_exported(
        &self,
        expression: &Expression,
        expression_ty: &Type,
        member: &str,
        resolution: &DotAccessResolution,
    ) -> bool {
        match resolution {
            DotAccessResolution::InstanceMethod { is_exported, .. } => {
                *is_exported || self.method_needs_export(member)
            }
            _ => {
                if self.compute_is_exported_context(expression, expression_ty)
                    || self.field_is_public(expression_ty, member)
                {
                    return true;
                }
                !self.has_field(expression_ty, member) && self.method_needs_export(member)
            }
        }
    }

    pub(crate) fn nullable_field_guard(&self, subject: &Expression) -> Option<NilGuard> {
        let Expression::DotAccess {
            expression,
            member,
            ty: result_ty,
            resolution,
            ..
        } = subject.unwrap_parens()
        else {
            return None;
        };
        let expression_ty = expression.get_type();
        let source_layout = self.field_slot_layout(
            &expression_ty,
            resolution.declaring_type(),
            None,
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
        Some(self.option_nil_guard(result_ty))
    }

    fn plan_nullable_field_access(
        &mut self,
        setup: &mut Vec<Statement>,
        access: NullableFieldAccess<'_>,
    ) -> Option<GoExpression> {
        let NullableFieldAccess {
            value,
            member,
            expression_ty,
            declaring_type,
            result_ty,
            callee,
        } = access;
        let source_layout =
            self.field_slot_layout(expression_ty, declaring_type, None, member, result_ty)?;
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

    fn try_emit_newtype_cast(
        &mut self,
        expression_ty: &Type,
        base: &GoExpression,
    ) -> Option<GoExpression> {
        let field_ty = self.get_newtype_underlying(expression_ty)?;
        let go_type = self.go_type(&field_ty);
        let operand = if expression_ty.is_ref() {
            GoExpression::dereference(base.clone())
        } else {
            base.clone()
        };
        Some(GoExpression::conversion(go_type, operand))
    }

    fn compute_is_exported_context(&self, expression: &Expression, expression_ty: &Type) -> bool {
        let is_import_namespace_identifier = matches!(
            expression,
            Expression::Identifier { ty, .. } if ty.as_import_namespace().is_some()
        );
        is_import_namespace_identifier || self.type_uses_exported_members(expression_ty)
    }

    fn plan_coerced_expression(
        &mut self,
        expression: &Expression,
        coercion: Option<ReceiverCoercion>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let elided_deref = expression
            .deref_inner()
            .filter(|inner| !self.points_to_ref(&inner.get_type()));
        let (staged, had_explicit_deref) = if let Some(inner) = elided_deref {
            (self.plan_operand(inner, ctx), true)
        } else {
            (self.plan_operand(expression, ctx), false)
        };
        let receiver = elided_deref.unwrap_or(expression);
        let needs_deref = matches!(
            receiver.get_type().as_compound(),
            Some((CompoundKind::Ref, [Type::Parameter(_), ..]))
        );
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

    pub(crate) fn points_to_ref(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        peeled.is_ref()
            && peeled
                .inner()
                .is_some_and(|inner| self.facts.peel_alias(&inner).is_ref())
    }

    fn has_go_value_receiver(&self, receiver_ty: &Type, method: &str) -> bool {
        let receiver_ty = self.facts.peel_alias(&receiver_ty.strip_refs());
        receiver_ty
            .get_qualified_id()
            .filter(|owner| go_name::is_go_import(owner))
            .and_then(|owner| self.facts.method(owner, method))
            .is_some_and(|method| method.go_hints.contains(&GoHint::ValueMethodSet))
    }

    fn go_constant_exceeds_int(&self, package: &str, member: &str, ty: &Type) -> bool {
        if !ty.is_unsigned_int() || !go_name::is_go_import(package) {
            return false;
        }
        let qualified_name = format!("{}.{}", package, member);
        self.facts
            .definition(qualified_name.as_str())
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
        self.facts
            .definition(qualified_name.as_str())
            .is_some_and(|definition| {
                matches!(definition.body, DefinitionBody::Value { .. }) && !definition.is_variable()
            })
    }
}

impl Planner<'_> {
    pub(crate) fn emit_enum_variant_dot(
        &mut self,
        definition: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        let make_fn_name = self.facts.variant_make_function(definition)?;
        let enum_package = self.facts.package_for_qualified_name(definition)?;
        let make_fn = if go_name::is_prelude_id(&make_fn_name) {
            go_name::resolve(&make_fn_name).into_expression()
        } else if self.facts.is_current_package(enum_package) {
            GoExpression::external_name(make_fn_name)
        } else {
            GoExpression::qualified(self.package_use_for_package(enum_package), make_fn_name)
        };

        match result_ty {
            Type::Function(f) => {
                let Type::Nominal {
                    params: ret_params, ..
                } = f.return_type.as_ref()
                else {
                    return None;
                };
                let type_args = self.constructor_fn_type_args(&f.params, ret_params, ctx);
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

    pub(crate) fn emit_static_method_dot(
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

    pub(crate) fn emit_instance_method_value_dot(
        &mut self,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        tagged_twin: bool,
    ) -> Option<GoExpression> {
        let receiver_param = &result_ty.as_function_type()?.params.first()?.ty;
        let mut go_method = self.method_go_name(member, is_exported);
        if tagged_twin {
            go_method = go_name::tagged_twin(&go_method);
        }
        Some(self.method_expression(receiver_param, go_method))
    }
}
