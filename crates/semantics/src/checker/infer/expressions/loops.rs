use crate::checker::EnvResolve;
use crate::checker::infer::context::LoopContext;
use syntax::ast::BindingKind;
use syntax::ast::{Binding, Expression, Pattern, Span};
use syntax::types::Type;

use crate::checker::infer::InferCtx;

enum IterSeqKind {
    Seq,
    Seq2,
}

fn iter_seq_kind(ty: &Type) -> Option<IterSeqKind> {
    let Type::Nominal { id, .. } = ty else {
        return None;
    };
    match id.as_str() {
        "go:iter.Seq" => Some(IterSeqKind::Seq),
        "go:iter.Seq2" => Some(IterSeqKind::Seq2),
        _ => None,
    }
}

impl InferCtx<'_> {
    pub(super) fn infer_loop(
        &mut self,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let break_ty = self.new_type_var();

        let new_body = self.with_loop(LoopContext::Value(break_ty.clone()), |this| {
            this.infer_expression(*body, &Type::ignored())
        });

        let loop_type = if new_body.contains_break() {
            break_ty.clone()
        } else {
            self.type_never()
        };

        if !expected_ty.is_ignored() {
            self.unify(expected_ty, &loop_type, &span);
        }

        Expression::Loop {
            body: new_body.into(),
            ty: loop_type,
            span,
        }
    }

    pub(super) fn infer_while(
        &mut self,
        condition: Box<Expression>,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        self.unify_statement_loop(expected_ty, &span, "while");

        let new_condition = self.infer_condition(*condition, &span);
        if let Some(span) = Self::find_propagate(&new_condition) {
            self.sink
                .push(diagnostics::infer::propagate_in_condition(span));
        }

        let new_body = self.with_loop(LoopContext::Statement, |s| {
            s.infer_expression(*body, &Type::ignored())
        });

        Expression::While {
            condition: new_condition.into(),
            body: new_body.into(),
            span,
        }
    }

    pub(super) fn infer_while_let(
        &mut self,
        pattern: Pattern,
        scrutinee: Box<Expression>,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        self.unify_statement_loop(expected_ty, &span, "while");

        let scrutinee_ty = self.new_type_var();
        let new_scrutinee = self.infer_expression(*scrutinee, &scrutinee_ty);

        self.ensure_subject_matchable(
            &scrutinee_ty.resolve_in(&self.env),
            &new_scrutinee.get_span(),
        );

        let (new_pattern, new_body) = self.with_scope(|this| {
            let new_pattern = this.infer_pattern(
                pattern,
                scrutinee_ty.resolve_in(&this.env),
                BindingKind::WhileLet,
            );
            let new_body = this.with_loop(LoopContext::Statement, |s| {
                s.infer_expression(*body, &Type::ignored())
            });
            (new_pattern, new_body)
        });

        Expression::WhileLet {
            pattern: new_pattern,
            scrutinee: new_scrutinee.into(),
            body: new_body.into(),
            span,
        }
    }

    pub(super) fn infer_for(
        &mut self,
        binding: Binding,
        iterable: Box<Expression>,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        self.unify_statement_loop(expected_ty, &span, "for");

        let iterable_ty = self.new_type_var();
        let new_iterable = self.infer_expression(*iterable, &iterable_ty);

        let (element_ty, iterable_ty_name, iter_seq) =
            self.for_element_type(&iterable_ty, &new_iterable);

        if let Some(annotation) = &binding.annotation {
            let annotated_ty = self.convert_to_type(store, annotation, &span);
            self.unify(&element_ty, &annotated_ty, &span);
        }

        let (new_binding, new_body) = self.with_scope(|this| {
            let inferred_pattern = this.infer_pattern(
                binding.pattern,
                element_ty.clone(),
                BindingKind::Let { mutable: false },
            );

            let new_binding = Binding {
                pattern: inferred_pattern,
                annotation: binding.annotation,
                ty: element_ty.clone(),
                mut_span: None,
            };

            let requires_tuple_destructuring =
                matches!(iterable_ty_name.as_str(), "Map" | "EnumeratedSlice")
                    || matches!(iter_seq, Some(IterSeqKind::Seq2));
            if requires_tuple_destructuring && element_ty.is_tuple() {
                match &new_binding.pattern {
                    Pattern::Tuple { .. } => (),
                    Pattern::WildCard { .. } => (),
                    _ => {
                        this.sink
                            .push(diagnostics::infer::tuple_literal_required_in_loop(span));
                    }
                }
            }

            let new_body = this.with_loop(LoopContext::Statement, |s| {
                s.infer_expression(*body, &Type::ignored())
            });
            (new_binding, new_body)
        });

        Expression::For {
            binding: Box::new(new_binding),
            iterable: new_iterable.into(),
            body: new_body.into(),
            span,
        }
    }

    /// Derives a for-loop's element type, name, and `iter.Seq` kind from its iterable type.
    fn for_element_type(
        &mut self,
        iterable_ty: &Type,
        iterable_expr: &Expression,
    ) -> (Type, String, Option<IterSeqKind>) {
        let store = self.store;

        let resolved_unpeeled = iterable_ty.resolve_in(&self.env);
        let iter_seq = iter_seq_kind(&resolved_unpeeled);

        let resolved_iterable_ty = if iter_seq.is_some() {
            resolved_unpeeled
        } else {
            store.peel_alias(&resolved_unpeeled)
        };

        let iterable_is_error = resolved_iterable_ty.is_error();

        let iterable_ty_name = match resolved_iterable_ty.get_name() {
            Some(name) => name,
            None => {
                if !iterable_is_error {
                    self.sink.push(diagnostics::infer::unknown_iterable_type(
                        iterable_expr.get_span(),
                    ));
                }
                "Slice"
            }
        };

        let fallback_args;
        let iterable_ty_args = match resolved_iterable_ty.get_type_params() {
            Some(args) => args,
            None => {
                let element = if iterable_is_error {
                    Type::Error
                } else {
                    self.new_type_var()
                };
                fallback_args = [element.clone(), element];
                &fallback_args
            }
        };

        let element_ty = match iterable_ty_name {
            "string" => {
                let receiver = iterable_expr.root_identifier().unwrap_or("s");
                self.sink.push(diagnostics::infer::string_not_iterable(
                    iterable_expr.get_span(),
                    receiver,
                ));
                Type::Error
            }

            "Slice" | "EnumeratedSlice" | "Receiver" | "Channel"
                if !iterable_ty_args.is_empty() =>
            {
                if iterable_ty_name == "EnumeratedSlice" {
                    Type::Tuple(vec![self.type_int(), iterable_ty_args[0].clone()])
                } else {
                    iterable_ty_args[0].clone()
                }
            }

            "Array" => match &resolved_iterable_ty {
                Type::Array { element, .. } => element.as_ref().clone(),
                _ => {
                    self.sink.push(diagnostics::infer::not_iterable(
                        &resolved_iterable_ty,
                        iterable_expr.get_span(),
                    ));
                    Type::Error
                }
            },

            "Map" if iterable_ty_args.len() >= 2 => Type::Tuple(vec![
                iterable_ty_args[0].clone(),
                iterable_ty_args[1].clone(),
            ]),

            "Seq" if iter_seq.is_some() && !iterable_ty_args.is_empty() => {
                iterable_ty_args[0].clone()
            }

            "Seq2" if iter_seq.is_some() && iterable_ty_args.len() >= 2 => Type::Tuple(vec![
                iterable_ty_args[0].clone(),
                iterable_ty_args[1].clone(),
            ]),

            "Range" | "RangeInclusive" | "RangeFrom" if !iterable_ty_args.is_empty() => {
                let elem_ty = &iterable_ty_args[0];
                if elem_ty.get_name() != Some("int") && !elem_ty.is_variable() {
                    self.sink
                        .push(diagnostics::infer::non_int_range_not_iterable(
                            elem_ty,
                            iterable_expr.get_span(),
                        ));
                }
                elem_ty.clone()
            }

            "RangeTo" | "RangeToInclusive" => {
                self.sink.push(diagnostics::infer::range_not_iterable(
                    iterable_ty_name,
                    iterable_expr.get_span(),
                ));
                Type::Error
            }

            _ => {
                self.sink.push(diagnostics::infer::not_iterable(
                    &resolved_iterable_ty,
                    iterable_expr.get_span(),
                ));
                Type::Error
            }
        };

        (element_ty, iterable_ty_name.to_string(), iter_seq)
    }

    pub(super) fn infer_break(
        &mut self,
        value: Option<Box<Expression>>,
        span: Span,
        is_subexpression: bool,
    ) -> Expression {
        if is_subexpression {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(
                    "break", span,
                ));
        }
        self.check_break_outside_loop(span);
        self.check_break_in_try_block(span);
        self.check_break_in_recover_block(span);
        self.check_break_in_defer_block(span);

        let new_value = if let Some(val) = value {
            if self.loop_break_type().is_none() && self.is_inside_loop() {
                self.sink
                    .push(diagnostics::infer::break_value_in_non_loop(span));
            }
            let break_ty = self.loop_break_type().cloned().unwrap_or(Type::Error);
            let inferred = self.with_value_context(|s| s.infer_expression(*val, &break_ty));
            Some(Box::new(inferred))
        } else {
            if let Some(break_ty) = self.loop_break_type().cloned() {
                let unit = self.type_unit();
                self.unify(&break_ty, &unit, &span);
            }
            None
        };

        Expression::Break {
            value: new_value,
            span,
        }
    }

    pub(super) fn infer_continue(&mut self, span: Span, is_subexpression: bool) -> Expression {
        if is_subexpression {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(
                    "continue", span,
                ));
        }
        self.check_continue_outside_loop(span);
        self.check_continue_in_try_block(span);
        self.check_continue_in_recover_block(span);
        self.check_continue_in_defer_block(span);

        Expression::Continue { span }
    }
}
