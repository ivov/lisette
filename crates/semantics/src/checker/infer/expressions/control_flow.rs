use crate::checker::EnvResolve;
use syntax::ast::BindingKind;
use syntax::ast::{Binding, BindingId, Expression, MatchArm, Pattern, Span, TypedPattern};
use syntax::types::{SimpleKind, Type};

use crate::checker::infer::InferCtx;

/// Outcome of unifying branch types: kept first, widened to a supertype, or failed.
enum BranchReconciliation {
    FirstBranch,
    Widened(Type),
    Failed,
}

#[derive(Clone, Copy)]
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

impl InferCtx<'_, '_> {
    pub(crate) fn reconcile_and_unify(
        &mut self,
        result_ty: &Type,
        branch_types: &[Type],
        span: &Span,
    ) {
        if branch_types.is_empty() {
            return;
        }
        match self.reconcile_branch_types(branch_types, span) {
            BranchReconciliation::FirstBranch => {
                self.unify(result_ty, &branch_types[0], span);
            }
            BranchReconciliation::Widened(ty) => {
                self.unify(result_ty, &ty, span);
            }
            BranchReconciliation::Failed => {
                debug_assert!(branch_types.len() >= 2);
                let _ = self.try_unify(&branch_types[0], &branch_types[1], span);
                self.unify(result_ty, &branch_types[0], span);
            }
        }
    }

    fn reconcile_branch_types(
        &mut self,
        branch_types: &[Type],
        span: &Span,
    ) -> BranchReconciliation {
        let store = self.store;
        if branch_types.len() < 2 {
            return BranchReconciliation::FirstBranch;
        }

        let mut common = branch_types[0].clone();
        let mut widened_to: Option<Type> = None;

        for next in &branch_types[1..] {
            let diag_count = self.sink.len();
            if self
                .speculatively(|this| InferCtx::new(this, store).try_unify(&common, next, span))
                .is_ok()
            {
                continue;
            }
            self.sink.truncate(diag_count);

            if self
                .speculatively(|this| InferCtx::new(this, store).try_unify(next, &common, span))
                .is_ok()
            {
                common = next.clone();
                widened_to = Some(common.clone());
                continue;
            }
            self.sink.truncate(diag_count);

            return BranchReconciliation::Failed;
        }

        match widened_to {
            Some(ty) => BranchReconciliation::Widened(ty),
            None => BranchReconciliation::FirstBranch,
        }
    }

    fn ensure_subject_matchable(&mut self, ty: &Type, span: &Span) {
        match ty {
            _ if ty.is_unknown() => {
                self.sink
                    .push(diagnostics::infer::cannot_match_on_unknown(*span));
            }
            Type::Nominal { .. } => {}
            Type::Function(_) => {
                self.sink
                    .push(diagnostics::infer::cannot_match_on_functions(*span));
            }
            Type::Var { .. } => {
                self.sink
                    .push(diagnostics::infer::cannot_match_on_unconstrained_type(
                        *span,
                    ));
            }
            Type::Forall { body, .. } => {
                self.ensure_subject_matchable(body, span);
            }
            Type::Parameter(_) => {}
            Type::Tuple(_) => {}
            Type::Never | Type::Error => {}
            Type::ImportNamespace(_) => {}
            Type::ReceiverPlaceholder => {}
            Type::Simple(_) | Type::Compound { .. } => {}
        }
    }

    fn infer_in_loop_context<F>(&mut self, f: F) -> Expression
    where
        F: FnOnce(&mut Self) -> Expression,
    {
        self.increment_try_block_loop_depth();
        self.increment_recover_block_loop_depth();
        self.scopes.increment_loop_depth();
        let result = f(self);
        self.scopes.decrement_loop_depth();
        self.decrement_recover_block_loop_depth();
        self.decrement_try_block_loop_depth();
        result
    }

    /// Like `infer_in_loop_context`, but clears `loop_break_type` so that
    /// `break value` is rejected. Used for `while`, `while let`, and `for`
    /// (only `loop` supports `break value`).
    fn infer_in_non_value_loop_context<F>(&mut self, f: F) -> Expression
    where
        F: FnOnce(&mut Self) -> Expression,
    {
        let prev_break_type = self.scopes.loop_break_type().cloned();
        self.scopes.clear_loop_break_type();
        let result = self.infer_in_loop_context(f);
        if let Some(prev) = prev_break_type {
            self.scopes.set_loop_break_type(prev);
        }
        result
    }

    fn infer_condition(&mut self, condition: Expression, span: &Span) -> Expression {
        let cond_ty = self.new_type_var();
        let inferred = self.infer_expression(condition, &cond_ty);
        if cond_ty.resolve_in(&self.env).underlying_simple_kind() != Some(SimpleKind::Bool) {
            let bool_ty = self.type_bool();
            self.unify(&bool_ty, &cond_ty, span);
        }
        inferred
    }

    pub(super) fn infer_if(
        &mut self,
        condition: Box<Expression>,
        consequence: Box<Expression>,
        alternative: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let consequence_ty = self.new_type_var();
        let alternative_ty = self.new_type_var();

        let is_expression = !expected_ty.is_ignored();
        let has_no_else = !alternative.has_else();

        // When expected_ty is already resolved to a concrete type (e.g. an
        // interface from a return type annotation), use a shared type variable
        // (like match does) so both branches can satisfy interface constraints.
        let expected_is_concrete =
            is_expression && !has_no_else && !expected_ty.resolve_in(&self.env).is_variable();

        if expected_is_concrete {
            self.unify(&consequence_ty, expected_ty, &span);
            self.unify(&alternative_ty, expected_ty, &span);
        }

        // Branch bodies are tail-like contexts where Never calls are valid.
        let saved_subexpression = self.scopes.set_in_subexpression(false);
        let new_consequence = self.infer_expression(*consequence, &consequence_ty);
        self.scopes.set_in_subexpression(false);
        let new_alternative = self.infer_expression(*alternative, &alternative_ty);
        self.scopes.set_in_subexpression(saved_subexpression);

        if has_no_else {
            // An `if` without `else` always has type () (unit), like Rust.
            // The consequence body can produce any type — it's discarded.
            if is_expression {
                let unit_ty = self.type_unit();
                self.unify(expected_ty, &unit_ty, &span);
            }
        } else if is_expression && !expected_is_concrete {
            let consequence_span = new_consequence.get_span();
            let alternative_span = new_alternative.get_span();

            let resolved_consequence = consequence_ty.resolve_in(&self.env);
            let resolved_alternative = alternative_ty.resolve_in(&self.env);

            match self
                .reconcile_branch_types(&[consequence_ty.clone(), alternative_ty.clone()], &span)
            {
                BranchReconciliation::FirstBranch => {
                    self.unify(expected_ty, &consequence_ty, &consequence_span);
                }
                BranchReconciliation::Widened(ref ty) => {
                    self.unify(expected_ty, ty, &alternative_span);
                }
                BranchReconciliation::Failed => {
                    let _ = self.try_unify(&consequence_ty, &alternative_ty, &span);
                    self.sink.push(diagnostics::infer::branch_type_mismatch(
                        &resolved_consequence,
                        consequence_span,
                        &resolved_alternative,
                        alternative_span,
                    ));
                    self.unify(expected_ty, &consequence_ty, &consequence_span);
                }
            }
        }

        let result_ty = if has_no_else {
            self.type_unit()
        } else if is_expression && !expected_is_concrete {
            expected_ty.resolve_in(&self.env)
        } else {
            consequence_ty
        };

        let new_condition = self.infer_condition(*condition, &span);
        if let Some(span) = Self::find_propagate(&new_condition) {
            self.sink
                .push(diagnostics::infer::propagate_in_condition(span));
        }
        Expression::If {
            condition: new_condition.into(),
            consequence: new_consequence.into(),
            alternative: new_alternative.into(),
            ty: result_ty,
            span,
        }
    }

    pub(super) fn infer_match(
        &mut self,
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let (new_subject, new_arms, result_ty) = self.infer_match_arms(
            subject,
            arms,
            BindingKind::MatchArm,
            false,
            span,
            expected_ty,
        );

        Expression::Match {
            subject: new_subject.into(),
            arms: new_arms,
            ty: result_ty,
            span,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_if_let(
        &mut self,
        pattern: Pattern,
        scrutinee: Box<Expression>,
        consequence: Box<Expression>,
        alternative: Box<Expression>,
        typed_pattern: Option<TypedPattern>,
        else_span: Option<Span>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let is_if_let_without_else = else_span.is_none();
        let arms = vec![
            MatchArm {
                pattern,
                guard: None,
                typed_pattern,
                expression: consequence,
            },
            MatchArm {
                pattern: Pattern::WildCard {
                    span: alternative.get_span(),
                },
                guard: None,
                typed_pattern: None,
                expression: alternative,
            },
        ];

        let (new_scrutinee, mut new_arms, result_ty) = self.infer_match_arms(
            scrutinee,
            arms,
            BindingKind::IfLet,
            is_if_let_without_else,
            span,
            expected_ty,
        );

        let wildcard_arm = new_arms.pop().expect("if-let has an else arm");
        let pattern_arm = new_arms.pop().expect("if-let has a pattern arm");

        Expression::IfLet {
            pattern: pattern_arm.pattern,
            scrutinee: new_scrutinee.into(),
            consequence: pattern_arm.expression,
            alternative: wildcard_arm.expression,
            typed_pattern: pattern_arm.typed_pattern,
            else_span,
            ty: result_ty,
            span,
        }
    }

    fn infer_match_arms(
        &mut self,
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
        arm_kind: BindingKind,
        is_if_let_without_else: bool,
        span: Span,
        expected_ty: &Type,
    ) -> (Expression, Vec<MatchArm>, Type) {
        let result_ty = self.new_type_var();
        let subject_ty = self.new_type_var();
        let new_subject = self.infer_expression(*subject, &subject_ty);

        let resolved_subject_ty = new_subject.get_type().resolve_in(&self.env);
        self.ensure_subject_matchable(&resolved_subject_ty, &new_subject.get_span());

        let is_statement = expected_ty.is_ignored();

        // if-let without else always has type (), like if without else.
        // Arms don't need to agree since the result is always ().
        let arms_independent = is_statement || is_if_let_without_else;

        if !is_statement {
            if is_if_let_without_else {
                let unit = self.type_unit();
                self.unify(expected_ty, &unit, &span);
                let _ = self.try_unify(&result_ty, &unit, &span);
            } else {
                self.unify(expected_ty, &result_ty, &span);
            }
        }

        let needs_reconciliation =
            !arms_independent && result_ty.resolve_in(&self.env).is_variable();

        let match_has_guard = arms.iter().any(|a| a.guard.is_some());
        let new_arms = arms
            .into_iter()
            .map(|a| {
                self.scopes.push();

                let pattern_ty = subject_ty.resolve_in(&self.env);
                let (new_pattern, typed_pattern) =
                    self.infer_pattern(a.pattern, pattern_ty, arm_kind);

                let new_guard = a
                    .guard
                    .map(|guard| Box::new(self.infer_condition(*guard, &span)));

                let independent_ty;
                let arm_expected = if arms_independent || needs_reconciliation {
                    independent_ty = self.new_type_var();
                    &independent_ty
                } else {
                    &result_ty
                };
                let saved_in_match_arm = self.scopes.set_in_match_arm(true);
                let saved_in_guarded = self.scopes.set_in_guarded_match_arm(
                    self.scopes.is_in_guarded_match_arm() || match_has_guard,
                );
                // Arm body is a tail-like context where Never calls are valid.
                self.scopes.set_in_subexpression(false);
                let new_expression = self.infer_expression(*a.expression, arm_expected);
                self.scopes.set_in_guarded_match_arm(saved_in_guarded);
                self.scopes.set_in_match_arm(saved_in_match_arm);

                self.scopes.pop();

                MatchArm {
                    pattern: new_pattern,
                    guard: new_guard,
                    typed_pattern: Some(typed_pattern),
                    expression: Box::new(new_expression),
                }
            })
            .collect::<Vec<_>>();

        if needs_reconciliation {
            let arm_types: Vec<Type> = new_arms.iter().map(|a| a.expression.get_type()).collect();
            self.reconcile_and_unify(&result_ty, &arm_types, &span);
        } else if is_statement && let Some(first_arm) = new_arms.first() {
            // In statement position, set the match's type from the first arm so the
            // expression still has a well-defined type for inspection, even though
            // arms are not required to agree.
            let first_ty = first_arm.expression.get_type();
            let _ = self.try_unify(&result_ty, &first_ty, &span);
        }

        (new_subject, new_arms, result_ty)
    }

    pub(super) fn infer_loop(
        &mut self,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let break_ty = self.new_type_var();

        let prev_break_type = self.scopes.loop_break_type().cloned();
        self.scopes.set_loop_break_type(break_ty.clone());

        let saved_in_match_arm = self.scopes.set_in_match_arm(false);
        self.scopes.push_loop_needs_label();

        let new_body = self.infer_in_loop_context(|s| s.infer_expression(*body, &Type::ignored()));

        let needs_label = self.scopes.pop_loop_needs_label();
        self.scopes.set_in_match_arm(saved_in_match_arm);

        if let Some(prev) = prev_break_type {
            self.scopes.set_loop_break_type(prev);
        } else {
            self.scopes.clear_loop_break_type();
        }

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
            needs_label,
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

        let saved_in_match_arm = self.scopes.set_in_match_arm(false);
        self.scopes.push_loop_needs_label();

        let new_body =
            self.infer_in_non_value_loop_context(|s| s.infer_expression(*body, &Type::ignored()));

        let needs_label = self.scopes.pop_loop_needs_label();
        self.scopes.set_in_match_arm(saved_in_match_arm);

        Expression::While {
            condition: new_condition.into(),
            body: new_body.into(),
            span,
            needs_label,
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

        self.scopes.push();
        let (new_pattern, typed_pattern) = self.infer_pattern(
            pattern,
            scrutinee_ty.resolve_in(&self.env),
            BindingKind::WhileLet,
        );

        let saved_in_match_arm = self.scopes.set_in_match_arm(false);
        self.scopes.push_loop_needs_label();

        let new_body =
            self.infer_in_non_value_loop_context(|s| s.infer_expression(*body, &Type::ignored()));

        let needs_label = self.scopes.pop_loop_needs_label();
        self.scopes.set_in_match_arm(saved_in_match_arm);

        self.scopes.pop();

        Expression::WhileLet {
            pattern: new_pattern,
            scrutinee: new_scrutinee.into(),
            body: new_body.into(),
            typed_pattern: Some(typed_pattern),
            span,
            needs_label,
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

        let resolved_unpeeled = iterable_ty.resolve_in(&self.env);
        let iter_seq = iter_seq_kind(&resolved_unpeeled);

        // `iter.Seq<V>` / `iter.Seq2<K, V>` are Go range-over-func iterators
        // stored as function aliases. Keep the nominal so its name and type args
        // stay available; peeling expands it to an unnamed `fn(...)` shape.
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
                        new_iterable.get_span(),
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
                let receiver = new_iterable.root_identifier().unwrap_or("s");
                self.sink.push(diagnostics::infer::string_not_iterable(
                    new_iterable.get_span(),
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
                            new_iterable.get_span(),
                        ));
                }
                elem_ty.clone()
            }

            "RangeTo" | "RangeToInclusive" => {
                self.sink.push(diagnostics::infer::range_not_iterable(
                    iterable_ty_name,
                    new_iterable.get_span(),
                ));
                Type::Error
            }

            _ => {
                self.sink.push(diagnostics::infer::not_iterable(
                    &resolved_iterable_ty,
                    new_iterable.get_span(),
                ));
                Type::Error
            }
        };

        if let Some(annotation) = &binding.annotation {
            let annotated_ty = self.convert_to_type(store, annotation, &span);
            self.unify(&element_ty, &annotated_ty, &span);
        }

        // Push a new scope so the loop variable doesn't shadow outer bindings
        self.scopes.push();

        let (inferred_pattern, typed_pattern) = self.infer_pattern(
            binding.pattern,
            element_ty.clone(),
            BindingKind::Let { mutable: false },
        );

        let new_binding = Binding {
            pattern: inferred_pattern,
            annotation: binding.annotation,
            typed_pattern: Some(typed_pattern),
            ty: element_ty.clone(),
            mutable: false,
        };

        let binding_id: Option<BindingId> = new_binding
            .pattern
            .get_identifier()
            .and_then(|name| self.scopes.lookup_binding_id(&name));

        // When iterating over types that yield multiple values (`Map`, `EnumeratedSlice`),
        // Go's `range` returns multiple values, so the binding must be a tuple literal.
        // This does NOT apply to `Slice<(A, B)>` where the element is already a tuple value.
        let requires_tuple_destructuring = matches!(iterable_ty_name, "Map" | "EnumeratedSlice")
            || matches!(iter_seq, Some(IterSeqKind::Seq2));
        if requires_tuple_destructuring && element_ty.is_tuple() {
            match &new_binding.pattern {
                Pattern::Tuple { .. } => (),
                Pattern::WildCard { .. } => (),
                _ => {
                    self.sink
                        .push(diagnostics::infer::tuple_literal_required_in_loop(span));
                }
            }
        }

        let saved_in_match_arm = self.scopes.set_in_match_arm(false);
        self.scopes.push_loop_needs_label();

        let new_body =
            self.infer_in_non_value_loop_context(|s| s.infer_expression(*body, &Type::ignored()));

        let needs_label = self.scopes.pop_loop_needs_label();
        self.scopes.set_in_match_arm(saved_in_match_arm);

        self.scopes.pop();

        Expression::For {
            binding: Box::new(new_binding),
            iterable: new_iterable.into(),
            body: new_body.into(),
            span,
            needs_label,
            binding_id,
        }
    }

    pub(super) fn infer_return_statement(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        parent_is_subexpression: bool,
    ) -> Expression {
        if parent_is_subexpression {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(
                    "return", span,
                ));
        }
        self.check_return_in_try_block(span);
        self.check_return_in_recover_block(span);
        self.check_return_in_defer_block(span);
        match &*expression {
            Expression::Break { span: s, .. } => {
                self.sink
                    .push(diagnostics::infer::control_flow_in_expression("break", *s));
            }
            Expression::Continue { span: s } => {
                self.sink
                    .push(diagnostics::infer::control_flow_in_expression(
                        "continue", *s,
                    ));
            }
            Expression::Return { span: s, .. } => {
                self.sink
                    .push(diagnostics::infer::control_flow_in_expression("return", *s));
            }
            _ => {}
        }
        self.scopes.set_in_subexpression(false);
        self.infer_return(expression, span)
    }

    fn infer_return(&mut self, expression: Box<Expression>, span: Span) -> Expression {
        let return_ty = self
            .scopes
            .lookup_fn_return_type()
            .cloned()
            .unwrap_or_else(|| {
                self.sink
                    .push(diagnostics::infer::return_outside_function(span));
                Type::Error
            });

        let new_expression =
            self.with_value_context(|s| s.infer_expression(*expression, &return_ty));

        Expression::Return {
            expression: new_expression.into(),
            ty: self.type_never(),
            span,
        }
    }

    pub(super) fn infer_defer(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if self.scopes.is_value_context() {
            self.sink
                .push(diagnostics::infer::defer_in_expression_position(span));
        }

        if self.scopes.is_inside_loop() {
            self.sink.push(diagnostics::infer::defer_in_loop(span));
        }

        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        let is_block = matches!(*expression, Expression::Block { .. });
        let saved_loop_depth = if is_block {
            self.scopes.increment_defer_block_depth();
            self.scopes.reset_loop_depth()
        } else {
            0
        };

        let defer_ty = self.new_type_var();
        let new_expression = self.infer_expression(*expression, &defer_ty);

        if is_block {
            self.scopes.restore_loop_depth(saved_loop_depth);
            self.scopes.decrement_defer_block_depth();
        }

        if let Some(propagate_span) = Self::find_propagate(&new_expression) {
            self.sink
                .push(diagnostics::infer::propagate_in_defer(propagate_span));
        }

        self.check_deferred_lock(&new_expression);

        Expression::Defer {
            expression: new_expression.into(),
            ty: self.type_unit(),
            span,
        }
    }

    pub(super) fn infer_assert(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let new_expression = self.infer_condition(*expression, &span);
        if let Some(propagate_span) = Self::find_propagate(&new_expression) {
            self.sink
                .push(diagnostics::infer::propagate_in_assert(propagate_span));
        }
        if !self.scopes.has_test_handle() {
            self.sink
                .push(diagnostics::infer::assert_without_test_context(span));
        }
        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        Expression::Assert {
            expression: new_expression.into(),
            ty: unit_ty,
            span,
        }
    }

    pub(super) fn infer_break(
        &mut self,
        value: Option<Box<Expression>>,
        span: Span,
        parent_is_subexpression: bool,
    ) -> Expression {
        if parent_is_subexpression {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(
                    "break", span,
                ));
        }
        self.check_break_outside_loop(span);
        self.check_break_in_try_block(span);
        self.check_break_in_recover_block(span);
        self.check_break_in_defer_block(span);

        self.mark_loop_needs_label_in_match_arm();

        let new_value = if let Some(val) = value {
            if self.scopes.loop_break_type().is_none() && self.scopes.is_inside_loop() {
                self.sink
                    .push(diagnostics::infer::break_value_in_non_loop(span));
            }
            let break_ty = self
                .scopes
                .loop_break_type()
                .cloned()
                .unwrap_or_else(|| Type::Error);
            let inferred = self.with_value_context(|s| s.infer_expression(*val, &break_ty));
            Some(Box::new(inferred))
        } else {
            if let Some(break_ty) = self.scopes.loop_break_type().cloned() {
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

    pub(super) fn infer_continue(
        &mut self,
        span: Span,
        parent_is_subexpression: bool,
    ) -> Expression {
        if parent_is_subexpression {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(
                    "continue", span,
                ));
        }
        self.check_continue_outside_loop(span);
        self.check_continue_in_try_block(span);
        self.check_continue_in_recover_block(span);
        self.check_continue_in_defer_block(span);

        if self.scopes.is_in_guarded_match_arm() {
            self.scopes.mark_current_loop_needs_label();
        }

        Expression::Continue { span }
    }

    fn mark_loop_needs_label_in_match_arm(&mut self) {
        if self.scopes.is_in_match_arm() {
            self.scopes.mark_current_loop_needs_label();
        }
    }

    pub(crate) fn find_propagate(expression: &Expression) -> Option<Span> {
        if let Expression::Propagate { span, .. } = expression {
            return Some(*span);
        }
        expression
            .children()
            .into_iter()
            .find_map(Self::find_propagate)
    }

    pub(super) fn infer_task(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if self.scopes.is_value_context() {
            self.sink
                .push(diagnostics::infer::task_in_expression_position(span));
        }

        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        // task spawns a new goroutine — enclosing loop context doesn't apply
        let saved_loop_depth = self.scopes.reset_loop_depth();

        let task_ty = self.new_type_var();
        let new_expression = self.infer_expression(*expression, &task_ty);

        self.scopes.restore_loop_depth(saved_loop_depth);

        Expression::Task {
            expression: new_expression.into(),
            ty: self.type_unit(),
            span,
        }
    }
}
