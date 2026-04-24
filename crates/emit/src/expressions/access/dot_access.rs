use syntax::ast::{Expression, StructKind, UnaryOperator};
use syntax::program::{Definition, DotAccessKind as SemanticDotKind, ReceiverCoercion};
use syntax::types::Type;

use crate::Emitter;
use crate::go_name;
use crate::types::coercion::Coercion;
use crate::write_line;

impl Emitter<'_> {
    pub(crate) fn emit_dot_access(
        &mut self,
        output: &mut String,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        dot_access_kind: Option<SemanticDotKind>,
        receiver_coercion: Option<ReceiverCoercion>,
    ) -> String {
        if let Some(s) =
            self.try_emit_pre_receiver_dot(expression, member, result_ty, dot_access_kind)
        {
            return s;
        }

        let expression_string = self.emit_coerced_expression(output, expression, receiver_coercion);
        let expression_ty = expression.get_type();

        if let Some(s) = self.try_emit_tuple_member_dot(
            &expression_string,
            &expression_ty,
            member,
            dot_access_kind,
        ) {
            return s;
        }

        let is_exported =
            self.resolve_is_exported(expression, &expression_ty, member, dot_access_kind);
        let field = go_field_name(&expression_ty, member, is_exported);

        if let Some(s) = self.try_emit_nullable_field_access(
            output,
            &expression_string,
            &field,
            &expression_ty,
            result_ty,
        ) {
            return s;
        }

        let result = format!("{}.{}", expression_string, field);
        self.append_cross_module_type_args(result, &expression_ty, member, result_ty)
    }

    /// Phase 1 dispatch: the semantic kind may resolve without needing the
    /// receiver emitted first (value-enum variant, enum constructor, static
    /// method, instance-method value). `ModuleMember` and unresolved kinds
    /// may still resolve as an enum variant or static method under a cross-
    /// module/alias rename, so both helpers are tried in order.
    fn try_emit_pre_receiver_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        dot_access_kind: Option<SemanticDotKind>,
    ) -> Option<String> {
        match dot_access_kind {
            Some(SemanticDotKind::ValueEnumVariant) => {
                self.emit_value_enum_variant(expression, member)
            }
            Some(SemanticDotKind::EnumVariant) => {
                self.emit_enum_variant_dot(expression, member, result_ty)
            }
            Some(SemanticDotKind::StaticMethod { .. }) => {
                self.emit_static_method_dot(expression, member, result_ty)
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
            ),
            Some(SemanticDotKind::ModuleMember) | None => self
                .emit_enum_variant_dot(expression, member, result_ty)
                .or_else(|| self.emit_static_method_dot(expression, member, result_ty)),
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
                    && let Some(cast) = self.try_emit_newtype_cast(expression_ty, expression_string)
                {
                    return Some(cast);
                }
                Some(format!("{}.F{}", expression_string, index))
            }
            _ => None,
        }
    }

    /// Decide whether the Go member name needs exporting (capitalization).
    /// Semantic `is_exported` covers cross-module + public visibility; the
    /// emit-side checks additionally cover Go-specific concerns like
    /// `#[json]`-tagged fields and interface-method capitalization.
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
    fn try_emit_nullable_field_access(
        &mut self,
        output: &mut String,
        expression_string: &str,
        field: &str,
        expression_ty: &Type,
        result_ty: &Type,
    ) -> Option<String> {
        if !Self::is_go_imported_type(expression_ty) || !self.is_go_nullable(result_ty) {
            return None;
        }
        let raw_access = format!("{}.{}", expression_string, field);
        let raw_var = self.fresh_var(Some("raw"));
        self.declare(&raw_var);
        write_line!(output, "{} := {}", raw_var, raw_access);
        let coercion = Coercion::resolve_wrap_go_nullable(self, result_ty);
        Some(coercion.apply(self, output, raw_var))
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
    ) -> String {
        if self.emitting_call_callee {
            return base_access;
        }
        let Some(module) = expression_ty.as_import_namespace() else {
            return base_access;
        };
        let qualified = format!("{}.{}", module, member);
        match self.format_cross_module_type_args(&qualified, result_ty) {
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
    ) -> Option<String> {
        let deref_ty = expression_ty.strip_refs();
        let Type::Nominal { id, .. } = &deref_ty else {
            return None;
        };
        let Some(Definition::Struct { fields, .. }) = self.ctx.definitions.get(id.as_str()) else {
            return None;
        };
        let field_ty = fields.first()?.ty.clone();
        let go_type = self.go_type_as_string(&field_ty);
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
        let is_import_namespace_ident = matches!(
            expression,
            Expression::Identifier { ty, .. } if ty.as_import_namespace().is_some()
        );
        is_import_namespace_ident
            || self.is_from_prelude(expression_ty)
            || if let Type::Nominal { id, .. } = expression_ty.strip_refs() {
                id.split_once('.')
                    .is_some_and(|(m, _)| m != self.current_module && m != go_name::PRELUDE_MODULE)
            } else {
                false
            }
    }

    /// Emit the base expression with receiver coercion applied.
    ///
    /// Handles explicit deref (`.*`), absorbed `Ref<T>` generics, and auto-address/auto-deref
    /// coercions. Returns the Go expression string ready for member access.
    fn emit_coerced_expression(
        &mut self,
        output: &mut String,
        expression: &Expression,
        coercion: Option<ReceiverCoercion>,
    ) -> String {
        let (expression_string, had_explicit_deref) = if let Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } = expression
        {
            (self.emit_operand(output, inner), true)
        } else {
            (self.emit_operand(output, expression), false)
        };

        let is_absorbed_ref = self.is_absorbed_ref_generic(expression);

        match (coercion, had_explicit_deref) {
            _ if is_absorbed_ref => expression_string,
            (Some(ReceiverCoercion::AutoAddress), true) => expression_string,
            (Some(ReceiverCoercion::AutoAddress), false) => match expression.unwrap_parens() {
                Expression::Call { .. } => {
                    let tmp = self.fresh_var(Some("ref"));
                    self.declare(&tmp);
                    write_line!(output, "{} := {}", tmp, expression_string);
                    tmp
                }
                Expression::StructCall { .. } => format!("(&{})", expression_string),
                _ => expression_string,
            },
            (Some(ReceiverCoercion::AutoDeref), _) => expression_string,
            (None, true) => expression_string,
            (None, false) => expression_string,
        }
    }

    /// Check if expression has an absorbed `Ref<T>` generic (T already emitted as `*Concrete`).
    /// When true, suppress auto-deref coercion — the pointer is already the right type.
    fn is_absorbed_ref_generic(&self, expression: &Expression) -> bool {
        let check_expression = if let Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } = expression
        {
            inner.as_ref()
        } else {
            expression
        };
        let expression_ty = check_expression.get_type();
        expression_ty.is_ref()
            && expression_ty.inner().is_some_and(|inner| {
                matches!(inner, Type::Parameter(name)
                    if self.module.absorbed_ref_generics.contains(name.as_ref()))
            })
    }

    pub(crate) fn try_emit_tuple_struct_field_access(
        &mut self,
        expression_string: &str,
        expression_ty: &Type,
        index: usize,
    ) -> Option<String> {
        let deref_ty = expression_ty.strip_refs();
        let Type::Nominal { ref id, .. } = deref_ty else {
            return None;
        };

        let Some(Definition::Struct {
            kind,
            fields,
            generics,
            ..
        }) = self.ctx.definitions.get(id.as_str())
        else {
            return None;
        };

        if *kind != StructKind::Tuple {
            return None;
        }

        if fields.len() == 1 && generics.is_empty() {
            let underlying_ty = self.go_type_as_string(&fields[0].ty);
            let expression = if expression_ty.is_ref() {
                format!("*{}", expression_string)
            } else {
                expression_string.to_string()
            };
            return Some(format!("{}({})", underlying_ty, expression));
        }

        Some(format!("{}.F{}", expression_string, index))
    }

    /// Whether the type resolves to a prelude-module declaration. Shared with
    /// the struct-call path, which also uses prelude-ness to decide field
    /// naming and type formatting.
    pub(super) fn is_from_prelude(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return false;
        };
        // Only return true if the type actually comes from the prelude module.
        // User-defined types with the same name should NOT be treated as prelude types.
        id.starts_with(go_name::PRELUDE_PREFIX)
    }
}

/// Pick the Go-side name for a struct field or method. Exported members on
/// prelude types follow snake_case → camelCase (matching the stdlib
/// convention); exported members elsewhere get first-letter capitalization;
/// non-exported members are escaped to avoid Go keywords.
fn go_field_name(expression_ty: &Type, member: &str, is_exported: bool) -> String {
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
