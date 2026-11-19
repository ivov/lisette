use std::mem;

use crate::checker::EnvResolve;
use crate::checker::infer::context::{BindingInference, LoopContext, LoopElementInference};
use syntax::ast::BindingKind;
use syntax::ast::{Binding, BindingId, Expression, Pattern, Span};
use syntax::types::{self, CompoundKind, IterSeqKind, RangeKind, SimpleKind, Type};

use crate::checker::infer::InferCtx;
use crate::checker::infer::validation::LoopExit;

fn collect_nested_loop_and_lambda_spans(
    expression: &Expression,
    nested_loops: &mut Vec<Span>,
    lambdas: &mut Vec<Span>,
) {
    match expression {
        Expression::Loop { span, .. }
        | Expression::While { span, .. }
        | Expression::WhileLet { span, .. }
        | Expression::For { span, .. } => nested_loops.push(*span),
        Expression::Lambda { span, .. } => lambdas.push(*span),
        _ => {}
    }
    for child in expression.children() {
        collect_nested_loop_and_lambda_spans(child, nested_loops, lambdas);
    }
}

fn span_contains(outer: &Span, inner: &Span) -> bool {
    outer.file_id == inner.file_id
        && outer.byte_offset <= inner.byte_offset
        && inner.end() <= outer.end()
}

#[derive(Clone, Copy)]
enum IterableKind {
    String,
    Slice,
    EnumeratedSlice,
    Channel,
    Array,
    Map,
    Seq,
    Seq2,
    Range(RangeKind),
    NotIterable,
}

impl IterableKind {
    fn of(ty: &Type) -> Option<Self> {
        if let Some(range) = RangeKind::of(ty) {
            return Some(Self::Range(range));
        }
        Some(match ty {
            Type::Simple(SimpleKind::String) => Self::String,
            Type::Array { .. } => Self::Array,
            Type::Compound { kind, .. } => match kind {
                CompoundKind::Slice => Self::Slice,
                CompoundKind::EnumeratedSlice => Self::EnumeratedSlice,
                CompoundKind::Channel | CompoundKind::Receiver => Self::Channel,
                CompoundKind::Map => Self::Map,
                _ => Self::NotIterable,
            },
            Type::Simple(_) | Type::Nominal { .. } | Type::ImportNamespace(_) => Self::NotIterable,
            _ => return None,
        })
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
            Type::Never
        };

        if !expected_ty.is_ignored() {
            self.unify(expected_ty, &loop_type, &span);
        }

        Expression::Loop {
            id: self.fresh_expression_id(),
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

        let new_condition = self.infer_condition(*condition);
        if let Some(span) = Self::find_propagate(&new_condition) {
            self.sink
                .push(diagnostics::infer::propagate_in_condition(span));
        }

        let new_body = self.with_loop(LoopContext::Statement, |s| {
            s.infer_expression(*body, &Type::ignored())
        });

        Expression::While {
            id: self.fresh_expression_id(),
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

        let resolved_scrutinee_ty = scrutinee_ty.resolve_in(&self.env);
        self.ensure_subject_matchable(&resolved_scrutinee_ty, &new_scrutinee.get_span());
        self.mark_scrutinee_grant(&new_scrutinee, &resolved_scrutinee_ty);

        let (new_pattern, new_body) = self.with_scope(|this| {
            let new_pattern =
                this.infer_pattern(pattern, resolved_scrutinee_ty, BindingKind::WhileLet);
            let new_body = this.with_loop(LoopContext::Statement, |s| {
                s.infer_expression(*body, &Type::ignored())
            });
            (new_pattern, new_body)
        });

        Expression::WhileLet {
            id: self.fresh_expression_id(),
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

        let (element_ty, requires_tuple_destructuring) =
            self.for_element_type(&iterable_ty, &new_iterable);

        if let Some(annotation) = &binding.annotation {
            let annotated_ty = self.convert_to_type(store, annotation, &span);
            self.unify(&element_ty, &annotated_ty, &span);
        }

        let mutable = binding.mut_span.is_some();
        if mutable && !binding.pattern.is_identifier() {
            self.sink.push(diagnostics::infer::disallowed_mut_use(
                binding.mut_span.unwrap_or(span),
            ));
        }
        let mut demoted_from_writable = false;
        let binding_element_ty = if mutable {
            element_ty.clone()
        } else {
            let resolved = element_ty.resolve_in(&self.env);
            demoted_from_writable = self.store.peel_alias(&resolved).is_writable();
            self.store.demoted_at_binding(&resolved)
        };

        if mutable && binding_element_ty.resolve_in(&self.env).is_writable() {
            self.mark_place_root_mutated(&new_iterable);
        }

        let (new_binding, new_body) = self.with_scope(|this| {
            let inferred_pattern = this.infer_pattern(
                binding.pattern,
                binding_element_ty.clone(),
                BindingKind::Let { mutable },
            );

            let element_binding_id = inferred_pattern
                .get_identifier()
                .and_then(|name| this.scopes.lookup_binding_id(&name));
            if let Some(binding_id) = element_binding_id {
                let collection = super::aliasing::place_root_name(new_iterable.unwrap_parens())
                    .map(|root| root.to_string());
                this.binding_inference.insert(
                    binding_id,
                    BindingInference::LoopElement(LoopElementInference {
                        collection,
                        was_demoted: demoted_from_writable,
                        reads: Vec::new(),
                        writes: Vec::new(),
                    }),
                );
            }

            let new_binding = Binding {
                pattern: inferred_pattern,
                annotation: binding.annotation,
                ty: binding_element_ty.clone(),
                mut_span: binding.mut_span,
            };

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
            if let Some(binding_id) = element_binding_id {
                this.report_dead_loop_copy_writes(binding_id, &new_body);
            }
            (new_binding, new_body)
        });

        Expression::For {
            id: self.fresh_expression_id(),
            binding: Box::new(new_binding),
            iterable: new_iterable.into(),
            body: new_body.into(),
            span,
        }
    }

    pub(super) fn record_loop_copy_write(&mut self, name: &str, span: Span) {
        let Some(binding_id) = self.scopes.lookup_binding_id(name) else {
            return;
        };
        let Some(inference) = self
            .binding_inference
            .get_mut(&binding_id)
            .and_then(|binding| binding.as_loop_element_mut())
        else {
            return;
        };
        inference.writes.push(span);
    }

    fn report_dead_loop_copy_writes(&mut self, binding_id: BindingId, body: &Expression) {
        let Some(inference) = self
            .binding_inference
            .get_mut(&binding_id)
            .and_then(|binding| binding.as_loop_element_mut())
        else {
            return;
        };
        let writes = mem::take(&mut inference.writes);
        let reads = mem::take(&mut inference.reads);
        if writes.is_empty() {
            return;
        }
        let mut nested_loops = Vec::new();
        let mut lambdas = Vec::new();
        collect_nested_loop_and_lambda_spans(body, &mut nested_loops, &mut lambdas);
        let observes = |read: &Span, write: &Span| {
            read.byte_offset >= write.end()
                || lambdas.iter().any(|lambda| span_contains(lambda, read))
                || nested_loops.iter().any(|nested_loop| {
                    span_contains(nested_loop, read) && span_contains(nested_loop, write)
                })
        };
        let dead_write = writes
            .iter()
            .find(|write| !reads.iter().any(|read| observes(read, write)));
        if let Some(write) = dead_write {
            let diagnostic = diagnostics::infer::loop_copy_write(
                &self.state.facts.bindings[&binding_id].name,
                inference.collection.as_deref(),
                *write,
            );
            self.sink.push(diagnostic);
        }
    }

    /// Derive the element type and whether the loop must destructure its tuple elements.
    fn for_element_type(&mut self, iterable_ty: &Type, iterable_expr: &Expression) -> (Type, bool) {
        let store = self.store;

        let resolved_unpeeled = iterable_ty.resolve_in(&self.env).strip_refs();

        // Peel one alias at a time, because `iter.Seq` is itself an alias.
        let (iter_seq, resolved_iterable_ty) =
            match types::peel_to_iter_seq(&resolved_unpeeled, |id| store.get_definition(id)) {
                Some((kind, seq)) => (Some(kind), seq),
                None => (None, store.peel_refs_and_aliases(&resolved_unpeeled).0),
            };

        let iterable_is_error = resolved_iterable_ty.is_error();

        let iterable_kind = match iter_seq {
            Some(IterSeqKind::Seq) => IterableKind::Seq,
            Some(IterSeqKind::Seq2) => IterableKind::Seq2,
            None => match IterableKind::of(&resolved_iterable_ty) {
                Some(kind) => kind,
                None => {
                    if !iterable_is_error {
                        self.sink.push(diagnostics::infer::unknown_iterable_type(
                            iterable_expr.get_span(),
                        ));
                    }
                    IterableKind::Slice
                }
            },
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

        let element_ty = match iterable_kind {
            IterableKind::String => {
                let receiver = iterable_expr.root_identifier().unwrap_or("s");
                self.sink.push(diagnostics::infer::string_not_iterable(
                    iterable_expr.get_span(),
                    receiver,
                ));
                Type::Error
            }

            IterableKind::Slice | IterableKind::Channel if !iterable_ty_args.is_empty() => {
                iterable_ty_args[0].clone()
            }

            IterableKind::EnumeratedSlice if !iterable_ty_args.is_empty() => {
                Type::Tuple(vec![Type::int(), iterable_ty_args[0].clone()])
            }

            IterableKind::Array => match &resolved_iterable_ty {
                Type::Array { element, .. } => element.as_ref().clone(),
                _ => unreachable!("an array kind comes from an array type"),
            },

            IterableKind::Map | IterableKind::Seq2 if iterable_ty_args.len() >= 2 => {
                Type::Tuple(vec![
                    iterable_ty_args[0].clone(),
                    iterable_ty_args[1].clone(),
                ])
            }

            IterableKind::Seq if !iterable_ty_args.is_empty() => iterable_ty_args[0].clone(),

            IterableKind::Range(range) if range.has_start() && !iterable_ty_args.is_empty() => {
                let elem_ty = &iterable_ty_args[0];
                if !store.peeled(elem_ty).is_simple(SimpleKind::Int) && !elem_ty.is_variable() {
                    self.sink
                        .push(diagnostics::infer::non_int_range_not_iterable(
                            elem_ty,
                            iterable_expr.get_span(),
                        ));
                }
                elem_ty.clone()
            }

            IterableKind::Range(range) if !range.has_start() => {
                let range_name = resolved_iterable_ty
                    .display_name()
                    .map(|name| name.to_string())
                    .unwrap_or_default();
                self.sink.push(diagnostics::infer::range_not_iterable(
                    &range_name,
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

        let requires_tuple_destructuring = matches!(
            iterable_kind,
            IterableKind::Map | IterableKind::EnumeratedSlice | IterableKind::Seq2
        );
        (element_ty, requires_tuple_destructuring)
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
        self.check_loop_exit(span, LoopExit::Break);

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
                let unit = Type::unit();
                self.unify(&break_ty, &unit, &span);
            }
            None
        };

        Expression::Break {
            id: self.fresh_expression_id(),
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
        self.check_loop_exit(span, LoopExit::Continue);

        Expression::Continue {
            id: self.fresh_expression_id(),
            span,
        }
    }
}
