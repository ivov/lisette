use crate::checker::{EnvResolve, resolved_generic_bounds};
use syntax::ast::{Annotation, Binding, BindingKind, Expression, Pattern, Span};
use syntax::types::{FunctionParameter, Type};

use crate::analysis::ProjectKind;
use crate::checker::infer::InferCtx;
use crate::checker::registration::test_functions::normalize_test_params;
use crate::store::ENTRY_MODULE_ID;

impl InferCtx<'_> {
    fn ty_is_test_context(&self, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env).strip_refs();
        resolved.get_qualified_id().is_some_and(|id| {
            id.strip_suffix(".TestContext")
                .is_some_and(|module| module == crate::prelude::TEST_PRELUDE_MODULE_ID)
        })
    }

    fn param_provides_test_handle(&self, param: &Binding) -> bool {
        matches!(&param.pattern, Pattern::Identifier { identifier, .. } if identifier != "_")
            && self.ty_is_test_context(&param.ty)
    }

    fn mark_test_context_params_used(&mut self, params: &[Binding]) {
        for param in params {
            if let Pattern::Identifier { identifier, .. } = &param.pattern
                && self.param_provides_test_handle(param)
                && let Some(id) = self.scopes.lookup_binding_id(identifier)
            {
                self.facts.mark_used(id);
            }
        }
    }

    pub(super) fn infer_function(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        let Expression::Function {
            doc,
            attributes,
            name,
            name_span,
            generics,
            params,
            return_annotation,
            visibility,
            body,
            span,
            ..
        } = expression
        else {
            unreachable!("infer_function called with non-Function expression");
        };

        if self.scopes.lookup_fn_return_type().is_some() {
            self.sink
                .push(diagnostics::infer::nested_function(name_span));
        }

        if name == "main"
            && self.project_kind == ProjectKind::Binary
            && self.cursor.module_id == ENTRY_MODULE_ID
            && (!params.is_empty() || return_annotation != Annotation::Unknown)
        {
            self.sink
                .push(diagnostics::infer::invalid_main_signature(name_span));
        }

        let (generics, new_params, return_ty, base_fn_ty, new_body) = self.with_scope(|this| {
            this.put_in_scope(&generics);
            let generics = this.ensure_generic_bounds(store, generics, &span);
            let bounds = resolved_generic_bounds(&generics);

            let resolved_expected = expected_ty.resolve_in(&this.env);
            let expected_function = store.resolve_to_function_type(&resolved_expected);
            let expected_params = expected_function
                .as_ref()
                .and_then(Type::get_function_params)
                .unwrap_or_default();
            let is_test = attributes.iter().any(|a| a.name == "test");
            let params = normalize_test_params(params, is_test);
            let new_params = this.infer_function_params(params, expected_params, true);

            if is_test {
                this.scopes.set_test_fn_name(name.clone());
            } else if new_params
                .iter()
                .any(|param| this.param_provides_test_handle(param))
            {
                this.scopes.mark_test_handle();
            }
            this.mark_test_context_params_used(&new_params);

            let unit_ty = this.type_unit();
            let return_ty =
                this.infer_return_type(&return_annotation, &resolved_expected, &span, unit_ty);

            this.scopes.current_mut().fn_return_type = Some(return_ty.clone());

            let base_fn_ty = Type::function(
                new_params
                    .iter()
                    .map(|param| {
                        FunctionParameter::named(
                            param.ty.clone(),
                            param.pattern.get_identifier(),
                            param.is_mutable(),
                        )
                    })
                    .collect(),
                bounds,
                return_ty.clone().into(),
            );

            let has_implicit_unit_return = return_annotation == Annotation::Unknown;
            let body_ty = if has_implicit_unit_return {
                Type::ignored()
            } else {
                return_ty.clone()
            };

            let new_body = body.map_definition(|body| {
                this.infer_function_body(Box::new(body), &body_ty, &return_annotation, &return_ty)
            });

            this.check_deferred_map_key_bounds(store);
            (generics, new_params, return_ty, base_fn_ty, new_body)
        });

        let fn_ty = if generics.is_empty() {
            base_fn_ty
        } else {
            let fn_forall_ty = Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(base_fn_ty),
            };
            self.instantiate(&fn_forall_ty).0
        };

        self.unify(expected_ty, &fn_ty, &span);

        self.facts.add_function_span(span);

        Expression::Function {
            doc,
            attributes,
            name,
            name_span,
            generics,
            params: new_params,
            return_annotation,
            return_type: return_ty,
            visibility,
            body: new_body,
            ty: fn_ty,
            span,
        }
    }

    pub(super) fn infer_lambda(
        &mut self,
        params: Vec<Binding>,
        return_annotation: Annotation,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        let (new_params, base_fn_ty, new_body) = self.with_scope(|this| {
            this.scopes.mark_lambda_scope();
            let resolved_expected = expected_ty.resolve_in(&this.env);
            let expected_function = store.resolve_to_function_type(&resolved_expected);
            let expected_params = expected_function
                .as_ref()
                .and_then(Type::get_function_params)
                .unwrap_or_default();
            let new_params = this.infer_function_params(params, expected_params, false);

            if new_params
                .iter()
                .any(|param| this.param_provides_test_handle(param))
            {
                this.scopes.mark_test_handle();
            }
            this.mark_test_context_params_used(&new_params);

            let default_return = this.new_type_var();
            let return_ty = this.infer_return_type(
                &return_annotation,
                &resolved_expected,
                &span,
                default_return,
            );

            this.scopes.current_mut().fn_return_type = Some(return_ty.clone());

            let base_fn_ty = Type::function(
                new_params
                    .iter()
                    .map(|param| {
                        FunctionParameter::named(
                            param.ty.clone(),
                            param.pattern.get_identifier(),
                            param.is_mutable(),
                        )
                    })
                    .collect(),
                vec![],
                return_ty.clone().into(),
            );

            let relax_body_to_unit =
                return_annotation == Annotation::Unknown && return_ty.is_unit();
            let body_ty = if relax_body_to_unit {
                Type::ignored()
            } else {
                return_ty.clone()
            };
            let new_body = this.without_enclosing_loop(|this| {
                this.infer_function_body(body, &body_ty, &return_annotation, &return_ty)
            });

            this.check_deferred_map_key_bounds(store);
            (new_params, base_fn_ty, new_body)
        });

        self.unify(expected_ty, &base_fn_ty, &span);

        Expression::Lambda {
            params: new_params,
            return_annotation,
            body: new_body.into(),
            ty: base_fn_ty,
            span,
        }
    }

    fn infer_function_body(
        &mut self,
        body: Box<Expression>,
        body_ty: &Type,
        return_annotation: &Annotation,
        return_ty: &Type,
    ) -> Expression {
        if let Expression::Block {
            items,
            span: body_span,
            ..
        } = body.as_ref()
            && items.is_empty()
            && *return_annotation != Annotation::Unknown
            && !return_ty.is_unit()
        {
            self.sink
                .push(diagnostics::infer::empty_body_return_mismatch(
                    return_ty,
                    return_annotation.get_span(),
                ));
            return Expression::Block {
                items: vec![],
                ty: self.type_unit(),
                span: *body_span,
            };
        }

        self.infer_expression(*body, body_ty)
    }

    fn infer_function_params(
        &mut self,
        params: Vec<Binding>,
        expected_params: &[FunctionParameter],
        handle_self_receiver: bool,
    ) -> Vec<Binding> {
        let store = self.store;

        // `VarArgs<T>` must be the last function parameter
        if let Some((_last, leading)) = params.split_last() {
            for binding in leading {
                if let Some(annotation @ Annotation::Constructor { name, .. }) = &binding.annotation
                    && name == "VarArgs"
                {
                    self.sink.push(diagnostics::infer::variadic_param_not_last(
                        annotation.get_span(),
                    ));
                }
            }
        }

        params
            .into_iter()
            .enumerate()
            .map(|(index, binding)| {
                let expected_param_ty = match binding.annotation {
                    // A `#[test]` handle carries a resolved type with no
                    // annotation. Honor it before falling back to the expected
                    // function type.
                    None if !binding.ty.is_uninferred() => Some(binding.ty.clone()),
                    None => expected_params.get(index).map(|param| param.ty.clone()),
                    _ => None,
                };

                let binding_ty = expected_param_ty.unwrap_or_else(|| {
                    let pattern_span = &binding.pattern.get_span();

                    if handle_self_receiver
                        && let Pattern::Identifier { identifier, .. } = &binding.pattern
                        && identifier == "self"
                        && binding.annotation.is_none()
                        && let Some(impl_ty) = self.scopes.impl_receiver_type()
                    {
                        return impl_ty.clone();
                    }

                    binding
                        .annotation
                        .as_ref()
                        .map(|a| self.convert_variadic_to_type(store, a, pattern_span))
                        .unwrap_or_else(|| self.new_type_var())
                });

                let mutable = binding.is_mutable();
                let new_pattern = self.infer_pattern(
                    binding.pattern,
                    binding_ty.clone(),
                    BindingKind::Parameter { mutable },
                );

                Binding {
                    pattern: new_pattern,
                    annotation: binding.annotation,
                    ty: binding_ty,
                    mut_span: binding.mut_span,
                }
            })
            .collect()
    }

    fn infer_return_type(
        &mut self,
        annotation: &Annotation,
        expected_ty: &Type,
        span: &Span,
        default_for_unknown: Type,
    ) -> Type {
        let store = self.store;
        match annotation {
            Annotation::Unknown => {
                let expected_function = store.resolve_to_function_type(expected_ty);
                if let Type::Function(f) = expected_function.as_ref().unwrap_or(expected_ty) {
                    (*f.return_type).clone()
                } else {
                    default_for_unknown
                }
            }
            _ => self.convert_to_type(store, annotation, span),
        }
    }
}
