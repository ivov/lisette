use syntax::ast::{Expression, StructKind};
use syntax::program::{
    Definition, DefinitionBody, DotAccessKind as SemanticDotKind, ReceiverCoercion,
};
use syntax::types::Type;

use crate::EmitEffects;
use crate::Planner;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::context::expression::ExpressionContext;
use crate::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{ValuePlan, value_plan_from_statements};

impl Planner<'_> {
    pub(crate) fn plan_dot_access(
        &mut self,
        dot_access: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let Expression::DotAccess {
            expression,
            member,
            ty: result_ty,
            dot_access_kind,
            receiver_coercion,
            ..
        } = dot_access
        else {
            unreachable!("plan_dot_access requires a DotAccess expression");
        };
        let dot_access_kind = *dot_access_kind;
        let receiver_coercion = *receiver_coercion;

        if let Some(s) =
            self.try_emit_pre_receiver_dot(expression, member, result_ty, dot_access_kind, ctx, fx)
        {
            return ValuePlan::Operand(s);
        }

        let (mut setup, expression_string) =
            self.plan_coerced_expression(expression, receiver_coercion, ctx, fx);
        let expression_ty = expression.get_type();

        if let Some(module) = expression_ty.as_import_namespace() {
            self.require_module_import_fx(module, fx);
        }

        if let Some(s) = self.try_emit_tuple_member_dot(
            &expression_string,
            &expression_ty,
            member,
            dot_access_kind,
            fx,
        ) {
            return value_plan_from_statements(setup, s);
        }

        let is_exported =
            self.resolve_is_exported(expression, &expression_ty, member, dot_access_kind);
        let field = self
            .try_resolve_cross_module_const(&expression_ty, member)
            .unwrap_or_else(|| go_field_name(&expression_ty, member, is_exported));

        if let Some(s) = self.plan_nullable_field_access(
            &mut setup,
            &expression_string,
            &field,
            &expression_ty,
            result_ty,
            fx,
        ) {
            return value_plan_from_statements(setup, s);
        }

        let result = format!("{}.{}", expression_string, field);
        let result =
            self.append_cross_module_type_args(result, &expression_ty, member, result_ty, ctx, fx);
        value_plan_from_statements(setup, result)
    }

    /// Dispatch kinds that can resolve without the receiver emitted first.
    /// `ModuleMember` and unresolved kinds may still resolve under a
    /// cross-module/alias rename.
    fn try_emit_pre_receiver_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        dot_access_kind: Option<SemanticDotKind>,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        match dot_access_kind {
            Some(SemanticDotKind::ValueEnumVariant) => {
                self.emit_value_enum_variant(expression, member, fx)
            }
            Some(SemanticDotKind::EnumVariant) => self.emit_enum_variant_dot(member, result_ty, fx),
            Some(SemanticDotKind::StaticMethod { .. }) => {
                self.emit_static_method_dot(expression, member, result_ty, ctx, fx)
            }
            Some(SemanticDotKind::InstanceMethodValue {
                is_exported,
                is_pointer_receiver,
            }) => self.emit_instance_method_value_dot(
                expression,
                member,
                result_ty,
                is_exported,
                is_pointer_receiver,
                fx,
            ),
            Some(SemanticDotKind::ModuleMember) | None => {
                if let Some(s) = self.emit_enum_variant_dot(member, result_ty, fx) {
                    Some(s)
                } else {
                    self.emit_static_method_dot(expression, member, result_ty, ctx, fx)
                }
            }
            _ => None,
        }
    }

    /// Tuple-shape members: plain tuple slots emit as `.F{index}` (or the
    /// `TUPLE_FIELDS` name); tuple-struct slots additionally try a newtype
    /// cast when the struct has a single field and no generics.
    fn try_emit_tuple_member_dot(
        &mut self,
        expression_string: &str,
        expression_ty: &Type,
        member: &str,
        dot_access_kind: Option<SemanticDotKind>,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Ok(index) = member.parse::<usize>() else {
            return None;
        };
        match dot_access_kind {
            Some(SemanticDotKind::TupleElement) => {
                let field = syntax::parse::TUPLE_FIELDS
                    .get(index)
                    .expect("oversize tuple arity");
                Some(format!("{}.{}", expression_string, field))
            }
            Some(SemanticDotKind::TupleStructField { is_newtype }) => {
                if is_newtype
                    && let Some(cast) =
                        self.try_emit_newtype_cast(expression_ty, expression_string, fx)
                {
                    return Some(cast);
                }
                Some(format!("{}.F{}", expression_string, index))
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
        dot_access_kind: Option<SemanticDotKind>,
    ) -> bool {
        match dot_access_kind {
            Some(SemanticDotKind::StructField { is_exported }) => {
                is_exported || self.field_is_public(expression_ty, member)
            }
            Some(SemanticDotKind::InstanceMethod { is_exported }) => {
                is_exported || self.method_needs_export(member)
            }
            _ => {
                self.compute_is_exported_context(expression, expression_ty)
                    || self.field_is_public(expression_ty, member)
                    || (!self.has_field(expression_ty, member) && self.method_needs_export(member))
            }
        }
    }

    /// Accessing a nullable field on a Go-imported type: capture the raw
    /// access into a temp and wrap in the Some/None nullable shape expected
    /// downstream. Returns `None` when no wrapping is needed.
    fn plan_nullable_field_access(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression_string: &str,
        field: &str,
        expression_ty: &Type,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if self.go_imported_shape(expression_ty).is_none() || !self.is_go_nullable(result_ty) {
            return None;
        }
        let raw_access = format!("{}.{}", expression_string, field);
        let raw_var = self.hoist_tmp_value_statement(setup, "raw", &raw_access);
        let coercion = Coercion::resolve(
            self,
            result_ty,
            result_ty,
            CoercionDirection::FromGoBoundary,
        );
        let (coercion_setup, coerced) = coercion.lower(self, raw_var, fx);
        setup.extend(coercion_setup);
        Some(coerced)
    }

    /// When accessing a cross-module generic member by value (not as a callee),
    /// look up the instantiation's type args and append them to the expression.
    /// Callee-position accesses skip this because the call site re-instantiates.
    fn append_cross_module_type_args(
        &mut self,
        base_access: String,
        expression_ty: &Type,
        member: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        if ctx.is_callee() {
            return base_access;
        }
        let Some(module) = expression_ty.as_import_namespace() else {
            return base_access;
        };
        let qualified = format!("{}.{}", module, member);
        match self.format_cross_module_type_args(&qualified, result_ty, fx) {
            Some(type_args) => format!("{}{}", base_access, type_args),
            None => base_access,
        }
    }

    /// Emit a newtype cast like `MyType(inner)` for single-field tuple struct access.
    /// Returns None if the struct shape doesn't match (no single field, non-struct type).
    fn try_emit_newtype_cast(
        &mut self,
        expression_ty: &Type,
        expression_string: &str,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let deref_ty = expression_ty.strip_refs();
        let Type::Nominal { id, .. } = &deref_ty else {
            return None;
        };
        let Some(Definition {
            body: DefinitionBody::Struct { fields, .. },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };
        let field_ty = fields.first()?.ty.clone();
        let go_type = self.go_type_string(&field_ty, fx);
        let operand = if expression_ty.is_ref() {
            format!("*{}", expression_string)
        } else {
            expression_string.to_string()
        };
        Some(if go_type.starts_with('*') {
            format!("({})({})", go_type, operand)
        } else {
            format!("{}({})", go_type, operand)
        })
    }

    /// Compute whether a dot access context requires exported (capitalized) Go names.
    /// Used as fallback when semantic DotAccessKind doesn't carry `is_exported`.
    fn compute_is_exported_context(&self, expression: &Expression, expression_ty: &Type) -> bool {
        let is_import_namespace_identifier = matches!(
            expression,
            Expression::Identifier { ty, .. } if ty.as_import_namespace().is_some()
        );
        is_import_namespace_identifier
            || is_from_prelude(expression_ty)
            || if let Type::Nominal { id, .. } = expression_ty.strip_refs() {
                self.facts
                    .module_for_qualified_name(id.as_str())
                    .is_some_and(|m| self.facts.is_foreign_module(m))
            } else {
                false
            }
    }

    /// Emit the base expression with receiver coercion applied.
    ///
    /// Handles explicit deref (`.*`), absorbed `Ref<T>` generics, and auto-address/auto-deref
    /// coercions. Returns the Go expression string ready for member access.
    fn plan_coerced_expression(
        &mut self,
        expression: &Expression,
        coercion: Option<ReceiverCoercion>,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let (staged, had_explicit_deref) = if let Some(inner) = expression.deref_inner() {
            (self.stage_operand(inner, ctx, fx), true)
        } else {
            (self.stage_operand(expression, ctx, fx), false)
        };
        let mut setup = staged.setup;
        let expression_string = staged.value;

        let is_absorbed_ref = self.is_absorbed_ref_generic(expression);

        let value = match (coercion, had_explicit_deref) {
            _ if is_absorbed_ref => expression_string,
            (Some(ReceiverCoercion::AutoAddress), true) => expression_string,
            (Some(ReceiverCoercion::AutoAddress), false) => match expression.unwrap_parens() {
                Expression::Call { .. } => {
                    self.hoist_tmp_value_statement(&mut setup, "ref", &expression_string)
                }
                Expression::StructCall { .. } => format!("(&{})", expression_string),
                _ => expression_string,
            },
            (Some(ReceiverCoercion::AutoDeref), _) => expression_string,
            (None, true) => expression_string,
            (None, false) => expression_string,
        };
        (setup, value)
    }

    /// Check if expression has an absorbed `Ref<T>` generic (T already emitted as `*Concrete`).
    /// When true, suppress auto-deref coercion — the pointer is already the right type.
    fn is_absorbed_ref_generic(&self, expression: &Expression) -> bool {
        let check_expression = expression.deref_inner().unwrap_or(expression);
        let expression_ty = check_expression.get_type();
        expression_ty.is_ref()
            && expression_ty.inner().is_some_and(|inner| {
                matches!(inner, Type::Parameter(name)
                    if self.function_state.is_absorbed_ref_generic(name.as_ref()))
            })
    }

    pub(crate) fn try_emit_tuple_struct_field_access(
        &mut self,
        expression_string: &str,
        expression_ty: &Type,
        index: usize,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let deref_ty = expression_ty.strip_refs();
        let Type::Nominal { ref id, .. } = deref_ty else {
            return None;
        };

        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    kind,
                    fields,
                    generics,
                    ..
                },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };

        if *kind != StructKind::Tuple {
            return None;
        }

        if fields.len() == 1 && generics.is_empty() {
            let underlying_ty = self.go_type_string(&fields[0].ty, fx);
            let expression = if expression_ty.is_ref() {
                format!("*{}", expression_string)
            } else {
                expression_string.to_string()
            };
            return Some(format!("{}({})", underlying_ty, expression));
        }

        Some(format!("{}.F{}", expression_string, index))
    }

    fn try_resolve_cross_module_const(&self, expression_ty: &Type, member: &str) -> Option<String> {
        let module = expression_ty.as_import_namespace()?;
        if go_name::is_go_import(module) {
            return None;
        }
        let qualified_name = format!("{}.{}", module, member);
        let definition = self.facts.definition(qualified_name.as_str())?;
        if !definition.visibility().is_public() {
            return None;
        }
        if !matches!(definition.body, DefinitionBody::Value { .. }) {
            return None;
        }
        let ty = definition.ty();
        let is_function = matches!(ty, Type::Function(_))
            || matches!(ty, Type::Forall { body, .. } if matches!(body.as_ref(), Type::Function(_)));
        if is_function {
            return None;
        }
        Some(member.to_string())
    }
}

/// Pick the Go-side name for a struct field or method. Exported members on
/// prelude types follow snake_case → camelCase (matching the stdlib
/// convention); exported members elsewhere get first-letter capitalization;
/// non-exported members are escaped to avoid Go keywords.
fn go_field_name(expression_ty: &Type, member: &str, is_exported: bool) -> String {
    if expression_ty
        .as_import_namespace()
        .is_some_and(go_name::is_go_import)
    {
        return member.to_string();
    }

    let is_prelude_type = expression_ty
        .strip_refs()
        .get_qualified_id()
        .is_some_and(|id| id.starts_with(go_name::PRELUDE_PREFIX));

    if !is_exported {
        return go_name::escape_keyword(member).into_owned();
    }
    if is_prelude_type {
        go_name::snake_to_camel(member)
    } else {
        go_name::make_exported(member)
    }
}

/// Whether the type resolves to a prelude-module declaration. Shared with
/// the struct-call path, which also uses prelude-ness to decide field
/// naming and type formatting.
pub(super) fn is_from_prelude(ty: &Type) -> bool {
    let Type::Nominal { id, .. } = ty.strip_refs() else {
        return false;
    };
    // Only return true if the type actually comes from the prelude module.
    // User-defined types with the same name should NOT be treated as prelude types.
    id.starts_with(go_name::PRELUDE_PREFIX)
}
