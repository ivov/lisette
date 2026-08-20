use crate::checker::EnvResolve;
use syntax::ast::BindingKind;
use syntax::ast::{Binding, Expression, Literal};
use syntax::program::DefinitionBody;
use syntax::types::{Symbol, Type};

use crate::checker::infer::InferCtx;
use crate::checker::infer::context::{BindingInference, LetInference, LetMutability};
use crate::facts::EmptyCollectionCheck;
use crate::loader;

enum ConstInitReject {
    NotSimple,
    Composite,
}

impl InferCtx<'_> {
    fn classify_const_init(&self, expression: &Expression) -> Option<ConstInitReject> {
        match expression.unwrap_parens() {
            Expression::Literal { literal, .. } => match literal {
                Literal::Slice(_) => Some(ConstInitReject::Composite),
                Literal::FormatString(_) => Some(ConstInitReject::NotSimple),
                _ => None,
            },
            Expression::Identifier { .. } => None,
            Expression::Binary { left, right, .. } => self
                .classify_const_init(left)
                .or_else(|| self.classify_const_init(right)),
            Expression::Unary { expression, .. } => self.classify_const_init(expression),
            Expression::StructCall { .. } => Some(ConstInitReject::Composite),
            Expression::Tuple { .. } => Some(ConstInitReject::Composite),
            dot if self.is_const_member_access(dot) => None,
            _ => Some(ConstInitReject::NotSimple),
        }
    }

    fn is_const_member_access(&self, expression: &Expression) -> bool {
        let Expression::DotAccess {
            expression: inner,
            member,
            ..
        } = expression
        else {
            return false;
        };
        let inner_ty = inner.get_type();
        let Some(package_id) = inner_ty.as_import_namespace() else {
            return false;
        };
        let qualified = Symbol::from_parts(package_id, member.as_str());
        self.store.is_const(qualified.as_str())
    }

    pub(super) fn infer_const_binding(&mut self, expression: Expression) -> Expression {
        let Expression::Const {
            doc,
            annotation,
            expression,
            identifier,
            identifier_span,
            visibility,
            span,
            ..
        } = expression
        else {
            unreachable!("infer_const_binding called with non-Const expression");
        };
        let store = self.store;
        let ty = if let Some(annotation) = &annotation {
            let ty = self.convert_to_type(store, annotation, &span);
            if self.is_lis(store) && store.contains_unknown(&ty) {
                self.sink
                    .push(diagnostics::infer::unknown_in_const_annotation(
                        annotation.get_span(),
                    ));
            }
            ty
        } else {
            // Look up the type variable that was created during registration.
            // This ensures the type variable in the store gets unified.
            self.lookup_type(store, &identifier)
                .unwrap_or_else(|| self.new_type_var())
        };

        let new_expression = expression.map_value(|expression| {
            let expression = self.infer_expression(expression, &ty);
            match self.classify_const_init(&expression) {
                None => {}
                Some(ConstInitReject::NotSimple) => {
                    self.sink
                        .push(diagnostics::infer::const_requires_simple_expression(
                            expression.get_span(),
                        ));
                }
                Some(ConstInitReject::Composite) => {
                    self.sink
                        .push(diagnostics::infer::const_disallows_composite(
                            expression.get_span(),
                        ));
                }
            }
            expression
        });

        Expression::Const {
            doc,
            identifier,
            identifier_span,
            expression: new_expression,
            annotation,
            ty,
            span,
            visibility,
        }
    }

    pub(super) fn infer_let_binding(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        let Expression::Let {
            binding,
            value,
            mode,
            span,
            ..
        } = expression
        else {
            unreachable!("infer_let_binding called with non-Let expression");
        };
        let binding = *binding;
        let mutable = binding.is_mutable();
        let mut_span = binding.mut_span;
        let store = self.store;
        let has_annotation = binding.annotation.is_some();
        let binding_name = binding.pattern.get_identifier();
        let pattern_span = binding.pattern.get_span();

        let is_assert = mode.is_assert();
        if is_assert && !self.scopes.has_test_handle() {
            self.sink
                .push(diagnostics::infer::assert_without_test_context(
                    pattern_span,
                ));
        }

        let ty = if let Some(annotation) = &binding.annotation {
            self.convert_to_type(store, annotation, &span)
        } else {
            self.new_type_var()
        };

        let new_value = self.with_let_binding_rhs(|state| {
            state.with_value_context(|state| state.infer_expression(*value, &ty))
        });

        let new_mode = mode.map_else(|else_expression, else_span| {
            let else_ty = self.new_type_var();
            let new_else = self.infer_expression(else_expression, &else_ty);

            let resolved_else_ty = else_ty.resolve_in(&self.env);
            if new_else.diverges().is_none() && !resolved_else_ty.is_never() {
                self.sink
                    .push(diagnostics::infer::let_else_must_diverge(else_span));
            }
            let never_ty = self.type_never();
            self.unify(&else_ty, &never_ty, &span);

            new_else
        });

        // Destructured components keep their own permission, `mut` is not
        // spellable on a pattern.
        let mut demoted_from_writable = false;
        let binding_ty = if mutable || has_annotation || !binding.pattern.is_identifier() {
            ty.clone()
        } else {
            let resolved = ty.resolve_in(&self.env);
            demoted_from_writable = self.store.peel_alias(&resolved).is_writable();
            self.store.demoted_at_binding(&resolved)
        };

        if mutable
            && binding_ty.resolve_in(&self.env).is_writable()
            && let Some(root) = super::aliasing::place_root_name(new_value.unwrap_parens())
            && let Some(binding_id) = self.scopes.lookup_binding_id(&root)
        {
            self.facts.mark_alias_mutated(binding_id);
        }

        let is_plain_identifier = binding.pattern.is_identifier();
        if !is_plain_identifier {
            self.mark_scrutinee_grant(&new_value, &binding_ty.resolve_in(&self.env));
        }
        let inferred_pattern = self.infer_pattern(
            binding.pattern,
            binding_ty.clone(),
            BindingKind::Let { mutable },
        );

        if is_plain_identifier
            && let Some(name) = inferred_pattern.get_identifier()
            && let Some(binding_id) = self.scopes.lookup_binding_id(&name)
        {
            let mutability = if mutable || is_assert {
                LetMutability::NoFix
            } else if demoted_from_writable {
                LetMutability::RestoreWriteWithMut
            } else {
                LetMutability::AddMut
            };
            let source = self.value_source(new_value.unwrap_parens(), &name);
            self.binding_inference.insert(
                binding_id,
                BindingInference::Let(LetInference { mutability, source }),
            );
        }

        let new_binding = Binding {
            pattern: inferred_pattern,
            annotation: binding.annotation,
            ty: binding_ty,
            mut_span,
        };

        if !has_annotation
            && new_value.is_empty_collection()
            && let Some(ref name) = binding_name
        {
            let package_id = self.cursor.package_id().to_string();
            self.facts
                .deferred
                .empty_collections
                .push(EmptyCollectionCheck {
                    name: name.to_string(),
                    ty: new_binding.ty.clone(),
                    span,
                    package_id,
                });
        }

        if mutable && !new_binding.pattern.is_identifier() {
            self.sink.push(diagnostics::infer::disallowed_mut_use(
                mut_span.unwrap_or(span),
            ));
        }

        // Reject enum type bindings: `let c = utils.Color`
        if let Expression::DotAccess {
            expression: inner,
            member,
            ..
        } = &new_value
        {
            let inner_ty = inner.get_type();
            if let Some(package_id) = inner_ty.as_import_namespace() {
                let qualified = Symbol::from_parts(package_id, member.as_str());
                if matches!(
                    store.get_definition(&qualified).map(|d| &d.body),
                    Some(DefinitionBody::Enum { .. })
                ) {
                    let type_name =
                        format!("{}.{}", loader::import_display_name(package_id), member);
                    self.sink.push(diagnostics::infer::let_binding_enum_type(
                        &type_name,
                        new_value.get_span(),
                    ));
                }
            }
        }

        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        Expression::Let {
            binding: Box::new(new_binding),
            value: new_value.into(),
            mode: new_mode,
            ty: self.type_unit(),
            span,
        }
    }
}
