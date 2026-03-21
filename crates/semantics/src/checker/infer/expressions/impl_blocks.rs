use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;
use syntax::ast::{Annotation, Expression, Generic, ParentInterface, Span};
use syntax::program::Definition;
use syntax::types::Type;

use super::super::Checker;
use super::super::checks::check_receiver;

impl Checker<'_, '_> {
    pub(super) fn infer_impl_block(
        &mut self,
        annotation: Annotation,
        methods: Vec<Expression>,
        receiver_name: EcoString,
        generics: Vec<Generic>,
        span: Span,
    ) -> Expression {
        self.scopes.push();

        self.put_in_scope(&generics);

        for g in &generics {
            let qualified_name = self.qualify_name(&g.name);
            for b in &g.bounds {
                let bound_ty = self.convert_to_type(b, &span);
                self.scopes
                    .current_mut()
                    .trait_bounds
                    .get_or_insert_with(HashMap::default)
                    .entry(qualified_name.clone())
                    .or_default()
                    .push(bound_ty);
            }
        }

        self.check_undeclared_impl_type_params(&annotation, &generics);
        let impl_ty = self.convert_to_type(&annotation, &span);

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
        if let Type::Constructor { id, .. } = &impl_ty
            && let Some(Definition::Struct {
                constructor: Some(ctor_ty),
                ..
            }) = self.store.get_definition(id)
        {
            let ctor_ty = ctor_ty.clone();
            self.scopes
                .current_mut()
                .values
                .insert(receiver_name.to_string(), ctor_ty);
        }

        self.inference.impl_receiver_type = Some(impl_ty.clone());

        let new_methods: Vec<Expression> = methods
            .into_iter()
            .map(|method| {
                let method_ty = self.new_type_var();
                let inferred = self.infer_expression(method, &method_ty);
                check_receiver(self.sink, &inferred, &impl_ty);
                inferred
            })
            .collect();

        self.inference.impl_receiver_type = None;
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

        self.check_prelude_shadowing(&name, name_span);

        self.scopes.push();
        self.put_in_scope(&generics);

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
                let parent_ty = self.convert_to_type(&parent.annotation, &parent.span);
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
