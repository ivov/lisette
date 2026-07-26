use ecow::EcoString;
use syntax::ast::{Annotation, Expression, Generic, ParentInterface, Span};
use syntax::program::Definition;
use syntax::types::Type;

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(super) fn infer_impl_block(
        &mut self,
        annotation: Annotation,
        methods: Vec<Expression>,
        receiver_name: EcoString,
        generics: Vec<Generic>,
        span: Span,
    ) -> Expression {
        let store = self.store;
        let (generics, impl_ty, new_methods) = self.with_scope(|this| {
            this.put_in_scope(&generics);
            let generics = this.ensure_generic_bounds(store, generics, &span);

            this.check_undeclared_impl_type_params(&annotation, &generics);
            let impl_ty = this.convert_receiver_to_type(store, &annotation, &span);

            if let Type::Nominal { id, .. } = &impl_ty
                && this.impl_has_simple_type_params(&impl_ty, &generics)
            {
                let receiver_qualified = id.clone();
                this.register_receiver_type_bounds(store, &receiver_qualified, &generics);
            }

            let receiver_ty = if generics.is_empty() {
                impl_ty.clone()
            } else {
                Type::Forall {
                    vars: generics.iter().map(|g| g.name.clone()).collect(),
                    body: Box::new(impl_ty.clone()),
                }
            };

            let scope = this.scopes.current_mut();
            scope.insert_value(receiver_name.to_string(), receiver_ty);

            if let Type::Nominal { id, .. } = &impl_ty
                && let Some(ctor_ty) = store
                    .get_definition(id)
                    .and_then(Definition::constructor_type)
            {
                this.scopes
                    .current_mut()
                    .insert_value(receiver_name.to_string(), ctor_ty);
            }

            this.scopes.set_impl_receiver_type(impl_ty.clone());

            let new_methods = methods
                .into_iter()
                .map(|method| {
                    let method_ty = this.new_type_var();
                    this.infer_expression(method, &method_ty)
                })
                .collect();
            (generics, impl_ty, new_methods)
        });

        Expression::ImplBlock {
            annotation,
            ty: impl_ty,
            receiver_name,
            methods: new_methods,
            generics,
            span,
        }
    }

    pub(super) fn infer_interface(&mut self, expression: Expression) -> Expression {
        let store = self.store;
        let Expression::Interface {
            doc,
            name,
            name_span,
            generics,
            method_signatures,
            parents,
            visibility,
            span,
        } = expression
        else {
            unreachable!()
        };

        let (generics, new_method_signatures, new_parents) = self.with_scope(|this| {
            this.put_in_scope(&generics);
            let generics = this.ensure_generic_bounds(store, generics, &span);

            let new_method_signatures = this.with_temporary_bindings(|this| {
                method_signatures
                    .into_iter()
                    .map(|method_signature| {
                        let signature_ty = this.new_type_var();
                        this.infer_expression(method_signature, &signature_ty)
                    })
                    .collect()
            });

            let new_parents = parents
                .into_iter()
                .map(|parent| {
                    let parent_ty = this.without_diagnostics(|this| {
                        this.convert_to_type(store, &parent.annotation, &parent.span)
                    });
                    this.check_interface_parent(&parent_ty, parent.span);
                    ParentInterface {
                        annotation: parent.annotation,
                        span: parent.span,
                        ty: parent_ty,
                    }
                })
                .collect();
            (generics, new_method_signatures, new_parents)
        });

        Expression::Interface {
            doc,
            name,
            name_span,
            generics,
            method_signatures: new_method_signatures,
            parents: new_parents,
            span,
            visibility,
        }
    }

    fn check_interface_parent(&mut self, parent_ty: &Type, span: Span) {
        if parent_ty.is_error() {
            return;
        }
        let (core, _) = self.store.peel_refs_and_aliases(parent_ty);
        if self.store.is_interface(&core) {
            return;
        }
        self.sink.push(diagnostics::embed::non_interface_parent(
            &parent_ty.to_string(),
            span,
        ));
    }
}
