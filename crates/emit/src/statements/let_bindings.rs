use crate::Emitter;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::control_flow::fallible::Fallible;
use crate::patterns::decision_tree;
use crate::types::coercion::Coercion;
use crate::types::emitter::Position;
use crate::utils::{DiscardGuard, requires_temp_var, try_flip_comparison};
use crate::write_line;
use syntax::ast::{Binding, Expression, Pattern};
use syntax::types::Type;

enum LetKind {
    /// Simple identifier binding: `let x = expression`
    SimpleIdentifier,
    /// Discard pattern: `let _ = expression`
    Discard,
    /// Complex pattern with temp var: `let (a, b) = expression`
    ComplexPattern,
    /// Go multi-value call optimization: `let (a, b) = go_func()`
    MultiValueCall,
    /// Propagation: `let x = expression?`
    Propagate,
    /// Let-else binding: `let P = expression else { ... }`
    LetElse,
}

pub(crate) struct LetEmitter<'a, 'e> {
    emitter: &'a mut Emitter<'e>,
    binding: &'a Binding,
    value: &'a Expression,
    else_block: Option<&'a Expression>,
    mutable: bool,
}

impl<'a, 'e> LetEmitter<'a, 'e> {
    pub(crate) fn new(
        emitter: &'a mut Emitter<'e>,
        binding: &'a Binding,
        value: &'a Expression,
        else_block: Option<&'a Expression>,
        mutable: bool,
    ) -> Self {
        Self {
            emitter,
            binding,
            value,
            else_block,
            mutable,
        }
    }

    pub(crate) fn emit(mut self, output: &mut String) {
        // Never-typed values diverge (break/continue/return).
        // Declare the binding variable (so later dead code can reference it),
        // then emit the value as a statement.
        if self.value.get_type().is_never() {
            self.emit_never_binding(output);
            return;
        }
        match self.classify() {
            LetKind::LetElse => self.emit_let_else(output),
            LetKind::SimpleIdentifier => self.emit_simple_identifier(output),
            LetKind::Discard => self.emit_discard(output),
            LetKind::Propagate => self.emit_propagate(output),
            LetKind::MultiValueCall => self.emit_multi_value_call(output),
            LetKind::ComplexPattern => self.emit_complex_pattern(output),
        }
    }

    /// Handle a let binding whose value expression diverges (Never type).
    /// Declare the variable with its zero value so dead code can reference it,
    /// then emit the diverging value as a statement.
    fn emit_never_binding(&mut self, output: &mut String) {
        if let Pattern::Identifier { identifier, .. } = &self.binding.pattern
            && let Some(raw_go_name) = self.emitter.go_name_for_binding(&self.binding.pattern)
        {
            let go_identifier = self.emitter.scope.bindings.add(identifier, &raw_go_name);
            self.emitter.try_declare(&go_identifier);
            let var_ty = self.emitter.go_type_as_string(&self.binding.ty);
            write_line!(output, "var {} {}", go_identifier, var_ty);
        }
        self.emitter.emit_statement(output, self.value);
    }

    fn classify(&self) -> LetKind {
        if self.else_block.is_some() {
            return LetKind::LetElse;
        }

        match &self.binding.pattern {
            Pattern::Identifier { .. } => {
                if matches!(self.value, Expression::Propagate { .. }) {
                    LetKind::Propagate
                } else {
                    LetKind::SimpleIdentifier
                }
            }
            Pattern::WildCard { .. } => LetKind::Discard,
            Pattern::Tuple { elements, .. } => {
                let all_unused = elements.iter().all(|el| match el {
                    Pattern::WildCard { .. } => true,
                    Pattern::Identifier { .. } => self.emitter.ctx.unused.is_unused_binding(el),
                    _ => false,
                });
                if all_unused {
                    LetKind::Discard
                } else if self.can_use_multi_value_optimization() {
                    LetKind::MultiValueCall
                } else {
                    LetKind::ComplexPattern
                }
            }
            _ => LetKind::ComplexPattern,
        }
    }

    /// Check if we can use Go multi-value call optimization.
    ///
    /// This optimization applies when:
    /// 1. The pattern is a tuple of simple patterns (identifiers/wildcards)
    /// 2. The value is a Go function call returning multiple values
    /// 3. The result type is not Result (which needs wrapping)
    fn can_use_multi_value_optimization(&self) -> bool {
        let Pattern::Tuple { .. } = &self.binding.pattern else {
            return false;
        };

        self.emitter
            .resolve_go_call_strategy(self.value)
            .is_some_and(|s| s.is_multi_return())
            && !self.value.get_type().is_result()
            && extract_simple_tuple_vars(&self.binding.pattern).is_some()
    }

    fn emit_simple_identifier(&mut self, output: &mut String) {
        let Pattern::Identifier { identifier, .. } = &self.binding.pattern else {
            unreachable!("emit_simple_identifier called with non-identifier pattern");
        };

        if self.value.get_type().is_unit()
            && matches!(self.value.unwrap_parens(), Expression::Call { .. })
        {
            self.emit_unit_call_binding(output, identifier);
            return;
        }

        let Some(raw_go_name) = self.emitter.go_name_for_binding(&self.binding.pattern) else {
            // Register `_` in scope so any later reassignment (`x = value`)
            // resolves to `_ = value` instead of emitting the undeclared name.
            self.emitter.scope.bindings.add(identifier.as_str(), "_");
            if requires_temp_var(self.value) {
                self.emit_temp_var_binding(output, "_");
            } else {
                self.emitter.emit_discard(output, self.value);
            }
            return;
        };

        if requires_temp_var(self.value) {
            let go_identifier = crate::escape_reserved(&raw_go_name);
            if self.emitter.is_declared(&go_identifier)
                || expression_contains_binding(self.value, identifier)
            {
                let fresh = self.emitter.fresh_var(Some(identifier));
                self.emit_temp_var_binding(output, &fresh);
                self.emitter.scope.bindings.add(identifier, &fresh);
            } else {
                self.emitter.scope.bindings.add(identifier, &raw_go_name);
                self.emit_temp_var_binding(output, &go_identifier);
            }
            return;
        }

        self.emit_direct_value_binding(output, identifier, &raw_go_name);
    }

    /// Unit-returning call bindings (`let x = foo()` where `foo(): unit`):
    /// emit the call as a statement, then declare the binding as `struct{}{}`.
    /// A new fresh var is taken if the preferred name is already declared.
    fn emit_unit_call_binding(&mut self, output: &mut String, identifier: &str) {
        let value_expression = self.emitter.emit_value(output, self.value);
        write_line!(output, "{}", value_expression);

        let Some(raw_go_name) = self.emitter.go_name_for_binding(&self.binding.pattern) else {
            return;
        };
        let go_identifier = crate::escape_reserved(&raw_go_name);
        if self.emitter.is_declared(&go_identifier) {
            let fresh = self.emitter.fresh_var(Some(identifier));
            self.emitter.declare(&fresh);
            write_line!(output, "{} := struct{{}}{{}}", fresh);
            self.emitter.scope.bindings.add(identifier, &fresh);
        } else {
            let go_identifier = self.emitter.scope.bindings.add(identifier, &raw_go_name);
            self.emitter.try_declare(&go_identifier);
            write_line!(output, "{} := struct{{}}{{}}", go_identifier);
        }
    }

    /// Emit a direct-value binding (no temp var needed): compute the RHS,
    /// optionally wrap for interface coercion or clone for mutable sub-slices,
    /// then emit `var` / `:=` / fresh-name depending on scope conditions.
    fn emit_direct_value_binding(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: &str,
    ) {
        let value_expression = self.emitter.emit_value(output, self.value);
        let coercion = Coercion::resolve(self.emitter, &self.value.get_type(), &self.binding.ty);
        let value_expression = coercion.apply(self.emitter, output, value_expression);
        let clone = Coercion::resolve_subslice_clone(self.value, self.mutable);
        let value_expression = clone.apply(self.emitter, output, value_expression);

        let go_identifier = self.emitter.scope.bindings.add(identifier, raw_go_name);
        let is_new = self.emitter.try_declare(&go_identifier);

        if !is_new || self.emitter.scope.assign_targets.contains(&go_identifier) {
            let fresh = self.emitter.fresh_var(Some(identifier));
            self.emitter.scope.bindings.add(identifier, &fresh);
            self.emitter.try_declare(&fresh);
            write_line!(output, "{} := {}", fresh, value_expression);
        } else if self.needs_explicit_type_declaration() {
            let var_ty = self.emitter.go_type_as_string(&self.binding.ty);
            write_line!(
                output,
                "var {} {} = {}",
                go_identifier,
                var_ty,
                value_expression
            );
        } else {
            write_line!(output, "{} := {}", go_identifier, value_expression);
        }
    }

    /// Check if we need explicit type declaration for this binding.
    ///
    /// This is needed when:
    /// 1. The value is a literal (integer or float), possibly negated
    /// 2. The binding type differs from Go's default inference for that literal
    /// 3. The binding type is an interface (Go's := would infer the concrete type)
    /// 4. The binding type is a defined-fn-type alias and the value emits as a
    ///    bare `func(...)` literal — without `var`, Go infers the anonymous
    ///    func type and `&binding` no longer matches `*Alias` at call sites.
    fn needs_explicit_type_declaration(&self) -> bool {
        let binding_ty = &self.binding.ty;

        if self.emitter.as_interface(binding_ty).is_some() {
            let value_ty = self.value.get_type();
            if *binding_ty != value_ty {
                return true;
            }
        }

        if is_fn_alias_nominal(binding_ty)
            && matches!(self.value.unwrap_parens(), Expression::Lambda { .. })
        {
            return true;
        }

        let inner_value = unwrap_unary_negation(self.value);

        match inner_value {
            Expression::Literal { literal, .. } => match literal {
                syntax::ast::Literal::Integer { .. } => {
                    let type_name = binding_ty.get_name();
                    !matches!(type_name, Some("int") | None)
                }
                syntax::ast::Literal::Float { .. } => {
                    let type_name = binding_ty.get_name();
                    !matches!(type_name, Some("float64") | None)
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Pick the Go type for a `var X T` temp. Diverging values use the
    /// binding type so dead `return x` paths still typecheck; tuple
    /// branching values widen slots to match the assignment site.
    fn resolve_temp_var_decl_ty(&mut self) -> Type {
        let value_ty = self.value.get_type();
        let base = if value_ty.is_unit() || value_ty.is_never() {
            let binding_ty = &self.binding.ty;
            if !binding_ty.is_unit() && !binding_ty.is_variable() {
                self.binding.ty.clone()
            } else {
                value_ty
            }
        } else {
            value_ty
        };
        let is_branching = matches!(
            self.value,
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. }
        );
        if is_branching && let Type::Tuple(slots) = &base {
            Type::Tuple(self.emitter.resolve_tuple_slot_types(slots.clone()))
        } else {
            base
        }
    }

    fn emit_temp_var_binding(&mut self, output: &mut String, identifier: &str) {
        if !self.emitter.is_declared(identifier) {
            let resolved_ty = self.resolve_temp_var_decl_ty();
            let ty = &resolved_ty;

            // When a try/recover block's ok_ty is an unresolved variable, the
            // var decl would be `Result[any, ...]`. Use the binding type if it
            // has a resolved ok_ty, or fall back to the return context type.
            let has_variable_ok_ty = matches!(
                self.value,
                Expression::TryBlock { .. } | Expression::RecoverBlock { .. }
            ) && !ty.is_variable()
                && ty.ok_type().is_variable();

            let var_ty = if has_variable_ok_ty {
                let binding_ty = &self.binding.ty;
                if !binding_ty.is_variable() && !binding_ty.ok_type().is_variable() {
                    self.emitter.go_type_as_string(binding_ty)
                } else if let Some(ctx_ty) = self
                    .emitter
                    .current_return_context
                    .as_ref()
                    .map(|c| c.ty.clone())
                {
                    if Fallible::from_type(&ctx_ty).is_some() {
                        self.emitter.go_type_as_string(&ctx_ty)
                    } else {
                        self.emitter.go_type_as_string(ty)
                    }
                } else {
                    self.emitter.go_type_as_string(ty)
                }
            } else {
                self.emitter.go_type_as_string(ty)
            };
            write_line!(output, "var {} {}", identifier, var_ty);
            self.emitter.try_declare(identifier);
        }

        let saved_target_ty = self
            .emitter
            .assign_target_ty
            .replace(self.binding.ty.clone());

        self.emit_value_to_temp(output, identifier);

        self.emitter.assign_target_ty = saved_target_ty;
    }

    /// Emit the value-producing expression into the already-declared temp var
    /// `identifier`. Branching expressions position themselves in `Assign(id)`;
    /// `Propagate`/`TryBlock`/`RecoverBlock` produce a value string assigned
    /// directly; `Loop` pushes the temp as its break-target before emitting.
    fn emit_value_to_temp(&mut self, output: &mut String, identifier: &str) {
        match self.value {
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                let value = self.value;
                self.emitter
                    .with_position(Position::Assign(identifier.to_string()), |this| {
                        this.emit_branching_directly(output, value)
                    });
            }
            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before emit")
            }
            Expression::Block { items, .. } => {
                let needs_braces = items.len() > 1;
                if needs_braces {
                    output.push_str("{\n");
                }
                self.emitter.emit_block_to_var_with_braces(
                    output,
                    self.value,
                    identifier,
                    needs_braces,
                );
                if needs_braces {
                    output.push_str("}\n");
                }
            }
            Expression::Loop {
                body, needs_label, ..
            } => {
                self.emitter.push_loop(identifier);
                self.emitter
                    .emit_labeled_loop(output, "for {\n", body, *needs_label);
                self.emitter.pop_loop();
            }
            Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. } => {
                let value_expression = self.emitter.emit_value(output, self.value);
                write_line!(output, "{} = {}", identifier, value_expression);
            }
            _ => unreachable!("requires_temp_var returned true for unexpected expression"),
        }
    }

    fn emit_discard(&mut self, output: &mut String) {
        self.emitter.emit_discard(output, self.value);
    }

    fn emit_propagate(&mut self, output: &mut String) {
        let Pattern::Identifier { identifier, .. } = &self.binding.pattern else {
            unreachable!("emit_propagate called with non-identifier pattern");
        };

        let Some(go_name) = self.emitter.go_name_for_binding(&self.binding.pattern) else {
            self.emitter.scope.bindings.add(identifier.as_str(), "_");
            self.emitter.emit_propagate_to_let(output, "_", self.value);
            return;
        };

        let go_identifier = crate::escape_reserved(&go_name).into_owned();
        let go_identifier = if self.emitter.is_declared(&go_identifier) {
            self.emitter.fresh_var(Some(identifier))
        } else {
            go_identifier
        };

        self.emitter
            .emit_propagate_to_let(output, &go_identifier, self.value);

        self.emitter.scope.bindings.add(identifier, &go_identifier);
        self.emitter.try_declare(&go_identifier);
    }

    fn emit_multi_value_call(&mut self, output: &mut String) {
        let Pattern::Tuple { elements, .. } = &self.binding.pattern else {
            unreachable!("emit_multi_value_call called with non-tuple pattern");
        };

        let vars = extract_simple_tuple_vars(&self.binding.pattern)
            .expect("multi-value optimization requires simple tuple vars");

        let mut any_new = false;
        let mut planned: Vec<Option<(&str, String)>> = Vec::new();
        let go_vars: Vec<String> = vars
            .iter()
            .zip(elements.iter())
            .map(|(var, pat)| {
                if var == "_" {
                    planned.push(None);
                    "_".to_string()
                } else if let Pattern::Identifier { identifier, .. } = pat
                    && let Some(go_name) = self.emitter.go_name_for_binding(pat)
                {
                    let escaped = crate::escape_reserved(&go_name).into_owned();
                    let name = if self.emitter.is_declared(&escaped) {
                        let fresh = self.emitter.fresh_var(Some(identifier));
                        any_new = true;
                        fresh
                    } else {
                        any_new = true;
                        escaped
                    };
                    planned.push(Some((identifier, name.clone())));
                    name
                } else {
                    planned.push(None);
                    "_".to_string()
                }
            })
            .collect();

        let call_str = self.emitter.emit_call(output, self.value, None);

        for (identifier, go_name) in planned.iter().flatten() {
            self.emitter.scope.bindings.add(*identifier, go_name);
            self.emitter.try_declare(go_name);
        }

        let op = if any_new { ":=" } else { "=" };
        write_line!(output, "{} {} {}", go_vars.join(", "), op, call_str);
    }

    /// Emit a complex pattern binding: `let (a, Point { x, y }) = expression`
    ///
    /// This creates a temp variable and destructures from it.
    fn emit_complex_pattern(&mut self, output: &mut String) {
        if let Expression::Identifier { value, .. } = self.value
            && !value.contains('.')
        {
            let go_name = self
                .emitter
                .scope
                .bindings
                .get(value)
                .map(|s| s.to_string())
                .unwrap_or_else(|| crate::escape_reserved(value).into_owned());
            let (_checks, bindings) = decision_tree::collect_pattern_info(
                self.emitter,
                &self.binding.pattern,
                self.binding.typed_pattern.as_ref(),
            );
            decision_tree::emit_tree_bindings(self.emitter, output, &bindings, &go_name);
            return;
        }

        let temp_var = self.emitter.fresh_var(None);
        self.emitter.declare(&temp_var);
        let value_expression = self.emitter.emit_value(output, self.value);
        write_line!(output, "{} := {}", temp_var, value_expression);

        let guard = DiscardGuard::new(output, &temp_var);
        let (_checks, bindings) = decision_tree::collect_pattern_info(
            self.emitter,
            &self.binding.pattern,
            self.binding.typed_pattern.as_ref(),
        );
        decision_tree::emit_tree_bindings(self.emitter, output, &bindings, &temp_var);
        guard.finish(output);
    }

    /// Emit a let-else binding: `let P = expression else { ... }`
    fn emit_let_else(&mut self, output: &mut String) {
        let else_block = self
            .else_block
            .expect("emit_let_else called without else block");

        let (subject_var, needs_guard) = if let Expression::Identifier { value, .. } = self.value {
            let has_collision = Emitter::pattern_binds_name(&self.binding.pattern, value);
            if !has_collision && !value.contains('.') {
                let go_name = self
                    .emitter
                    .scope
                    .bindings
                    .get(value)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| crate::escape_reserved(value).into_owned());
                (go_name, false)
            } else {
                let var = self.emitter.fresh_var(Some("subject"));
                self.emitter.declare(&var);
                let value_expression = self.emitter.emit_value(output, self.value);
                write_line!(output, "{} := {}", var, value_expression);
                (var, true)
            }
        } else {
            let var = self.emitter.fresh_var(Some("subject"));
            self.emitter.declare(&var);
            let value_expression = self.emitter.emit_value(output, self.value);
            write_line!(output, "{} := {}", var, value_expression);
            (var, true)
        };

        let subject_guard = if needs_guard {
            Some(DiscardGuard::new(output, &subject_var))
        } else {
            None
        };

        if let Pattern::Or { patterns, .. } = &self.binding.pattern {
            self.emit_or_pattern_let_else(output, patterns, &subject_var, else_block);
            if let Some(guard) = subject_guard {
                guard.finish(output);
            }
            return;
        }

        let (checks, bindings) = decision_tree::collect_pattern_info(
            self.emitter,
            &self.binding.pattern,
            self.binding.typed_pattern.as_ref(),
        );

        if checks.is_empty() {
            decision_tree::emit_tree_bindings(self.emitter, output, &bindings, &subject_var);
        } else {
            let condition = decision_tree::render_condition(&checks, &subject_var);
            let guard = if checks.len() == 1 {
                negate_condition(&condition)
            } else {
                format!("!({})", condition)
            };
            let guard = wrap_if_struct_literal(guard);
            write_line!(output, "if {} {{", guard);
            self.emitter.emit_block(output, else_block);
            output.push_str("}\n");

            decision_tree::emit_tree_bindings(self.emitter, output, &bindings, &subject_var);
        }

        if let Some(guard) = subject_guard {
            guard.finish(output);
        }
    }

    /// Emit let-else with or-pattern: `let A | B = expression else { ... }`
    fn emit_or_pattern_let_else(
        &mut self,
        output: &mut String,
        patterns: &[Pattern],
        subject_var: &str,
        else_block: &Expression,
    ) {
        let outer_snapshot = self.emitter.scope.bindings.snapshot();

        self.emitter.emit_binding_declarations_with_type(
            output,
            &self.binding.pattern,
            &self.binding.ty,
            self.binding.typed_pattern.as_ref(),
        );

        let pattern_snapshot = self.emitter.scope.bindings.snapshot();

        let collected: Vec<_> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self.emitter, alt, None))
            .collect();

        if collected.iter().any(|(checks, _)| checks.is_empty()) {
            return;
        }

        for (i, (checks, bindings)) in collected.iter().enumerate() {
            let condition = decision_tree::render_condition(checks, subject_var);

            if i == 0 {
                write_line!(output, "if {} {{", condition);
            } else {
                write_line!(output, "}} else if {} {{", condition);
            }

            decision_tree::emit_tree_assignments(self.emitter, output, bindings, subject_var);
        }

        // Restore outer bindings for else body.
        self.emitter.scope.bindings.restore_snapshot(outer_snapshot);
        output.push_str("} else {\n");
        self.emitter.emit_block(output, else_block);
        output.push_str("}\n");

        // Re-apply pattern bindings for post-else code.
        self.emitter
            .scope
            .bindings
            .restore_snapshot(pattern_snapshot);
    }
}

/// Extracts variable names from a tuple pattern for direct Go multi-value destructuring.
///
/// Returns `Some(vec)` if all elements are simple (identifiers or wildcards),
/// `None` if any element is complex (nested tuple, struct, etc.).
///
/// - Identifiers become their name
/// - Wildcards become "_"
fn extract_simple_tuple_vars(pattern: &Pattern) -> Option<Vec<String>> {
    let Pattern::Tuple { elements, .. } = pattern else {
        return None;
    };

    let mut vars = Vec::with_capacity(elements.len());

    for element in elements {
        match element {
            Pattern::Identifier { identifier, .. } => {
                vars.push(identifier.to_string());
            }
            Pattern::WildCard { .. } => {
                vars.push("_".to_string());
            }
            _ => return None,
        }
    }

    Some(vars)
}

/// Unwrap unary negation to get the underlying expression.
/// This handles `-1`, `-1.0`, etc. for type declaration checks.
fn unwrap_unary_negation(expression: &Expression) -> &Expression {
    match expression {
        Expression::Unary {
            operator: syntax::ast::UnaryOperator::Negative,
            expression,
            ..
        } => expression.as_ref(),
        Expression::Paren { expression, .. } => unwrap_unary_negation(expression),
        _ => expression,
    }
}

fn is_fn_alias_nominal(ty: &Type) -> bool {
    let resolved = match ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let Type::Nominal {
        underlying_ty: Some(inner),
        ..
    } = resolved
    else {
        return false;
    };
    let inner = match inner.as_ref() {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    matches!(inner, Type::Function { .. })
}

/// Check if an expression contains a binding with the given name.
fn expression_contains_binding(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Match { arms, .. } => arms
            .iter()
            .any(|arm| pattern_contains_name(&arm.pattern, name)),
        Expression::Block { items, .. } => items.iter().any(|item| match item {
            Expression::Let { binding, .. } => pattern_contains_name(&binding.pattern, name),
            _ => false,
        }),
        Expression::If {
            consequence,
            alternative,
            ..
        } => {
            expression_contains_binding(consequence, name)
                || expression_contains_binding(alternative, name)
        }
        Expression::Select { arms, .. } => arms.iter().any(|arm| {
            use syntax::ast::SelectArmPattern;
            match &arm.pattern {
                SelectArmPattern::Receive { binding, .. } => pattern_contains_name(binding, name),
                SelectArmPattern::MatchReceive { arms, .. } => {
                    arms.iter().any(|a| pattern_contains_name(&a.pattern, name))
                }
                _ => false,
            }
        }),
        Expression::Loop { body, .. } => expression_contains_binding(body, name),
        _ => false,
    }
}

/// Check if a pattern contains an identifier binding with the given name.
fn pattern_contains_name(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Identifier { identifier, .. } => identifier.as_str() == name,
        Pattern::EnumVariant { fields, .. } => {
            fields.iter().any(|f| pattern_contains_name(f, name))
        }
        Pattern::Struct { fields, .. } => {
            fields.iter().any(|f| pattern_contains_name(&f.value, name))
        }
        Pattern::Tuple { elements, .. } => elements.iter().any(|e| pattern_contains_name(e, name)),
        Pattern::Slice { prefix, rest, .. } => {
            prefix.iter().any(|p| pattern_contains_name(p, name))
                || matches!(rest, syntax::ast::RestPattern::Bind { name: n, .. } if n == name)
        }
        Pattern::Or { patterns, .. } => patterns.iter().any(|p| pattern_contains_name(p, name)),
        Pattern::AsBinding {
            pattern,
            name: as_name,
            ..
        } => as_name == name || pattern_contains_name(pattern, name),
        Pattern::Literal { .. } | Pattern::Unit { .. } | Pattern::WildCard { .. } => false,
    }
}

fn negate_condition(condition: &str) -> String {
    try_flip_comparison(condition).unwrap_or_else(|| format!("!({})", condition))
}

impl Emitter<'_> {
    pub(crate) fn emit_let(
        &mut self,
        output: &mut String,
        binding: &Binding,
        value: &Expression,
        else_block: Option<&Expression>,
        mutable: bool,
    ) {
        LetEmitter::new(self, binding, value, else_block, mutable).emit(output);
    }

    /// Handles Go multi-value returns correctly.
    pub(crate) fn emit_discard(&mut self, output: &mut String, value: &Expression) {
        if let Expression::Propagate { expression, .. } = value.unwrap_parens() {
            self.emit_propagate(output, expression, Some("_"));
            return;
        }
        let value_ty = value.get_type();
        if value_ty.is_unit() || value_ty.is_variable() {
            let value_expression = self.emit_operand(output, value);
            if !value_expression.is_empty() {
                if matches!(value.unwrap_parens(), Expression::Call { .. }) {
                    write_line!(output, "{}", value_expression);
                } else {
                    write_line!(output, "_ = {}", value_expression);
                }
            }
            return;
        }

        let is_go_multi_return = self
            .resolve_go_call_strategy(value)
            .is_some_and(|s| s.is_multi_return());
        let is_lowered_lisette_call = if let Expression::Call {
            expression: callee, ..
        } = value
        {
            self.classify_callee_abi(callee).is_some()
        } else {
            false
        };
        if is_go_multi_return || is_lowered_lisette_call {
            let call_str = self.emit_call(output, value, None);
            write_line!(output, "{}", call_str);
        } else {
            let value_expression = self.emit_operand(output, value);
            write_line!(output, "_ = {}", value_expression);
        }
    }
}
