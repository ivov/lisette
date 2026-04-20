use syntax::ast::{Expression, Span, StructKind, UnaryOperator};
use syntax::program::{Definition, DotAccessKind as SemanticDotKind, ReceiverCoercion};
use syntax::types::Type;

use crate::Emitter;
use crate::go_name;
use crate::write_line;

impl Emitter<'_> {
    pub(crate) fn emit_dot_access(
        &mut self,
        output: &mut String,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        span: Span,
    ) -> String {
        let dot_access_kind = self.ctx.resolutions.get_dot_access(span);

        // Phase 1: cases that don't need the receiver emitted first. ModuleMember
        // and unresolved accesses may still resolve to enum/static form (e.g.
        // cross-module or alias patterns), so fall through to both.
        let phase_one = match dot_access_kind {
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
        };
        if let Some(s) = phase_one {
            return s;
        }

        // Phase 2: Post-receiver emission (struct fields, tuple fields, instance methods)
        let expression_string = self.emit_coerced_expression(output, expression);
        let expression_ty = expression.get_type();

        // Tuple element: direct field access using TUPLE_FIELDS names
        if let Some(SemanticDotKind::TupleElement) = dot_access_kind
            && let Ok(index) = member.parse::<usize>()
        {
            let field = syntax::parse::TUPLE_FIELDS
                .get(index)
                .expect("oversize tuple arity");
            return format!("{}.{}", expression_string, field);
        }

        // Tuple struct field: newtype cast or positional field access
        if let Some(SemanticDotKind::TupleStructField { is_newtype }) = dot_access_kind
            && let Ok(index) = member.parse::<usize>()
        {
            if is_newtype
                && let Some(cast) = self.try_emit_newtype_cast(&expression_ty, &expression_string)
            {
                return cast;
            }
            return format!("{}.F{}", expression_string, index);
        }

        // Determine whether to capitalize the Go name from pre-computed metadata.
        // Semantic `is_exported` covers cross-module + public visibility.
        // Emit-side checks are still needed for Go-specific concerns:
        // - `field_is_public`: also checks #[json] tag-exported fields
        // - `method_needs_export`: methods that must be capitalized for Go interfaces
        let is_exported = match dot_access_kind {
            Some(SemanticDotKind::StructField { is_exported }) => {
                is_exported || self.field_is_public(&expression_ty, member)
            }
            Some(SemanticDotKind::InstanceMethod { is_exported }) => {
                is_exported || self.method_needs_export(member)
            }
            _ => {
                // Fallback for ModuleMember/None/unresolved
                self.compute_is_exported_context(expression, &expression_ty)
                    || self.field_is_public(&expression_ty, member)
                    || (!self.has_field(&expression_ty, member) && self.method_needs_export(member))
            }
        };

        let is_prelude_type = expression_ty
            .resolve()
            .strip_refs()
            .get_qualified_id()
            .is_some_and(|id| id.starts_with(go_name::PRELUDE_PREFIX));

        let field = if is_exported {
            if is_prelude_type {
                go_name::snake_to_camel(member)
            } else {
                go_name::make_exported(member)
            }
        } else {
            go_name::escape_keyword(member).into_owned()
        };

        // Go nullable field wrapping
        if Self::is_go_imported_type(&expression_ty) && self.is_go_nullable(result_ty) {
            let raw_access = format!("{}.{}", expression_string, field);
            let raw_var = self.fresh_var(Some("raw"));
            self.declare(&raw_var);
            write_line!(output, "{} := {}", raw_var, raw_access);
            return self.maybe_wrap_go_nullable(output, &raw_var, result_ty);
        }

        // Regular field/method access with cross-module type args
        let result = format!("{}.{}", expression_string, field);
        if !self.emitting_call_callee {
            let resolved_expression_ty = expression_ty.resolve();
            if let Type::Constructor { ref id, .. } = resolved_expression_ty
                && let Some(module) = id.strip_prefix(go_name::IMPORT_PREFIX)
            {
                let qualified = format!("{}.{}", module, member);
                if let Some(type_args) = self.format_cross_module_type_args(&qualified, result_ty) {
                    return format!("{}{}", result, type_args);
                }
            }
        }
        result
    }

    /// Emit a newtype cast like `MyType(inner)` for single-field tuple struct access.
    /// Returns None if the struct shape doesn't match (no single field, non-struct type).
    fn try_emit_newtype_cast(
        &mut self,
        expression_ty: &Type,
        expression_string: &str,
    ) -> Option<String> {
        let deref_ty = expression_ty.resolve().strip_refs();
        let Type::Constructor { id, .. } = &deref_ty else {
            return None;
        };
        let Some(Definition::Struct { fields, .. }) = self.ctx.definitions.get(id.as_str()) else {
            return None;
        };
        let field_ty = fields.first()?.ty.clone();
        let go_type = self.go_type_as_string(&field_ty);
        let operand = if expression_ty.resolve().is_ref() {
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
        matches!(
            expression,
            Expression::Identifier { ty: Type::Constructor { id, .. }, .. } if id.starts_with(go_name::IMPORT_PREFIX)
        ) || self.is_from_prelude(expression_ty)
            || if let Type::Constructor { id, .. } = expression_ty.resolve().strip_refs() {
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
    fn emit_coerced_expression(&mut self, output: &mut String, expression: &Expression) -> String {
        let coercion = self.ctx.coercions.get_coercion(expression.get_span());

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
            (Some(ReceiverCoercion::AutoAddress), false) => {
                if matches!(expression.unwrap_parens(), Expression::Call { .. }) {
                    let tmp = self.fresh_var(Some("ref"));
                    self.declare(&tmp);
                    write_line!(output, "{} := {}", tmp, expression_string);
                    tmp
                } else {
                    expression_string
                }
            }
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
        let expression_ty = check_expression.get_type().resolve();
        expression_ty.is_ref()
            && expression_ty.inner().is_some_and(|inner| {
                matches!(inner.resolve(), Type::Parameter(name)
                    if self.module.absorbed_ref_generics.contains(name.as_ref()))
            })
    }

    pub(crate) fn try_emit_tuple_struct_field_access(
        &mut self,
        expression_string: &str,
        expression_ty: &Type,
        index: usize,
    ) -> Option<String> {
        let deref_ty = expression_ty.resolve().strip_refs();
        let Type::Constructor { ref id, .. } = deref_ty else {
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
            let expression = if expression_ty.resolve().is_ref() {
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
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
            return false;
        };
        // Only return true if the type actually comes from the prelude module.
        // User-defined types with the same name should NOT be treated as prelude types.
        id.starts_with(go_name::PRELUDE_PREFIX)
    }
}
