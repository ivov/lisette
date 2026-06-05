use crate::EmitEffects;
use crate::Planner;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::calls::go_interop::{GoCallStrategy, WrapperTarget};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::Fallible;
use crate::escape_reserved;
use crate::patterns::sites::{AnnotatedPattern, PatternSubject};
use crate::plan::bodies::{LetForm, LetPlan, LoweredBlock, LoweredStatement};
use crate::plan::calls::CalleePlan;
use crate::plan::placement::{expression_contains_binding, is_unit_call, requires_temp_var};
use crate::types::native::NativeGoType;
use crate::write_line;
use syntax::ast::{Binding, Expression, Literal, Pattern, UnaryOperator};
use syntax::types::{Type, peel_to_range_type};

#[derive(Clone, Copy)]
pub(crate) struct LetSpec<'a> {
    pub(crate) identifier: &'a str,
    pub(crate) value: &'a Expression,
    pub(crate) binding_ty: &'a Type,
    pub(crate) mutable: bool,
}

fn needs_explicit_type_declaration(
    planner: &Planner,
    value: &Expression,
    binding_ty: &Type,
) -> bool {
    if planner.facts.as_interface(binding_ty).is_some() {
        let value_ty = value.get_type();
        if *binding_ty != value_ty {
            return true;
        }
    }
    if is_fn_alias_nominal(binding_ty) {
        let value_ty = value.get_type();
        if matches!(value_ty.unwrap_forall(), Type::Function(_)) {
            return true;
        }
    }
    match unwrap_unary_negation(value) {
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { .. } => !matches!(binding_ty.get_name(), Some("int") | None),
            Literal::Float { .. } => !matches!(binding_ty.get_name(), Some("float64") | None),
            Literal::String { .. } => !matches!(binding_ty.get_name(), Some("string") | None),
            Literal::Boolean(_) => !matches!(binding_ty.get_name(), Some("bool") | None),
            _ => false,
        },
        _ => false,
    }
}

fn unwrap_unary_negation(expression: &Expression) -> &Expression {
    match expression {
        Expression::Unary {
            operator: UnaryOperator::Negative,
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
    matches!(inner.unwrap_forall(), Type::Function(_))
}

/// `let mut x = arr[range]` would otherwise alias the backing array.
fn maybe_clone_subslice(
    planner: &mut Planner,
    value: &Expression,
    mutable: bool,
    expression: String,
    fx: &mut EmitEffects,
) -> String {
    if !is_mutable_subslice(planner, value, mutable) {
        return expression;
    }
    fx.require_slices();
    format!("slices.Clone({})", expression)
}

fn is_mutable_subslice(planner: &Planner, value: &Expression, mutable: bool) -> bool {
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
    let collection_ty = if let Some(inner) = expression.deref_inner() {
        let inner_ty = inner.get_type();
        inner_ty.inner().unwrap_or(inner_ty)
    } else {
        expression.get_type()
    };
    planner.is_native_shape(&collection_ty, NativeGoType::Slice)
}

/// Pick the Go type for a `let` binding's `var X T` temp. Diverging values
/// use the binding type so dead `return x` paths still typecheck; branching
/// values that produce tuples widen slots to match the assignment site.
fn resolve_let_temp_declaration_ty(
    planner: &mut Planner,
    value: &Expression,
    binding_ty: &Type,
) -> Type {
    let value_ty = value.get_type();
    let widens_to_interface =
        planner.facts.as_interface(binding_ty).is_some() && *binding_ty != value_ty;
    if !value_ty.is_unit() && !value_ty.is_never() && widens_to_interface {
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
        value,
        Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. }
    );
    if is_branching && let Type::Tuple(slots) = &base {
        Type::Tuple(planner.resolve_tuple_slot_types(slots.clone(), false))
    } else {
        base
    }
}

impl Planner<'_> {
    fn choose_let_go_name(
        &mut self,
        identifier: &str,
        raw_go_name: &str,
        force_fresh: bool,
    ) -> String {
        let escaped = escape_reserved(raw_go_name);
        if force_fresh || self.is_declared(&escaped) {
            self.fresh_var(Some(identifier))
        } else {
            escaped.into_owned()
        }
    }

    /// Emit a `let identifier = value` binding; `raw_go_name == None` is
    /// unused.
    pub(crate) fn emit_let_value(
        &mut self,
        output: &mut String,
        let_spec: LetSpec,
        raw_go_name: Option<&str>,
        fx: &mut EmitEffects,
    ) {
        let LetSpec {
            identifier,
            value,
            binding_ty,
            ..
        } = let_spec;
        if is_unit_call(value) {
            self.emit_let_unit_call(output, identifier, raw_go_name, value, fx);
            return;
        }
        let needs_temp = requires_temp_var(value);
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind(identifier, "_");
            if needs_temp {
                self.emit_let_temp(output, "_", value, binding_ty, fx);
            } else {
                self.emit_discard(output, value, fx);
            }
            return;
        };
        if needs_temp {
            let go_identifier = escape_reserved(raw_go_name);
            if self.is_declared(&go_identifier) || expression_contains_binding(value, identifier) {
                let fresh = self.fresh_var(Some(identifier));
                self.emit_let_temp(output, &fresh, value, binding_ty, fx);
                self.scope.bind(identifier, &fresh);
            } else {
                self.scope.bind(identifier, raw_go_name);
                self.emit_let_temp(output, &go_identifier, value, binding_ty, fx);
            }
            return;
        }
        self.emit_let_direct(output, let_spec, raw_go_name, fx);
    }

    /// `let x = expr?`. Adds a leading `var x T` when the binding widens to
    /// an interface.
    pub(crate) fn lower_let_propagate(
        &mut self,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
        binding_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let Expression::Propagate {
            expression: inner, ..
        } = value
        else {
            unreachable!("lower_let_propagate requires a Propagate value");
        };
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind(identifier, "_");
            return self.lower_propagate(inner, Some("_"), fx).0;
        };
        let go_identifier = self.choose_let_go_name(identifier, raw_go_name, false);
        let widens_to_interface =
            self.facts.is_interface(binding_ty) && *binding_ty != value.get_type();
        let mut statements = Vec::new();
        if widens_to_interface {
            let var_ty = self.go_type_string(binding_ty, fx);
            statements.push(LoweredStatement::RawGo(format!(
                "var {} {}\n",
                go_identifier, var_ty
            )));
            self.declare(&go_identifier);
        }
        statements.extend(self.lower_propagate(inner, Some(&go_identifier), fx).0);
        self.scope.bind(identifier, &go_identifier);
        self.try_declare(&go_identifier);
        statements
    }

    /// `let x = foo()` where `foo()` returns unit: emit the call as a
    /// statement, then declare the binding as `struct{}{}`.
    fn emit_let_unit_call(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
        fx: &mut EmitEffects,
    ) {
        let value_expression = self.emit_value(output, value, ExpressionContext::value(), fx);
        write_line!(output, "{}", value_expression);
        let Some(raw_go_name) = raw_go_name else {
            return;
        };
        let escaped = escape_reserved(raw_go_name);
        if self.is_declared(&escaped) {
            let fresh = self.fresh_var(Some(identifier));
            self.declare(&fresh);
            write_line!(output, "{} := struct{{}}{{}}", fresh);
            self.scope.bind(identifier, &fresh);
        } else {
            let go_identifier = self.scope.bind(identifier, raw_go_name);
            self.try_declare(&go_identifier);
            write_line!(output, "{} := struct{{}}{{}}", go_identifier);
        }
    }

    fn emit_let_direct(
        &mut self,
        output: &mut String,
        let_spec: LetSpec,
        raw_go_name: &str,
        fx: &mut EmitEffects,
    ) {
        let LetSpec {
            identifier,
            value,
            binding_ty,
            mutable,
        } = let_spec;
        if !mutable
            && self.try_emit_let_into_wrapper_slot(
                output,
                identifier,
                raw_go_name,
                value,
                binding_ty,
                fx,
            )
        {
            return;
        }

        let value_expression = self.emit_value(output, value, ExpressionContext::value(), fx);
        let coercion = Coercion::resolve(
            self,
            &value.get_type(),
            binding_ty,
            CoercionDirection::Internal,
        );
        let value_expression = coercion.apply(self, output, value_expression, fx);
        let value_expression = maybe_clone_subslice(self, value, mutable, value_expression, fx);

        let go_identifier = self.scope.bind(identifier, raw_go_name);
        let is_new = self.try_declare(&go_identifier);

        if !is_new || self.scope.is_active_assign_target(&go_identifier) {
            let fresh = self.fresh_var(Some(identifier));
            self.scope.bind(identifier, &fresh);
            self.try_declare(&fresh);
            write_line!(output, "{} := {}", fresh, value_expression);
        } else if needs_explicit_type_declaration(self, value, binding_ty) {
            let var_ty = self.go_type_string(binding_ty, fx);
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

    /// Route a slot-style Go-interop wrapper into the let's Go name, removing
    /// the `name := result_N` alias.
    fn try_emit_let_into_wrapper_slot(
        &mut self,
        output: &mut String,
        identifier: &str,
        raw_go_name: &str,
        value: &Expression,
        binding_ty: &Type,
        fx: &mut EmitEffects,
    ) -> bool {
        let go_identifier = escape_reserved(raw_go_name);
        if self.is_declared(&go_identifier)
            || self.scope.is_active_assign_target(&go_identifier)
            || self.scope.has_binding_for_go_name(&go_identifier)
        {
            return false;
        }
        if value.get_type() != *binding_ty {
            return false;
        }
        let Some(plan) = self.plan_call(value) else {
            return false;
        };
        let CalleePlan::GoInterop(strategy) = plan.callee else {
            return false;
        };
        if matches!(strategy, GoCallStrategy::Tuple { .. }) {
            return false;
        }
        let target = WrapperTarget::Slot(&go_identifier);
        let outcome =
            self.emit_go_wrapped_call_to(output, value, &strategy, binding_ty, target, fx);
        if outcome.is_none() {
            return false;
        }
        // `open_wrapper_slot` / `emit_simple_wrapper_value` already declared
        // `go_identifier`; only the binding from the user-name still needs setup.
        self.scope.bind(identifier, go_identifier.as_ref());
        true
    }

    fn emit_let_temp(
        &mut self,
        output: &mut String,
        name: &str,
        value: &Expression,
        binding_ty: &Type,
        fx: &mut EmitEffects,
    ) {
        if !self.is_declared(name) {
            self.emit_let_temp_var_declaration(output, name, value, binding_ty, fx);
            self.try_declare(name);
        }
        self.emit_assign(output, value, name, Some(binding_ty), fx);
    }

    fn emit_let_temp_var_declaration(
        &mut self,
        output: &mut String,
        name: &str,
        value: &Expression,
        binding_ty: &Type,
        fx: &mut EmitEffects,
    ) {
        if name == "_" {
            return;
        }
        let return_ctx = self.return_ctx();
        let resolved_ty = resolve_let_temp_declaration_ty(self, value, binding_ty);
        let has_variable_ok_ty = matches!(
            value,
            Expression::TryBlock { .. } | Expression::RecoverBlock { .. }
        ) && !resolved_ty.is_variable()
            && resolved_ty.ok_type().is_variable();

        let var_ty = if has_variable_ok_ty {
            if !binding_ty.is_variable() && !binding_ty.ok_type().is_variable() {
                self.go_type_string(binding_ty, fx)
            } else if let Some(ctx_ty) = return_ctx.ty().cloned() {
                if Fallible::from_type(&ctx_ty).is_some() {
                    self.go_type_string(&ctx_ty, fx)
                } else {
                    self.go_type_string(&resolved_ty, fx)
                }
            } else {
                self.go_type_string(&resolved_ty, fx)
            }
        } else {
            self.go_type_string(&resolved_ty, fx)
        };
        write_line!(output, "var {} {}", name, var_ty);
    }
}

enum LetKind {
    SimpleIdentifier,
    Discard,
    ComplexPattern,
    MultiValueCall,
    LetElse,
}

pub(crate) struct LetPlanner<'a, 'e> {
    planner: &'a mut Planner<'e>,
    binding: &'a Binding,
    value: &'a Expression,
    else_block: Option<&'a Expression>,
    mutable: bool,
}

impl<'a, 'e> LetPlanner<'a, 'e> {
    pub(crate) fn new(
        planner: &'a mut Planner<'e>,
        binding: &'a Binding,
        value: &'a Expression,
        else_block: Option<&'a Expression>,
        mutable: bool,
    ) -> Self {
        Self {
            planner,
            binding,
            value,
            else_block,
            mutable,
        }
    }

    /// Classify the binding and build the matching `LetForm`.
    pub(crate) fn build_form(mut self, fx: &mut EmitEffects) -> LetForm {
        let wrap = |buffer: String| LoweredBlock {
            statements: vec![LoweredStatement::RawGo(buffer)],
        };

        // Never-typed values diverge (break/continue/return). Declare the
        // binding so dead code can reference it, then emit the value as a
        // statement.
        if self.value.get_type().is_never() {
            let declaration = if let Pattern::Identifier { identifier, .. } = &self.binding.pattern
                && let Some(raw_go_name) = self.planner.go_name_for_binding(&self.binding.pattern)
            {
                let go_identifier = self.planner.scope.bind(identifier, &raw_go_name);
                self.planner.try_declare(&go_identifier);
                let var_ty = self.planner.go_type_string(&self.binding.ty, fx);
                Some(format!("var {} {}\n", go_identifier, var_ty))
            } else {
                None
            };
            let mut buffer = String::new();
            self.planner.emit_statement(&mut buffer, self.value, fx);
            return LetForm::Never {
                declaration,
                body: wrap(buffer),
            };
        }

        match self.classify() {
            LetKind::LetElse => {
                let else_block = self
                    .else_block
                    .expect("LetKind::LetElse classified without else block");
                let statements = self.planner.lower_let_else_pattern_site(
                    AnnotatedPattern {
                        pattern: &self.binding.pattern,
                        typed: self.binding.typed_pattern.as_ref(),
                    },
                    &self.binding.ty,
                    self.value,
                    else_block,
                    fx,
                );
                LetForm::LetElse {
                    body: LoweredBlock { statements },
                }
            }
            LetKind::SimpleIdentifier => LetForm::SimpleIdentifier {
                body: self.lower_simple_identifier(fx),
            },
            LetKind::Discard => LetForm::Discard {
                body: self.lower_discard(fx),
            },
            LetKind::MultiValueCall => LetForm::MultiValueCall {
                body: self.lower_multi_value_call(fx),
            },
            LetKind::ComplexPattern => {
                let value_ty = self.value.get_type();
                let statements = self.planner.lower_irrefutable_pattern_site(
                    PatternSubject::expression(self.value, &self.binding.pattern, None),
                    &self.binding.pattern,
                    self.binding.typed_pattern.as_ref(),
                    &value_ty,
                    fx,
                );
                LetForm::ComplexPattern {
                    body: LoweredBlock { statements },
                }
            }
        }
    }

    fn classify(&self) -> LetKind {
        if self.else_block.is_some() {
            return LetKind::LetElse;
        }

        match &self.binding.pattern {
            Pattern::Identifier { .. } => LetKind::SimpleIdentifier,
            Pattern::WildCard { .. } => LetKind::Discard,
            Pattern::Tuple { elements, .. } => {
                let all_unused = elements.iter().all(|el| match el {
                    Pattern::WildCard { .. } => true,
                    Pattern::Identifier { .. } => self.planner.facts.is_unused_binding(el),
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

    /// `let (a, b) = go_func()` is a direct Go destructure when the pattern
    /// is simple, the call returns multiple values, and the result is not
    /// `Result` (which would need wrapping).
    fn can_use_multi_value_optimization(&self) -> bool {
        let Pattern::Tuple { .. } = &self.binding.pattern else {
            return false;
        };

        let has_multi_return_go_strategy = matches!(
            self.planner.plan_call(self.value),
            Some(plan) if matches!(
                &plan.callee,
                CalleePlan::GoInterop(strategy) if strategy.is_multi_return()
            )
        );
        has_multi_return_go_strategy
            && !self.value.get_type().is_result()
            && extract_simple_tuple_vars(&self.binding.pattern).is_some()
    }

    fn lower_simple_identifier(&mut self, fx: &mut EmitEffects) -> LoweredBlock {
        let Pattern::Identifier { identifier, .. } = &self.binding.pattern else {
            unreachable!("lower_simple_identifier called with non-identifier pattern");
        };
        let raw_go_name = self.planner.go_name_for_binding(&self.binding.pattern);
        if matches!(self.value, Expression::Propagate { .. }) {
            let statements = self.planner.lower_let_propagate(
                identifier,
                raw_go_name.as_deref(),
                self.value,
                &self.binding.ty,
                fx,
            );
            return LoweredBlock { statements };
        }
        let mut buffer = String::new();
        self.planner.emit_let_value(
            &mut buffer,
            LetSpec {
                identifier,
                value: self.value,
                binding_ty: &self.binding.ty,
                mutable: self.mutable,
            },
            raw_go_name.as_deref(),
            fx,
        );
        LoweredBlock {
            statements: vec![LoweredStatement::RawGo(buffer)],
        }
    }

    fn lower_discard(&mut self, fx: &mut EmitEffects) -> LoweredBlock {
        if let Expression::Propagate {
            expression: inner, ..
        } = self.value.unwrap_parens()
        {
            let statements = self.planner.lower_propagate_statement(inner, fx);
            return LoweredBlock { statements };
        }
        let mut buffer = String::new();
        self.planner.emit_discard(&mut buffer, self.value, fx);
        LoweredBlock {
            statements: vec![LoweredStatement::RawGo(buffer)],
        }
    }

    fn lower_multi_value_call(&mut self, fx: &mut EmitEffects) -> LoweredBlock {
        let Pattern::Tuple { elements, .. } = &self.binding.pattern else {
            unreachable!("lower_multi_value_call called with non-tuple pattern");
        };

        let vars = extract_simple_tuple_vars(&self.binding.pattern)
            .expect("multi-value optimization requires simple tuple vars");

        let mut any_new = false;
        let mut planned: Vec<Option<(&str, String)>> = Vec::new();
        let go_vars: Vec<String> = vars
            .iter()
            .zip(elements.iter())
            .map(|(var, pattern)| {
                if var == "_" {
                    planned.push(None);
                    "_".to_string()
                } else if let Pattern::Identifier { identifier, .. } = pattern
                    && let Some(go_name) = self.planner.go_name_for_binding(pattern)
                {
                    let escaped = escape_reserved(&go_name).into_owned();
                    let name = if self.planner.is_declared(&escaped) {
                        let fresh = self.planner.fresh_var(Some(identifier));
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

        let mut setup = String::new();
        let call_str =
            self.planner
                .emit_call(&mut setup, self.value, None, ExpressionContext::value(), fx);

        for (identifier, go_name) in planned.iter().flatten() {
            self.planner.scope.bind(*identifier, go_name);
            self.planner.try_declare(go_name);
        }

        let op = if any_new { ":=" } else { "=" };
        let mut statements = Vec::new();
        if !setup.is_empty() {
            statements.push(LoweredStatement::RawGo(setup));
        }
        statements.push(LoweredStatement::RawGo(format!(
            "{} {} {}\n",
            go_vars.join(", "),
            op,
            call_str
        )));
        LoweredBlock { statements }
    }
}

/// Variable names from a simple tuple pattern (identifiers or wildcards);
/// `None` when any element is composite.
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

impl Planner<'_> {
    /// Build a `LetPlan` by classifying the binding into a `LetForm`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_let_plan(
        &mut self,
        binding: &Binding,
        value: &Expression,
        else_block: Option<&Expression>,
        mutable: bool,
        directive: String,
        fx: &mut EmitEffects,
    ) -> LetPlan {
        let form = LetPlanner::new(self, binding, value, else_block, mutable).build_form(fx);
        LetPlan { directive, form }
    }
}
