use crate::checker::EnvResolve;
use crate::store::Store;
use ecow::EcoString;
use syntax::ast::BindingKind;
use syntax::ast::{Annotation, Binding, Expression, Literal, Span, Visibility};
use syntax::types::Type;

use super::super::TaskState;

enum ConstInitReject {
    NotSimple,
    Composite,
}

fn classify_const_init(expression: &Expression) -> Option<ConstInitReject> {
    match expression.unwrap_parens() {
        Expression::Literal { literal, .. } => match literal {
            Literal::Slice(_) => Some(ConstInitReject::Composite),
            Literal::FormatString(_) => Some(ConstInitReject::NotSimple),
            _ => None,
        },
        Expression::Identifier { .. } => None,
        Expression::Binary { left, right, .. } => {
            classify_const_init(left).or_else(|| classify_const_init(right))
        }
        Expression::Unary { expression, .. } => classify_const_init(expression),
        Expression::StructCall { .. } => Some(ConstInitReject::Composite),
        Expression::Tuple { .. } => Some(ConstInitReject::Composite),
        _ => Some(ConstInitReject::NotSimple),
    }
}

impl TaskState<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_const_binding(
        &mut self,
        store: &mut Store,
        doc: Option<String>,
        annotation: Option<Annotation>,
        expression: Box<Expression>,
        identifier: EcoString,
        identifier_span: Span,
        visibility: Visibility,
        span: Span,
    ) -> Expression {
        let ty = if let Some(annotation) = &annotation {
            self.convert_to_type(store, annotation, &span)
        } else {
            // Look up the type variable that was created during registration.
            // This ensures the type variable in the store gets unified.
            self.lookup_type(store, &identifier)
                .unwrap_or_else(|| self.new_type_var())
        };

        let new_expression = self.infer_expression(store, *expression, &ty);

        match classify_const_init(&new_expression) {
            None => {}
            Some(ConstInitReject::NotSimple) => {
                self.sink
                    .push(diagnostics::infer::const_requires_simple_expression(
                        new_expression.get_span(),
                    ));
            }
            Some(ConstInitReject::Composite) => {
                self.sink
                    .push(diagnostics::infer::const_disallows_composite(
                        new_expression.get_span(),
                    ));
            }
        }

        Expression::Const {
            doc,
            identifier,
            identifier_span,
            expression: new_expression.into(),
            annotation,
            ty,
            span,
            visibility,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_let_binding(
        &mut self,
        store: &mut Store,
        binding: Binding,
        value: Box<Expression>,
        mutable: bool,
        mut_span: Option<Span>,
        else_block: Option<Box<Expression>>,
        else_span: Option<Span>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let has_annotation = binding.annotation.is_some();
        let binding_name = binding.pattern.get_identifier();

        let ty = if let Some(annotation) = &binding.annotation {
            self.convert_to_type(store, annotation, &span)
        } else {
            self.new_type_var()
        };

        let new_value = self.with_value_context(|s| s.infer_expression(store, *value, &ty));

        let new_else_block = if let Some(else_expression) = else_block {
            let else_ty = self.new_type_var();
            let new_else = self.infer_expression(store, *else_expression, &else_ty);

            let resolved_else_ty = else_ty.resolve_in(&self.env);
            if new_else.diverges().is_none() && !resolved_else_ty.is_never() {
                let error_span = else_span.expect("let-else must have else_span");
                self.sink
                    .push(diagnostics::infer::let_else_must_diverge(error_span));
            }
            let never_ty = self.type_never();
            self.unify(store, &else_ty, &never_ty, &span);

            Some(Box::new(new_else))
        } else {
            None
        };

        let (inferred_pattern, typed_pattern) = self.infer_pattern(
            store,
            binding.pattern,
            ty.clone(),
            BindingKind::Let { mutable },
        );

        let new_binding = Binding {
            pattern: inferred_pattern,
            annotation: binding.annotation,
            typed_pattern: Some(typed_pattern.clone()),
            ty: ty.clone(),
            mutable: false,
        };

        if !has_annotation
            && new_value.is_empty_collection()
            && let Some(name) = binding_name
        {
            self.facts
                .empty_collection_checks
                .push(crate::facts::EmptyCollectionCheck {
                    name: name.to_string(),
                    ty: new_binding.ty.clone(),
                    span,
                });
        }

        if mutable && !new_binding.pattern.is_identifier() {
            self.sink.push(diagnostics::infer::disallowed_mut_use(
                mut_span.unwrap_or(span),
            ));
        }

        let unit_ty = self.type_unit();
        self.unify(store, expected_ty, &unit_ty, &span);

        Expression::Let {
            binding: Box::new(new_binding),
            value: new_value.into(),
            mutable,
            mut_span,
            else_block: new_else_block,
            else_span,
            typed_pattern: Some(typed_pattern),
            ty: self.type_unit(),
            span,
        }
    }
}
