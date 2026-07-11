use ecow::EcoString;
use syntax::ast::{Annotation, Expression, Generic, ParentInterface, Span};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::Type;

use crate::checker::infer::InferCtx;

impl InferCtx<'_, '_> {
    pub(super) fn infer_impl_block(
        &mut self,
        annotation: Annotation,
        methods: Vec<Expression>,
        receiver_name: EcoString,
        generics: Vec<Generic>,
        span: Span,
    ) -> Expression {
        let store = self.store;
        self.scopes.push();

        self.put_in_scope(&generics);

        for generic in &generics {
            for bound in &generic.bounds {
                self.register_generic_bound(store, &generic.name, bound, &span);
            }
        }

        self.check_undeclared_impl_type_params(&annotation, &generics);
        let impl_ty = self.convert_to_type_inner(store, &annotation, &span, false, false);

        if self.impl_has_simple_type_params(&impl_ty, &generics) {
            let receiver_qualified = impl_ty.get_qualified_name();
            self.register_receiver_type_bounds(store, &receiver_qualified, &generics);
        }

        let receiver_ty = if generics.is_empty() {
            impl_ty.clone()
        } else {
            Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(impl_ty.clone()),
            }
        };

        let scope = self.scopes.current_mut();
        scope.values.insert(receiver_name.to_string(), receiver_ty);

        // If this is a tuple struct with a constructor, the receiver_name (which is the
        // type name) shadows the constructor function in the parent scope. Re-insert the
        // constructor so it's callable from within impl methods.
        if let Type::Nominal { id, .. } = &impl_ty
            && let Some(Definition {
                body:
                    DefinitionBody::Struct {
                        constructor: Some(ctor_ty),
                        ..
                    },
                ..
            }) = store.get_definition(id)
        {
            let ctor_ty = ctor_ty.clone();
            self.scopes
                .current_mut()
                .values
                .insert(receiver_name.to_string(), ctor_ty);
        }

        self.scopes.set_impl_receiver_type(Some(impl_ty.clone()));

        let new_methods: Vec<Expression> = methods
            .into_iter()
            .map(|method| {
                let method_ty = self.new_type_var();
                self.infer_expression(method, &method_ty)
            })
            .collect();

        self.scopes.set_impl_receiver_type(None);
        self.scopes.pop();

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

        self.scopes.push();
        self.put_in_scope(&generics);
        self.validate_generic_bounds(store, &generics, &span);

        // Interface method parameters are declarations, not implementations — they
        // have no body and are always "unused". Remove their bindings so the unused
        // parameter lint doesn't fire (e.g., `self` would otherwise trigger it).
        let checkpoint = self.facts.binding_checkpoint();
        let new_method_signatures = method_signatures
            .into_iter()
            .map(|method_signature| {
                let signature_ty = self.new_type_var();
                self.infer_expression(method_signature, &signature_ty)
            })
            .collect();
        self.facts.remove_bindings_from(checkpoint);

        let new_parents = parents
            .into_iter()
            .map(|parent| {
                let parent_ty = self.convert_to_type(store, &parent.annotation, &parent.span);
                ParentInterface {
                    annotation: parent.annotation,
                    span: parent.span,
                    ty: parent_ty,
                }
            })
            .collect();

        self.scopes.pop();

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
}
