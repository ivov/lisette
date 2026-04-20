use ecow::EcoString;
use syntax::ast::BindingKind;
use syntax::ast::{Annotation, Binding, Expression, Pattern, Span, Visibility};
use syntax::types::Type;

use super::super::Checker;
use super::super::checks::{check_binding_pattern, reject_as_binding_in_irrefutable_context};
use crate::checker::PostInferenceCheck;

fn is_const_eligible(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal { .. } => true,
        Expression::Identifier { .. } => true,
        Expression::Binary { left, right, .. } => {
            is_const_eligible(left) && is_const_eligible(right)
        }
        Expression::Unary { expression, .. } => is_const_eligible(expression),
        Expression::StructCall { .. } => true,
        Expression::Tuple { elements, .. } => elements.iter().all(is_const_eligible),
        _ => false,
    }
}

impl Checker<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_const_binding(
        &mut self,
        doc: Option<String>,
        annotation: Option<Annotation>,
        expression: Box<Expression>,
        identifier: EcoString,
        identifier_span: Span,
        visibility: Visibility,
        span: Span,
    ) -> Expression {
        let ty = if let Some(annotation) = &annotation {
            self.convert_to_type(annotation, &span)
        } else {
            // Look up the type variable that was created during registration.
            // This ensures the type variable in the store gets unified.
            self.lookup_type(&identifier)
                .unwrap_or_else(|| self.new_type_var())
        };

        let new_expression = self.infer_expression(*expression, &ty);

        if !is_const_eligible(&new_expression) {
            self.sink
                .push(diagnostics::infer::const_requires_simple_expression(
                    new_expression.get_span(),
                ));
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
            self.convert_to_type(annotation, &span)
        } else {
            self.new_type_var()
        };

        let new_value = self.with_value_context(|s| s.infer_expression(*value, &ty));

        let new_else_block = if let Some(else_expression) = else_block {
            let else_ty = self.new_type_var();
            let new_else = self.infer_expression(*else_expression, &else_ty);

            let resolved_else_ty = else_ty.resolve();
            if new_else.diverges().is_none() && !resolved_else_ty.is_never() {
                let error_span = else_span.expect("let-else must have else_span");
                self.sink
                    .push(diagnostics::infer::let_else_must_diverge(error_span));
            }
            self.unify(&else_ty, &self.type_never(), &span);

            Some(Box::new(new_else))
        } else {
            None
        };

        reject_as_binding_in_irrefutable_context(self.sink, &binding.pattern);

        let (inferred_pattern, typed_pattern) =
            self.infer_pattern(binding.pattern, ty.clone(), BindingKind::Let { mutable });

        if new_else_block.is_none() {
            check_binding_pattern(self.sink, &inferred_pattern);
        } else {
            let mut innermost = &inferred_pattern;
            while let Pattern::AsBinding { pattern, .. } = innermost {
                innermost = pattern;
            }
            if matches!(innermost, Pattern::Literal { .. }) {
                self.sink
                    .push(diagnostics::infer::literal_pattern_in_binding(
                        inferred_pattern.get_span(),
                    ));
            }
        }

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
            self.post_inference_checks
                .push(PostInferenceCheck::EmptyCollection {
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

        self.unify(expected_ty, &self.type_unit(), &span);

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
