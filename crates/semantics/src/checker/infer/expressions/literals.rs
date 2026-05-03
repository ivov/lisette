use crate::checker::EnvResolve;
use crate::store::Store;
use rustc_hash::FxHashSet;
use syntax::ast::{Expression, FormatStringPart, Literal, Span};
use syntax::program::DefinitionBody;
use syntax::types::{SubstitutionMap, Symbol, Type, substitute};

use super::super::TaskState;

impl TaskState<'_> {
    pub(super) fn infer_literal(
        &mut self,
        store: &mut Store,
        literal: Literal,
        expected_ty: &Type,
        span: Span,
    ) -> Expression {
        match literal {
            Literal::Boolean(boolean) => {
                let bool_ty = self.type_bool();
                self.unify(store, expected_ty, &bool_ty, &span);

                Expression::Literal {
                    literal: Literal::Boolean(boolean),
                    ty: bool_ty,
                    span,
                }
            }

            Literal::Integer { value, text } => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if let Some(numeric) = numeric_adapt_target(&resolved, store) {
                    let is_pre_negated = text.as_deref().is_some_and(|t| t.starts_with('-'));
                    if is_pre_negated {
                        self.check_negative_magnitude_overflow(
                            value.wrapping_neg(),
                            &numeric,
                            span,
                        );
                    } else if !self.scopes.is_inside_negation() {
                        self.check_integer_literal_overflow(value, &numeric, span);
                    }
                    resolved.clone()
                } else {
                    let int_ty = self.type_int();
                    self.unify(store, expected_ty, &int_ty, &span);
                    int_ty
                };

                Expression::Literal {
                    literal: Literal::Integer { value, text },
                    ty,
                    span,
                }
            }

            Literal::Float { value, text } => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if numeric_adapt_target(&resolved, store).is_some_and(|n| n.is_float()) {
                    self.check_float_literal_overflow(value, &resolved, span);
                    resolved.clone()
                } else {
                    let float_ty = self.type_float();
                    self.unify(store, expected_ty, &float_ty, &span);
                    float_ty
                };

                Expression::Literal {
                    literal: Literal::Float { value, text },
                    ty,
                    span,
                }
            }

            Literal::Imaginary(coef) => {
                let complex_ty = self.type_complex128();
                self.unify(store, expected_ty, &complex_ty, &span);

                Expression::Literal {
                    literal: Literal::Imaginary(coef),
                    ty: complex_ty,
                    span,
                }
            }

            Literal::String { value, raw } => {
                let string_ty = self.type_string();
                self.unify(store, expected_ty, &string_ty, &span);

                Expression::Literal {
                    literal: Literal::String { value, raw },
                    ty: string_ty,
                    span,
                }
            }

            Literal::Char(char) => {
                let resolved = expected_ty.resolve_in(&self.env);
                let ty = if let Some(numeric) = numeric_adapt_target(&resolved, store) {
                    if let Some(codepoint) = char_literal_codepoint(&char) {
                        self.check_integer_literal_overflow(codepoint, &numeric, span);
                    }
                    resolved.clone()
                } else {
                    let char_ty = self.type_char();
                    self.unify(store, expected_ty, &char_ty, &span);
                    char_ty
                };

                Expression::Literal {
                    literal: Literal::Char(char),
                    ty,
                    span,
                }
            }

            Literal::Slice(elements) => {
                // If expected type is Slice<T>, propagate T to element inference
                // so literals can adapt (e.g., `let x: Slice<int8> = [1, 2, 3]` works)
                let resolved = expected_ty.resolve_in(&self.env);
                let element_expected_ty = if resolved.get_name() == Some("Slice") {
                    resolved
                        .inner()
                        .unwrap_or_else(|| self.new_type_var_with_hint("T"))
                } else {
                    self.new_type_var_with_hint("T")
                };

                let new_elements: Vec<Expression> = elements
                    .into_iter()
                    .map(|e| {
                        self.with_value_context(|s| {
                            s.infer_expression(store, e, &element_expected_ty)
                        })
                    })
                    .collect();

                let slice_ty = self.type_slice(element_expected_ty);
                self.unify(store, expected_ty, &slice_ty, &span);

                Expression::Literal {
                    literal: Literal::Slice(new_elements),
                    ty: slice_ty,
                    span,
                }
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
                            let inferred_expression =
                                self.infer_expression(store, *expression, &type_var);
                            FormatStringPart::Expression(Box::new(inferred_expression))
                        }
                    })
                    .collect();

                if is_single_expression
                    && let Some(FormatStringPart::Expression(expression)) = new_parts.first()
                    && expression.get_type().resolve_in(&self.env).is_string()
                {
                    self.facts.add_expression_only_fstring(span);
                }

                let string_ty = self.type_string();
                self.unify(store, expected_ty, &string_ty, &span);

                Expression::Literal {
                    literal: Literal::FormatString(new_parts),
                    ty: string_ty,
                    span,
                }
            }
        }
    }

    pub(super) fn infer_unit(
        &mut self,
        store: &mut Store,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let new_ty = self.new_type_var();
        let unit_ty = self.type_unit();
        self.unify(store, &new_ty, &unit_ty, &span);
        self.unify(store, expected_ty, &new_ty, &span);
        Expression::Unit { ty: new_ty, span }
    }
}

/// Walks the alias chain through the store rather than the cached
/// `underlying_ty` field so multi-hop aliases (with forward-declared
/// intermediates) resolve. Rejects when any nominal in the chain is a value
/// enum — those are a hard boundary requiring an explicit `as` cast.
fn numeric_adapt_target(ty: &Type, store: &Store) -> Option<Type> {
    let mut current = ty.clone();
    let mut seen: FxHashSet<Symbol> = FxHashSet::default();
    while let Type::Nominal { id, params, .. } = &current {
        if !seen.insert(id.clone()) {
            break;
        }
        if store.value_variants_of(id).is_some() {
            return None;
        }
        let Some(def) = store.get_definition(id.as_str()) else {
            break;
        };
        if !matches!(def.body, DefinitionBody::TypeAlias { .. }) {
            break;
        }
        let def_ty = &def.ty;
        let (vars, body) = match def_ty {
            Type::Forall { vars, body } => (vars.clone(), body.as_ref().clone()),
            other => (vec![], other.clone()),
        };
        let map: SubstitutionMap = vars.iter().cloned().zip(params.iter().cloned()).collect();
        current = substitute(&body, &map);
    }
    current.underlying_numeric_type()
}

fn char_literal_codepoint(s: &str) -> Option<u64> {
    if let Some(rest) = s.strip_prefix('\\') {
        match rest.as_bytes().first()? {
            b'n' => Some(10),
            b't' => Some(9),
            b'r' => Some(13),
            b'0' => Some(0),
            b'\\' => Some(92),
            b'\'' => Some(39),
            b'x' => u64::from_str_radix(&rest[1..], 16).ok(),
            _ => None,
        }
    } else {
        s.chars().next().map(|c| c as u64)
    }
}
