use syntax::EcoString;
use syntax::ast::DeadCodeCause;
use syntax::ast::{BinaryOperator, Expression, Span, UnaryOperator};
use syntax::program::Visibility;
use syntax::types::Type;

use super::super::Checker;
use super::super::checks::{check_is_non_addressable, check_non_addressable_assignment_target};
use crate::checker::PostInferenceCheck;
use crate::facts::DiscardedTailKind;

/// Checks whether an assignment target expression contains a deref (`.* `)
/// anywhere in its chain. For example, `p.*.x` is a `DotAccess` wrapping a
/// `Unary::Deref`, and mutations through a deref don't require the variable
/// to be declared `mut` since they mutate the pointed-to value.
pub(crate) fn contains_deref(expression: &Expression) -> bool {
    match expression {
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => true,
        Expression::DotAccess { expression, .. } => contains_deref(expression),
        Expression::IndexedAccess { expression, .. } => contains_deref(expression),
        _ => false,
    }
}

/// Checks whether an expression contains a stored Reference (`&var_name`) to a specific variable.
/// Used to detect self-referential assignment patterns like `x = Foo { field: &x }`.
///
/// Note: This does NOT reject immediately-dereferenced references like `(&x).*` since those
/// don't create circular references - the reference is created and consumed in the same expression.
fn contains_stored_reference_to(expression: &Expression, var_name: &str) -> bool {
    match expression {
        // A reference inside a deref is immediately consumed, so it's safe
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => {
            // Don't check inside a deref - references here are immediately consumed
            false
        }
        // References in struct fields are stored
        Expression::StructCall {
            field_assignments, ..
        } => {
            field_assignments
                .iter()
                .any(|f| contains_stored_reference_to(&f.value, var_name))
                || field_assignments.iter().any(|f| {
                    // Direct reference in a field value
                    if let Expression::Reference { expression, .. } = &*f.value {
                        expression.get_var_name().as_deref() == Some(var_name)
                    } else {
                        false
                    }
                })
        }
        // References in function arguments might be stored (e.g., Some(&x))
        Expression::Call { args, spread, .. } => {
            let check = |expr: &Expression| {
                if let Expression::Reference { expression, .. } = expr {
                    expression.get_var_name().as_deref() == Some(var_name)
                } else {
                    contains_stored_reference_to(expr, var_name)
                }
            };
            args.iter().any(check) || spread.as_ref().as_ref().is_some_and(check)
        }
        // Recurse but skip immediately-dereferenced contexts
        Expression::Binary { left, right, .. } => {
            contains_stored_reference_to(left, var_name)
                || contains_stored_reference_to(right, var_name)
        }
        Expression::Paren { expression, .. } | Expression::DotAccess { expression, .. } => {
            contains_stored_reference_to(expression, var_name)
        }
        Expression::IndexedAccess {
            expression, index, ..
        } => {
            contains_stored_reference_to(expression, var_name)
                || contains_stored_reference_to(index, var_name)
        }
        _ => false,
    }
}

impl Checker<'_, '_> {
    pub(super) fn infer_paren(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
        parent_is_subexpression: bool,
    ) -> Expression {
        if !parent_is_subexpression {
            match &*expression {
                Expression::Return { span: s, .. } => {
                    self.sink
                        .push(diagnostics::infer::control_flow_in_expression("return", *s));
                }
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
                _ => {}
            }
        }

        self.inference.in_subexpression = parent_is_subexpression;
        let new_expression = self.infer_expression(*expression, expected_ty);
        let new_ty = new_expression.get_type();

        Expression::Paren {
            expression: new_expression.into(),
            ty: new_ty,
            span,
        }
    }

    pub(super) fn infer_block(
        &mut self,
        items: Vec<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if items.is_empty() {
            let unit_ty = self.type_unit();
            let resolved = expected_ty.resolve();
            if let Type::Constructor { id, params, .. } = &resolved
                && id.ends_with("Map")
                && params.len() == 2
            {
                let k = params[0].resolve();
                let v = params[1].resolve();
                self.sink
                    .push(diagnostics::infer::invalid_map_initialization(&k, &v, span));
            } else {
                self.unify(expected_ty, &unit_ty, &span);
            }
            return Expression::Block {
                items,
                ty: unit_ty,
                span,
            };
        }

        self.scopes.push();
        self.register_types_and_values(&items, &Visibility::Local);

        let new_items = self.infer_block_items(items, expected_ty.clone());

        let last_item = new_items.last().expect("block must have at least one item");
        let block_ty = last_item.get_type();

        self.scopes.pop();

        Expression::Block {
            items: new_items,
            ty: block_ty,
            span,
        }
    }

    pub(super) fn infer_reference(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let inner_ty = self.new_type_var();
        let new_expression = self.infer_expression(*expression, &inner_ty);

        let resolved_inner = inner_ty.resolve();
        let is_already_ref = resolved_inner.is_ref();

        // Collapse &ref_var to ref_var — adding another reference layer is a no-op
        let ref_ty = if is_already_ref {
            self.facts
                .add_overused_reference(span, new_expression.get_var_name());
            resolved_inner
        } else {
            self.type_reference(inner_ty.clone())
        };

        self.unify(expected_ty, &ref_ty, &span);

        if !is_already_ref {
            if self.has_newtype_dot0_in_chain(&new_expression) {
                self.sink
                    .push(diagnostics::infer::reference_through_newtype(span));
            }

            if let Some(kind) = check_is_non_addressable(&new_expression) {
                self.sink
                    .push(diagnostics::infer::non_addressable_expression(kind, span));
            }

            if let Some(var_name) = new_expression.get_var_name()
                && let Some(binding_id) = self.scopes.lookup_binding_id(&var_name)
            {
                self.facts.mark_mutated(binding_id);
            }
        }

        Expression::Reference {
            expression: new_expression.into(),
            ty: ref_ty,
            span,
        }
    }

    pub(super) fn infer_identifier(
        &mut self,
        value: EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let binding_id = self.scopes.lookup_binding_id(&value);
        if let Some(id) = binding_id {
            // Don't mark assignment targets as "used" - only mark actual uses
            if !self.is_inferring_assignment_target() {
                self.facts.mark_used(id);
            }

            if let Some(binding_fact) = self.facts.bindings.get(&id) {
                let definition_span = binding_fact.span;
                self.facts.add_usage(span, definition_span);
            }
        }

        let qualified: Option<EcoString> = if binding_id.is_none() {
            self.lookup_qualified_name(&value).map(EcoString::from)
        } else {
            None
        };

        if let Some(ref qname) = qualified
            && let Some(definition_span) = self.get_definition_name_span(qname.as_str())
        {
            self.facts.add_usage(span, definition_span);
        }

        let ty = match self.lookup_type(&value) {
            Some(ty) => ty,
            None => {
                if value == "self" {
                    self.sink
                        .push(diagnostics::infer::self_in_static_method(span));
                } else {
                    self.error_name_not_found(&value, span);
                }
                Type::Error
            }
        };

        let (identifier_ty, _) = self.instantiate(&ty);

        self.unify(expected_ty, &identifier_ty, &span);

        if !self.scopes.is_callee_context() {
            self.check_native_value_usage(&value, &identifier_ty, span);
        }

        Expression::Identifier {
            value,
            ty: identifier_ty,
            span,
            binding_id,
            qualified,
        }
    }

    pub(super) fn infer_assignment(
        &mut self,
        target: Box<Expression>,
        value: Box<Expression>,
        compound_operator: Option<BinaryOperator>,
        span: Span,
    ) -> Expression {
        let target_ty = self.new_type_var();
        // Prevent simple assignment targets from being marked as "used" in the lint system.
        // Complex targets like `a[i]` or `r.*` have subexpressions that ARE being read.
        let is_simple_target = matches!(&*target, Expression::Identifier { .. });
        if is_simple_target {
            self.set_inferring_assignment_target();
        }
        let new_target = self.infer_expression(*target, &target_ty);
        if is_simple_target {
            self.clear_inferring_assignment_target();
        }

        if let Some(kind) = check_non_addressable_assignment_target(&new_target) {
            self.sink
                .push(diagnostics::infer::non_addressable_assignment(kind, span));
        }

        self.check_newtype_field_assignment(&new_target, span);
        self.check_map_field_chain_assignment(&new_target, span);

        // Propagates type information to the RHS (e.g., lambda params
        // get their types from a Map's value type).
        let value_expected = target_ty.resolve();
        let new_value = self.infer_expression(*value, &value_expected);
        let value_ty = new_value.get_type();

        // Track mutation for binding-rooted targets. Call-based lvalues
        // (e.g., `get().*.x = ...`) have no local binding to track.
        if let Some(var_name) = new_target.get_var_name() {
            if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
                // For compound assignments (+=, -=, etc.), the target is being read.
                // For simple assignments (=), the target is not read, handled via inferring_assignment_target.
                if compound_operator.is_some() {
                    self.facts.mark_used(binding_id);
                }
                self.facts.mark_mutated(binding_id);
            }

            let is_mutable = self.scopes.lookup_mutable(&var_name);

            let is_deref = contains_deref(&new_target);

            // Mutation through a Ref<T> binding doesn't require mut — the pointer
            // isn't being reassigned, the pointee is being mutated through it.
            let binding_is_ref = self
                .scopes
                .lookup_value(&var_name)
                .map(|t| t.resolve().is_ref())
                .unwrap_or(false);

            let can_mutate = is_mutable || is_deref || binding_is_ref;

            if !can_mutate {
                let self_type_name = if var_name == "self" {
                    self.lookup_type("self")
                        .and_then(|t| t.get_name().map(str::to_owned))
                } else {
                    None
                };
                let is_match_arm = self
                    .scopes
                    .lookup_binding_id(&var_name)
                    .and_then(|id| self.facts.bindings.get(&id))
                    .is_some_and(|b| b.kind.is_match_arm());
                self.sink.push(diagnostics::infer::disallowed_mutation(
                    &var_name,
                    span,
                    self_type_name.as_deref(),
                    is_match_arm,
                ));
            }

            // Check for self-referential assignment: x = Expr { field: &x }
            // This creates a circular reference in Go and is not allowed.
            if contains_stored_reference_to(&new_value, &var_name) {
                self.sink
                    .push(diagnostics::infer::self_reference_in_assignment(span));
            }
        }

        // Only unify if the RHS type is still a variable (not yet resolved).
        // If the RHS was inferred with `value_expected` from the target, the
        // type inference already emitted any mismatch diagnostic — a second
        // unify here would duplicate it.
        if value_ty.is_variable() {
            self.unify(&target_ty, &value_ty, &span);
        }

        Expression::Assignment {
            target: new_target.into(),
            value: new_value.into(),
            compound_operator,
            span,
        }
    }

    pub(super) fn infer_tuple(
        &mut self,
        elements: Vec<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let expected_elements: Vec<Type> = match expected_ty.resolve() {
            Type::Tuple(elems) if elems.len() == elements.len() => elems,
            _ => elements.iter().map(|_| self.new_type_var()).collect(),
        };

        let inferred_elements: Vec<Expression> = elements
            .into_iter()
            .zip(expected_elements.iter())
            .map(|(element, expected)| {
                self.with_value_context(|s| s.infer_expression(element, expected))
            })
            .collect();

        for elem in &inferred_elements {
            self.check_not_temp_producing(elem);
        }

        let element_types: Vec<Type> = inferred_elements.iter().map(|e| e.get_type()).collect();

        let tuple_ty = Type::Tuple(element_types);

        self.unify(&tuple_ty, expected_ty, &span);

        Expression::Tuple {
            elements: inferred_elements,
            ty: tuple_ty,
            span,
        }
    }

    pub(super) fn infer_block_items(
        &mut self,
        items: Vec<Expression>,
        last_item_expected_ty: Type,
    ) -> Vec<Expression> {
        let items_len = items.len();
        let mut new_items = Vec::with_capacity(items_len);
        let mut diverged_at: Option<(usize, DeadCodeCause)> = None;

        for (i, item) in items.into_iter().enumerate() {
            if diverged_at.is_some() {
                let dead_item_ty = self.new_type_var();
                let inferred_item = self.infer_expression(item, &dead_item_ty);
                new_items.push(inferred_item);
                continue;
            }

            let is_last = i == items_len - 1;
            let is_literal = matches!(item, Expression::Literal { .. });
            let item_span = item.get_span();

            let is_statement_only = matches!(
                item,
                Expression::Let { .. }
                    | Expression::Assignment { .. }
                    | Expression::Task { .. }
                    | Expression::Defer { .. }
            );

            let suppress_unused_check = item.is_control_flow();

            let callee_name = item.callee_name();

            // Reject statement-only items (let, =, task, defer) as block tail
            // when the block is expected to produce a non-unit value.
            if is_last && is_statement_only {
                let expected = last_item_expected_ty.resolve();
                if last_item_expected_ty.is_ignored() {
                    // ignored context — never fire
                } else if matches!(expected, Type::Variable(_)) {
                    // Type not yet resolved — defer check until after inference
                    self.post_inference_checks
                        .push(PostInferenceCheck::StatementTail {
                            expected_ty: last_item_expected_ty.clone(),
                            span: item_span,
                        });
                } else if !expected.is_unit() {
                    self.sink
                        .push(diagnostics::infer::statement_as_tail(item_span));
                }
            }

            let expression_ty = if is_statement_only {
                Type::ignored()
            } else if is_last {
                last_item_expected_ty.clone()
            } else if suppress_unused_check {
                Type::ignored()
            } else {
                self.new_type_var()
            };

            let prev_ctx = if !is_last {
                Some(self.scopes.set_statement_context())
            } else {
                None
            };

            // Mark as top-level statement so `Err(x)?`/`None?` is allowed here
            // but rejected when nested inside a compound sub-expression.
            self.inference.in_subexpression = false;
            let inferred_item = self.infer_expression(item, &expression_ty);

            if let Some(ctx) = prev_ctx {
                self.scopes.restore_use_context(ctx);
            }

            if !is_statement_only && !suppress_unused_check && !is_last {
                let mut allowed_lints = callee_name
                    .as_ref()
                    .map(|name| self.callee_allowed_lints(name, &inferred_item))
                    .unwrap_or_default();

                // Channel send returns bool but fire-and-forget is the common pattern
                if Self::is_channel_send(&inferred_item) {
                    allowed_lints.push("unused_value".to_string());
                }

                self.check_unused_expression(
                    item_span,
                    &expression_ty.resolve(),
                    is_literal,
                    &allowed_lints,
                );
            }

            if is_last
                && !is_statement_only
                && !suppress_unused_check
                && last_item_expected_ty.is_ignored()
                && let Some(callee_return_ty) = Self::get_call_return_type(&inferred_item)
            {
                let resolved = callee_return_ty.resolve();
                let classification = if resolved.is_result() {
                    Some(("unused_result", DiscardedTailKind::Result))
                } else if resolved.is_option() {
                    Some(("unused_option", DiscardedTailKind::Option))
                } else if resolved.is_partial() {
                    Some(("unused_partial", DiscardedTailKind::Partial))
                } else {
                    None
                };

                if let Some((lint_name, kind)) = classification {
                    let allowed_lints = callee_name
                        .as_ref()
                        .map(|name| self.callee_allowed_lints(name, &inferred_item))
                        .unwrap_or_default();

                    if !allowed_lints.contains(&lint_name.to_string()) {
                        self.facts
                            .add_discarded_tail(item_span, kind, resolved.to_string());
                    }
                }
            }

            if let Some(cause) = inferred_item.diverges() {
                diverged_at = Some((i, cause));
            }

            new_items.push(inferred_item);
        }

        if let Some((diverged_index, cause)) = diverged_at
            && let Some(first_dead) = new_items.get(diverged_index + 1)
        {
            self.facts.add_dead_code(first_dead.get_span(), cause);
        }

        new_items
    }

    pub(super) fn error_name_not_found(&mut self, variable_name: &str, span: Span) {
        if self.imports.failed_imports.contains(variable_name) {
            return;
        }

        let mut available_names = self.scopes.collect_all_value_names();

        let module = self
            .store
            .get_module(&self.cursor.module_id)
            .expect("current module must exist in store");
        for qualified_name in module.definitions.keys() {
            let parts: Vec<&str> = qualified_name.rsplitn(2, '.').collect();
            if parts.len() == 2 {
                let module_name = parts[1];
                let name = parts[0];
                if module_name == module.id {
                    available_names.push(name.to_string());
                }
            }
        }

        self.sink.push(diagnostics::infer::name_not_found(
            variable_name,
            span,
            &available_names,
        ));
    }
}
