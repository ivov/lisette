use crate::Emitter;
use crate::control_flow::fallible::Fallible;
use crate::patterns::sites::PatternSubject;
use crate::types::coercion::{Coercion, CoercionDirection};
use crate::types::emitter::Destination;
use crate::utils::requires_temp_var;
use crate::write_line;
use syntax::ast::{Binding, Expression, Literal, Pattern, UnaryOperator};
use syntax::types::{Type, peel_to_range_type};

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
            LetKind::LetElse => {
                let else_block = self
                    .else_block
                    .expect("LetKind::LetElse classified without else block");
                self.emitter.emit_let_else_pattern_site(
                    output,
                    &self.binding.pattern,
                    self.binding.typed_pattern.as_ref(),
                    &self.binding.ty,
                    self.value,
                    else_block,
                );
            }
            LetKind::SimpleIdentifier => self.emit_simple_identifier(output),
            LetKind::Discard => self.emit_discard(output),
            LetKind::Propagate => self.emit_propagate(output),
            LetKind::MultiValueCall => self.emit_multi_value_call(output),
            LetKind::ComplexPattern => {
                let value_ty = self.value.get_type();
                self.emitter.emit_irrefutable_pattern_site(
                    output,
                    PatternSubject::expression(self.value, &self.binding.pattern, None),
                    &self.binding.pattern,
                    self.binding.typed_pattern.as_ref(),
                    &value_ty,
                );
            }
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
        let coercion = Coercion::resolve(
            self.emitter,
            &self.value.get_type(),
            &self.binding.ty,
            CoercionDirection::Internal,
        );
        let value_expression = coercion.apply(self.emitter, output, value_expression);
        let value_expression =
            maybe_clone_subslice(self.emitter, self.value, self.mutable, value_expression);

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

        if is_fn_alias_nominal(binding_ty) {
            let value_ty = self.value.get_type();
            if matches!(value_ty.unwrap_forall(), Type::Function { .. }) {
                return true;
            }
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

    fn binding_widens_to_interface(&self) -> bool {
        let binding_ty = &self.binding.ty;
        let value_ty = self.value.get_type();
        self.emitter.as_interface(binding_ty).is_some() && *binding_ty != value_ty
    }

    /// Pick the Go type for a `var X T` temp. Diverging values use the
    /// binding type so dead `return x` paths still typecheck; tuple
    /// branching values widen slots to match the assignment site.
    fn resolve_temp_var_decl_ty(&mut self) -> Type {
        let value_ty = self.value.get_type();
        let binding_ty = &self.binding.ty;
        if !value_ty.is_unit() && !value_ty.is_never() && self.binding_widens_to_interface() {
            return binding_ty.clone();
        }
        let base = if value_ty.is_unit() || value_ty.is_never() {
            if !binding_ty.is_unit() && !binding_ty.is_variable() {
                binding_ty.clone()
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
            self.emit_var_decl_if_needed(output, identifier);
            self.emitter.try_declare(identifier);
        }
        self.emit_value_to_temp(output, identifier);
    }

    fn emit_var_decl_if_needed(&mut self, output: &mut String, identifier: &str) {
        if identifier == "_" {
            return;
        }
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
            } else if let Some(ctx_ty) = self.emitter.return_mode.ty().cloned() {
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
    }

    /// Emit the value-producing expression into the already-declared temp var
    /// `identifier`. Branching expressions enter `Destination::Assign`;
    /// `Propagate`/`TryBlock`/`RecoverBlock` produce a value string assigned
    /// directly; `Loop` pushes the temp as its break-target before emitting.
    fn emit_value_to_temp(&mut self, output: &mut String, identifier: &str) {
        match self.value {
            Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                let value = self.value;
                let target_ty = self.binding.ty.clone();
                self.emitter.with_destination(
                    Destination::Assign {
                        var: identifier.to_string(),
                        target_ty: Some(target_ty),
                    },
                    |this| this.emit_branching_directly(output, value),
                );
            }
            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before emit")
            }
            Expression::Block { items, .. } => {
                let needs_braces = items.len() > 1;
                if needs_braces {
                    output.push_str("{\n");
                }
                let target_ty = self.binding.ty.clone();
                let value = self.value;
                self.emitter.with_destination(
                    Destination::Assign {
                        var: identifier.to_string(),
                        target_ty: Some(target_ty),
                    },
                    |this| {
                        this.emit_block_to_var_with_braces(output, value, identifier, needs_braces);
                    },
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

        if self.binding_widens_to_interface() {
            let var_ty = self.emitter.go_type_as_string(&self.binding.ty);
            write_line!(output, "var {} {}", go_identifier, var_ty);
            self.emitter.declare(&go_identifier);
        }

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
    let Type::Nominal {
        underlying_ty: Some(inner),
        ..
    } = ty.unwrap_forall()
    else {
        return false;
    };
    matches!(inner.unwrap_forall(), Type::Function { .. })
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

/// True when discarding `expression` is safe to omit — its value has no
/// side effects. `FormatString` and `Slice` literals are excluded since they
/// can hold sub-expressions that do.
fn is_side_effect_free_discard(expression: &Expression) -> bool {
    match expression {
        Expression::Unit { .. } => true,
        Expression::Literal { literal, .. } => matches!(
            literal,
            Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_)
        ),
        _ => false,
    }
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

    pub(crate) fn emit_discard(&mut self, output: &mut String, value: &Expression) {
        let unwrapped = value.unwrap_parens();

        if is_side_effect_free_discard(unwrapped) {
            return;
        }

        if let Expression::Propagate { expression, .. } = unwrapped {
            self.emit_propagate(output, expression, Some("_"));
            return;
        }

        let value_ty = value.get_type();
        if value_ty.is_unit() || value_ty.is_variable() || value_ty.is_never() {
            let value_expression = self.emit_operand(output, value);
            if !value_expression.is_empty() {
                if matches!(unwrapped, Expression::Call { .. }) {
                    write_line!(output, "{}", value_expression);
                } else {
                    write_line!(output, "_ = {}", value_expression);
                }
            }
            return;
        }

        if let Expression::Call { .. } = unwrapped
            && let Some(raw) = self.emit_go_call_discarded(output, unwrapped)
        {
            write_line!(output, "{}", raw);
            return;
        }

        let is_lowered_lisette_call = if let Expression::Call {
            expression: callee, ..
        } = unwrapped
        {
            self.classify_callee_abi(callee).is_some()
        } else {
            false
        };
        if is_lowered_lisette_call {
            let call_str = self.emit_call(output, value, None);
            write_line!(output, "{}", call_str);
            return;
        }

        let value_expression = self.emit_operand(output, value);
        write_line!(output, "_ = {}", value_expression);
    }
}

/// `let mut x = arr[range]` would otherwise alias the backing array.
fn maybe_clone_subslice(
    emitter: &mut Emitter<'_>,
    value: &Expression,
    mutable: bool,
    expression: String,
) -> String {
    if !is_mutable_subslice(value, mutable) {
        return expression;
    }
    emitter.flags.needs_slices = true;
    format!("slices.Clone({})", expression)
}

fn is_mutable_subslice(value: &Expression, mutable: bool) -> bool {
    if !mutable {
        return false;
    }
    let value = value.unwrap_parens();
    let Expression::IndexedAccess {
        expression, index, ..
    } = value
    else {
        return false;
    };

    let is_range_index = matches!(**index, Expression::Range { .. })
        || peel_to_range_type(&index.get_type()).is_some();

    if !is_range_index {
        return false;
    }

    let collection_ty = match expression.as_ref() {
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => {
            let inner_ty = inner.get_type();
            inner_ty.inner().unwrap_or(inner_ty)
        }
        other => other.get_type(),
    };
    collection_ty.has_name("Slice")
}
