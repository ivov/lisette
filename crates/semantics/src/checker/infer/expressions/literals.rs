use crate::checker::EnvResolve;
use crate::store::Store;
use syntax::ast::{Expression, FormatStringPart, Literal, Span};
use syntax::lex::{interpolation_holes, rune_codepoint};
use syntax::types::{CompoundKind, SimpleKind, Type};

use crate::checker::infer::InferCtx;
use crate::facts::EmptyLiteralCheck;

impl InferCtx<'_> {
    pub(super) fn infer_literal(
        &mut self,
        literal: Literal,
        expected_ty: &Type,
        span: Span,
    ) -> Expression {
        let store = self.store;
        let (literal, ty) = match literal {
            Literal::Boolean(boolean) => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if adapts_to_named_type(&resolved, store, SimpleKind::Bool) {
                    resolved.clone()
                } else {
                    let bool_ty = Type::bool();
                    self.unify(expected_ty, &bool_ty, &span);
                    bool_ty
                };

                (Literal::Boolean(boolean), ty)
            }

            Literal::Integer { value, text } => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if let Some(numeric) = store.literal_adaptation_target(&resolved) {
                    let is_pre_negated = text.as_deref().is_some_and(|t| t.starts_with('-'));
                    if !self.is_inside_constant_expression() {
                        if is_pre_negated {
                            self.check_negative_magnitude_overflow(
                                value.wrapping_neg(),
                                &numeric,
                                span,
                            );
                        } else if !self.is_inside_negation() {
                            self.check_integer_literal_overflow(value, &numeric, span);
                        }
                    }
                    resolved.clone()
                } else {
                    let int_ty = Type::int();
                    self.unify(expected_ty, &int_ty, &span);
                    int_ty
                };

                (Literal::Integer { value, text }, ty)
            }

            Literal::Float { value, text } => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if let Some(numeric) = store.literal_adaptation_target(&resolved)
                    && (numeric.is_float() || numeric.is_complex())
                {
                    self.check_float_literal_overflow(value, &numeric, span);
                    resolved.clone()
                } else {
                    let float_ty = Type::Simple(SimpleKind::Float64);
                    self.unify(expected_ty, &float_ty, &span);
                    float_ty
                };

                (Literal::Float { value, text }, ty)
            }

            Literal::Imaginary(coef) => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if let Some(numeric) = store.literal_adaptation_target(&resolved)
                    && numeric.is_complex()
                {
                    self.check_float_literal_overflow(coef, &numeric, span);
                    resolved.clone()
                } else {
                    let complex_ty = Type::Simple(SimpleKind::Complex128);
                    self.unify(expected_ty, &complex_ty, &span);
                    complex_ty
                };

                (Literal::Imaginary(coef), ty)
            }

            Literal::String { value, raw } => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if adapts_to_named_type(&resolved, store, SimpleKind::String) {
                    resolved.clone()
                } else {
                    let string_ty = Type::string();
                    self.unify(expected_ty, &string_ty, &span);
                    string_ty
                };

                if !raw
                    && !self.is_in_pattern()
                    && let Some(names) = interpolation_holes(&value)
                    && names.iter().all(|name| self.hole_would_interpolate(name))
                {
                    self.facts
                        .add_unprefixed_fstring(span, names[0].to_string());
                }

                (Literal::String { value, raw }, ty)
            }

            Literal::Char(char) => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if let Some(numeric) = store.literal_adaptation_target(&resolved) {
                    if !self.is_inside_constant_expression()
                        && let Some(codepoint) = rune_codepoint(&char)
                    {
                        self.check_integer_literal_overflow(codepoint as u64, &numeric, span);
                    }
                    resolved.clone()
                } else {
                    let char_ty = Type::Simple(SimpleKind::Rune);
                    self.unify(expected_ty, &char_ty, &span);
                    char_ty
                };

                (Literal::Char(char), ty)
            }

            Literal::Slice(elements) => {
                let resolved = self.peel_resolved(expected_ty);
                let element_expected_ty = if let Type::Array { length, element } = &resolved {
                    if elements.len() as u64 != *length {
                        self.sink
                            .push(diagnostics::infer::array_literal_length_mismatch(
                                *length,
                                elements.len(),
                                span,
                            ));
                    }
                    element.as_ref().clone()
                } else if resolved.get_name() == Some("Slice") {
                    resolved
                        .inner()
                        .unwrap_or_else(|| self.new_type_var_with_hint("T"))
                } else {
                    self.new_type_var_with_hint("T")
                };

                let new_elements: Vec<Expression> = elements
                    .into_iter()
                    .map(|element| {
                        self.with_value_context(|this| {
                            this.infer_expression(element, &element_expected_ty)
                        })
                    })
                    .collect();
                let ty = if let Type::Array { length, .. } = &resolved {
                    self.type_array(*length, element_expected_ty)
                } else {
                    // A slice literal is fresh storage and grants write permission.
                    Type::qualified_compound(CompoundKind::Slice, vec![element_expected_ty], true)
                };
                let unified = self.unify(expected_ty, &ty, &span);
                if !matches!(resolved, Type::Array { .. }) && new_elements.is_empty() && unified {
                    let package_id = self.cursor.package_id().to_string();
                    self.facts.deferred.empty_literals.push(EmptyLiteralCheck {
                        ty: ty.clone(),
                        span,
                        package_id,
                    });
                }
                (Literal::Slice(new_elements), ty)
            }

            Literal::FormatString(parts) => {
                let is_single_expression = parts.len() == 1
                    && matches!(parts.first(), Some(FormatStringPart::Expression(_)));

                let new_parts: Vec<_> = parts
                    .into_iter()
                    .map(|part| match part {
                        FormatStringPart::Text(text) => FormatStringPart::Text(text),
                        FormatStringPart::Expression(expression) => {
                            let type_var = self.new_type_var();
                            let inferred_expression = self.infer_expression(*expression, &type_var);
                            FormatStringPart::Expression(Box::new(inferred_expression))
                        }
                    })
                    .collect();

                if is_single_expression
                    && let Some(FormatStringPart::Expression(expression)) = new_parts.first()
                    && expression.get_type().resolve_in(&self.env).is_string()
                {
                    self.facts
                        .add_expression_only_fstring(span, fstring_inner_needs_parens(expression));
                }

                let string_ty = Type::string();
                self.unify(expected_ty, &string_ty, &span);

                (Literal::FormatString(new_parts), string_ty)
            }
        };
        Expression::Literal { literal, ty, span }
    }

    fn hole_would_interpolate(&self, name: &str) -> bool {
        self.scopes.lookup_binding_id(name).is_some()
            && self
                .scopes
                .lookup_value(name)
                .is_some_and(|ty| self.store.is_interpolatable(&ty.resolve_in(&self.env)))
    }

    pub(super) fn infer_unit(&mut self, ty: Type, span: Span, expected_ty: &Type) -> Expression {
        if matches!(ty, Type::Error) {
            self.unify(expected_ty, &ty, &span);
            return Expression::Unit { ty, span };
        }

        let new_ty = self.new_type_var();
        let unit_ty = Type::unit();
        self.unify(&new_ty, &unit_ty, &span);
        self.unify(expected_ty, &new_ty, &span);
        Expression::Unit { ty: new_ty, span }
    }
}

/// Whether `expr` must be parenthesized to replace its f-string: true unless it
/// binds at least as tightly as a postfix operator.
fn fstring_inner_needs_parens(expression: &Expression) -> bool {
    match expression {
        Expression::Identifier { .. }
        | Expression::Literal { .. }
        | Expression::DotAccess { .. }
        | Expression::IndexedAccess { .. }
        | Expression::Paren { .. }
        | Expression::Propagate { .. } => false,
        Expression::Call { .. } => expression.is_piped_call(),
        _ => true,
    }
}

fn adapts_to_named_type(ty: &Type, store: &Store, kind: SimpleKind) -> bool {
    let peeled = store.peel_alias(ty);
    matches!(&peeled, Type::Nominal { id, .. }
        if store.is_nominal_defined_type(id.as_str())
            && store.underlying_simple_kind(&peeled) == Some(kind))
}
