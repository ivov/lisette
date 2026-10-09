use crate::state::bindings::BindingValue;
use syntax::ast::{Expression, StructFields};
use syntax::parse;
use syntax::program::{Definition, DefinitionBody, DotAccessResolution, ReceiverCoercion};
use syntax::types::{CompoundKind, Symbol, Type};

use crate::Planner;
use crate::abi::coercion::{CoercionPlan, LayoutBridge};
use crate::abi::layout::SlotOrigin;
use crate::calls::go_interop::NilGuard;
use crate::context::expression::ExpressionContext;
use crate::go_name;
use crate::plan::bodies::Statement;
use crate::plan::values::{EvaluationEffect, GoExpression, Stability, ValuePlan};
use crate::utils::reads_value_member;

struct NullableFieldAccess<'a> {
    base: &'a GoExpression,
    member: &'a str,
    field: &'a str,
    expression_ty: &'a Type,
    declaring_type: Option<&'a Symbol>,
    result_ty: &'a Type,
}

impl Planner<'_> {
    pub(crate) fn plan_dot_access(
        &mut self,
        dot_access: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        self.plan_dot_access_with(dot_access, ctx, true)
    }

    /// Read a nullable Go field as its raw Go value, without the `Option` wrap.
    pub(crate) fn plan_raw_nullable_field(&mut self, dot_access: &Expression) -> ValuePlan {
        self.plan_dot_access_with(dot_access, ExpressionContext::value(), false)
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
        let receiver_coercion = resolution.receiver_coercion();

        if let Some(expression) = self.try_emit_pre_receiver_dot(member, result_ty, resolution, ctx)
        {
            return ValuePlan::computed(Vec::new(), expression, EvaluationEffect::Pure);
        }

        if let Some(component) = self.tuple_component_read(expression, member) {
            return ValuePlan::captured(Vec::new(), component);
        }

        let expression_ty = expression.get_type();

        let package = expression_ty
            .as_import_namespace()
            .map(|package| self.package_use_for_package(package));
        let base_plan = match &package {
            Some(package) => ValuePlan::captured(Vec::new(), package.qualifier().to_string()),
            None => self.plan_coerced_expression(expression, receiver_coercion, ctx),
        };
        let effect = base_plan.facts().effect;
        let stability = if let Some(package) = expression_ty.as_import_namespace()
            && self.package_member_is_fixed(package, member)
        {
            Stability::Fixed
        } else if reads_value_member(resolution, expression, &expression_ty) {
            base_plan.facts().stability
        } else {
            Stability::Observable
        };
        let (mut setup, base) = base_plan.into_parts();
        if let Some(member_access) =
            self.try_emit_tuple_member_dot(&base, &expression_ty, member, resolution)
        {
            return ValuePlan::computed(setup, member_access, effect).with_stability(stability);
        }

        let is_exported = self.resolve_is_exported(expression, &expression_ty, member, resolution);
        let is_embedded = self.field_is_embedded(&expression_ty, member);
        let field = self
            .try_resolve_cross_package_const(&expression_ty, member)
            .unwrap_or_else(|| go_field_name(&expression_ty, member, is_exported, is_embedded));

        if wrap_nullable
            && let Some(wrapped) = self.plan_nullable_field_access(
                &mut setup,
                NullableFieldAccess {
                    base: &base,
                    member,
                    field: &field,
                    expression_ty: &expression_ty,
                    declaring_type: resolution.declaring_type(),
                    result_ty,
                },
            )
        {
            return ValuePlan::computed(setup, wrapped, effect);
        }

        let value_field =
            resolution.is_field_read() && self.field_read_cannot_panic(&expression_ty);
        let selector = match package {
            Some(package) => GoExpression::qualified(package, field),
            None if value_field => GoExpression::value_field(base, field),
            None => GoExpression::selector(base, field),
        };
        let expression =
            self.append_cross_package_type_args(selector, &expression_ty, resolution, ctx);
        ValuePlan::computed(setup, expression, effect).with_stability(stability)
    }

    fn try_emit_pre_receiver_dot(
        &mut self,
        member: &str,
        result_ty: &Type,
        resolution: &DotAccessResolution,
        ctx: ExpressionContext<'_>,
    ) -> Option<GoExpression> {
        match resolution {
            DotAccessResolution::EnumVariant { definition, .. } => {
                self.emit_enum_variant_dot(definition, result_ty)
            }
            DotAccessResolution::StaticMethod { definition, .. } => {
                Some(self.emit_static_method_dot(definition, resolution, ctx))
            }
            DotAccessResolution::InstanceMethodValue {
                is_exported,
                is_pointer_receiver,
                ..
            } => self.emit_instance_method_value_dot(
                member,
                result_ty,
                *is_exported,
                *is_pointer_receiver,
            ),
            _ => None,
        }
    }

    fn tuple_component_read(&self, expression: &Expression, member: &str) -> Option<String> {
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
        tuple.names.get(index).map(ToString::to_string)
    }

    /// Tuple-shape members: plain tuple slots emit as `.F{index}` (or the
    /// `TUPLE_FIELDS` name); tuple-struct slots additionally try a newtype
    /// cast when the struct has a single field and no generics.
    fn try_emit_tuple_member_dot(
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
                Some(self.field_access(base.clone(), expression_ty, format!("F{}", index)))
            }
            _ => None,
        }
    }

    /// Whether the Go member name must be capitalized. Adds emit-side checks
    /// on top of semantic `is_exported` (`#[json]`, interface methods).
    fn resolve_is_exported(
        &self,
        expression: &Expression,
        expression_ty: &Type,
        member: &str,
        resolution: &DotAccessResolution,
    ) -> bool {
        match resolution {
            DotAccessResolution::StructField { is_exported, .. } => {
                *is_exported || self.struct_field_is_exported(expression_ty, member)
            }
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
        if expression_ty.as_import_namespace().is_some() {
            return None;
        }
        let source_layout = self.field_slot_layout(
            &expression_ty,
            resolution.declaring_type(),
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

    /// Accessing a nullable field on a Go-imported type: capture the raw
    /// access into a temp and wrap in the Some/None nullable shape expected
    /// downstream. Returns `None` when no wrapping is needed.
    fn plan_nullable_field_access(
        &mut self,
        setup: &mut Vec<Statement>,
        access: NullableFieldAccess<'_>,
    ) -> Option<GoExpression> {
        let NullableFieldAccess {
            base,
            member,
            field,
            expression_ty,
            declaring_type,
            result_ty,
        } = access;
        let source_layout =
            self.field_slot_layout(expression_ty, declaring_type, member, result_ty)?;
        let target_layout = self.value_layout(result_ty, SlotOrigin::Lisette);
        let coercion = CoercionPlan::bridge(self, &source_layout, &target_layout);
        if coercion.is_identity() {
            return None;
        }
        let raw_access = GoExpression::selector(base.clone(), field.to_string());
        let raw_var = self.hoist_tmp_value_statement(setup, "raw", raw_access);
        let (coercion_setup, coerced) = coercion.lower(self, GoExpression::name(raw_var));
        setup.extend(coercion_setup);
        Some(coerced)
    }

    /// When accessing a cross-package generic member by value (not as a callee),
    /// append the type args of the instantiation the checker recorded.
    /// Callee-position accesses skip this because the call site re-instantiates.
    fn append_cross_package_type_args(
        &mut self,
        base_access: GoExpression,
        expression_ty: &Type,
        resolution: &DotAccessResolution,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        if ctx.is_callee() || expression_ty.as_import_namespace().is_none() {
            return base_access;
        }
        match self.format_value_type_args(resolution.definition(), resolution.instantiation()) {
            Some(type_args) => GoExpression::instantiation(base_access, type_args),
            None => base_access,
        }
    }

    /// Emit `.0` on a newtype as a Go conversion to the field type, `int(n)`.
    /// Peels type aliases, so `.0` through `type Alias = New` also converts.
    /// Returns None when the type is not a newtype.
    fn try_emit_newtype_cast(
        &mut self,
        expression_ty: &Type,
        base: &GoExpression,
    ) -> Option<GoExpression> {
        let field_ty = self.get_newtype_underlying(expression_ty)?;
        let go_type = self.use_go_type(&field_ty);
        let operand = if expression_ty.is_ref() {
            GoExpression::dereference(base.clone())
        } else {
            base.clone()
        };
        Some(GoExpression::conversion(go_type, operand))
    }

    /// Compute whether a dot access context requires exported (capitalized) Go names.
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
        let (staged, had_explicit_deref) = if let Some(inner) = expression.deref_inner() {
            (self.plan_operand(inner, ctx), true)
        } else {
            (self.plan_operand(expression, ctx), false)
        };
        let receiver = expression.deref_inner().unwrap_or(expression);
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
                    GoExpression::name(self.hoist_tmp_value_statement(setup, "ref", base))
                }
                Expression::StructCall { .. } => GoExpression::address_of(base),
                _ => base,
            }
        })
    }

    pub(crate) fn try_emit_tuple_struct_field_access(
        &self,
        base: GoExpression,
        expression_ty: &Type,
        index: usize,
    ) -> Option<GoExpression> {
        let deref_ty = expression_ty.strip_refs();
        let Type::Nominal { ref id, .. } = deref_ty else {
            return None;
        };

        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    fields: StructFields::Tuple(_),
                    ..
                },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };

        Some(GoExpression::selector(base, format!("F{index}")))
    }

    fn package_member_is_fixed(&self, package: &str, member: &str) -> bool {
        let qualified_name = format!("{}.{}", package, member);
        self.facts
            .definition(qualified_name.as_str())
            .is_some_and(|definition| {
                matches!(definition.body, DefinitionBody::Value { .. }) && !definition.is_variable()
            })
    }

    fn try_resolve_cross_package_const(
        &self,
        expression_ty: &Type,
        member: &str,
    ) -> Option<String> {
        let package = expression_ty.as_import_namespace()?;
        if go_name::is_go_import(package) {
            return None;
        }
        let qualified_name = format!("{}.{}", package, member);
        let definition = self.facts.definition(qualified_name.as_str())?;
        if !definition.visibility.is_public() {
            return None;
        }
        if !matches!(definition.body, DefinitionBody::Value { .. }) {
            return None;
        }
        let ty = &definition.ty;
        let is_function = matches!(ty, Type::Function(_))
            || matches!(ty, Type::Forall { body, .. } if matches!(body.as_ref(), Type::Function(_)));
        if is_function {
            return None;
        }
        Some(go_name::screaming_snake_to_camel(member))
    }
}

/// Pick the Go-side name for a struct field or method. Exported members on
/// prelude types follow snake_case → camelCase (matching the stdlib
/// convention); exported members elsewhere get first-letter capitalization;
/// non-exported members become lower camelCase, embedded fields keep their
/// type's name.
fn go_field_name(
    expression_ty: &Type,
    member: &str,
    is_exported: bool,
    is_embedded: bool,
) -> String {
    if expression_ty
        .as_import_namespace()
        .is_some_and(go_name::is_go_import)
    {
        return member.to_string();
    }

    go_name::member_go_name(expression_ty, member, is_exported, is_embedded)
}

/// Whether the type resolves to a prelude-package declaration. Shared with
/// the struct-call path, which also uses prelude-ness to decide field
/// naming and type formatting.
pub(super) fn is_from_prelude(ty: &Type) -> bool {
    let Type::Nominal { id, .. } = ty.strip_refs() else {
        return false;
    };
    // Only return true if the type actually comes from the prelude package.
    // User-defined types with the same name should NOT be treated as prelude types.
    id.starts_with(go_name::PRELUDE_PREFIX)
}
