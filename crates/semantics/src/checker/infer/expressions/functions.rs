use crate::checker::EnvResolve;
use ecow::EcoString;
use syntax::ast::BindingKind;
use syntax::ast::{
    Annotation, Binding, Expression, Literal, Pattern, Span, StructKind, UnaryOperator,
};
use syntax::program::{CallKind, Definition, DefinitionBody, NativeTypeKind};
use syntax::types::{
    Bound, CompoundKind, SubstitutionMap, Symbol, Type, peel_to_range_type, substitute,
    unqualified_name,
};

use super::super::carry_mut::can_carry_mutation_across_fn_boundary;
use super::super::unify::Dispatched;
use super::primitives::contains_deref;
use crate::checker::infer::InferCtx;
use crate::checker::registration::test_functions::normalize_test_params;
use crate::store::ENTRY_MODULE_ID;

impl InferCtx<'_, '_> {
    pub(crate) fn check_call_arity(
        &mut self,
        param_types: &[Type],
        args: &[Expression],
        callee_expression: &Expression,
        span: &Span,
    ) {
        if param_types.len() == args.len() {
            return;
        }
        let expected: Vec<Type> = param_types
            .iter()
            .map(|t| t.resolve_in(&self.env))
            .collect();
        let actual: Vec<Type> = args
            .iter()
            .map(|e| e.get_type().resolve_in(&self.env))
            .collect();
        let generic_params = self.get_generic_param_names(callee_expression);
        let is_constructor = callee_expression
            .get_var_name()
            .map(|name| name.chars().next().is_some_and(|c| c.is_uppercase()))
            .unwrap_or(false);
        self.sink.push(diagnostics::infer::arity_mismatch(
            &expected,
            &actual,
            &generic_params,
            is_constructor,
            *span,
        ));
    }

    fn get_generic_param_names(&self, expression: &Expression) -> Vec<String> {
        if let Expression::Identifier { value, .. } = expression
            && let Some(ty) = self.scopes.lookup_value(value)
        {
            return match ty {
                Type::Forall { vars, .. } => vars.iter().map(|s| s.to_string()).collect(),
                _ => vec![],
            };
        }
        vec![]
    }
}

impl InferCtx<'_, '_> {
    pub(super) fn ty_is_test_context(&self, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env).strip_refs();
        resolved.get_qualified_id().is_some_and(|id| {
            id.strip_suffix(".TestContext")
                .is_some_and(|module| module == crate::prelude::TEST_PRELUDE_MODULE_ID)
        })
    }

    fn param_provides_test_handle(&self, param: &Binding) -> bool {
        matches!(&param.pattern, Pattern::Identifier { identifier, .. } if identifier != "_")
            && self.ty_is_test_context(&param.ty)
    }

    fn mark_test_context_params_used(&mut self, params: &[Binding]) {
        for param in params {
            if let Pattern::Identifier { identifier, .. } = &param.pattern
                && self.param_provides_test_handle(param)
                && let Some(id) = self.scopes.lookup_binding_id(identifier)
            {
                self.facts.mark_used(id);
            }
        }
    }

    pub(super) fn infer_function(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        let Expression::Function {
            doc,
            attributes,
            name,
            name_span,
            generics,
            params,
            return_annotation,
            visibility,
            body,
            span,
            ..
        } = expression
        else {
            unreachable!("infer_function called with non-Function expression");
        };

        if self.scopes.lookup_fn_return_type().is_some() {
            self.sink
                .push(diagnostics::infer::nested_function(name_span));
        }

        if name == "main"
            && self.cursor.module_id == ENTRY_MODULE_ID
            && (!params.is_empty() || return_annotation != Annotation::Unknown)
        {
            self.sink
                .push(diagnostics::infer::invalid_main_signature(name_span));
        }

        self.scopes.push();

        self.put_in_scope(&generics);

        let mut bounds = vec![];

        for generic in &generics {
            for bound in &generic.bounds {
                let bound_ty = self.register_generic_bound(store, &generic.name, bound, &span);

                bounds.push(Bound {
                    param_name: generic.name.clone(),
                    generic: Type::Parameter(generic.name.clone()),
                    ty: bound_ty,
                });
            }
        }

        let resolved_expected = expected_ty.resolve_in(&self.env);
        let expected_params = resolved_expected.get_function_params().unwrap_or_default();
        let is_test = attributes.iter().any(|a| a.name == "test");
        let params = normalize_test_params(params, is_test);
        let new_params = self.infer_function_params(params, expected_params, true);

        if is_test
            || new_params
                .iter()
                .any(|p| self.param_provides_test_handle(p))
        {
            self.scopes.mark_test_handle();
        }
        if is_test {
            self.scopes.set_test_fn_name(name.clone());
        }
        self.mark_test_context_params_used(&new_params);

        let unit_ty = self.type_unit();
        let return_ty =
            self.infer_return_type(&return_annotation, &resolved_expected, &span, unit_ty);

        self.scopes.current_mut().fn_return_type = Some(return_ty.clone());

        let base_fn_ty = Type::function_with_names(
            new_params.iter().map(|p| p.ty.clone()).collect(),
            new_params
                .iter()
                .map(|p| p.pattern.get_identifier())
                .collect(),
            new_params.iter().map(|p| p.mutable).collect(),
            bounds,
            return_ty.clone().into(),
        );

        // `Type::ignored()` defers the tail-position check to
        // `passes/fact_producers/unused_expressions.rs`, which honors `#[allow(unused_*)]`.
        let has_implicit_unit_return = return_annotation == Annotation::Unknown;
        let body_ty = if has_implicit_unit_return {
            Type::ignored()
        } else {
            return_ty.clone()
        };

        let new_body = self.infer_function_body(body, &body_ty, &return_annotation, &return_ty);

        self.check_deferred_map_key_bounds(store);

        self.scopes.pop();

        let fn_ty = if generics.is_empty() {
            base_fn_ty
        } else {
            let fn_forall_ty = Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(base_fn_ty),
            };
            self.instantiate(&fn_forall_ty).0
        };

        self.unify(expected_ty, &fn_ty, &span);

        self.facts.add_function_span(span);

        Expression::Function {
            doc,
            attributes,
            name,
            name_span,
            generics,
            params: new_params,
            return_annotation,
            return_type: return_ty,
            visibility,
            body: new_body.into(),
            ty: fn_ty,
            span,
        }
    }

    pub(super) fn infer_lambda(
        &mut self,
        params: Vec<Binding>,
        return_annotation: Annotation,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        self.scopes.push();

        // Resolve type variables so that a Go function alias bound via speculative
        // unification (e.g. T = tea.Cmd) is visible as its underlying function shape.
        let resolved_expected = expected_ty.resolve_in(&self.env);
        let expected_params = resolved_expected.get_function_params().unwrap_or_default();
        let new_params = self.infer_function_params(params, expected_params, false);

        if new_params
            .iter()
            .any(|p| self.param_provides_test_handle(p))
        {
            self.scopes.mark_test_handle();
        }
        self.mark_test_context_params_used(&new_params);

        let default_return = self.new_type_var();
        let return_ty = self.infer_return_type(
            &return_annotation,
            &resolved_expected,
            &span,
            default_return,
        );

        self.scopes.current_mut().fn_return_type = Some(return_ty.clone());

        let base_fn_ty = Type::function_with_names(
            new_params.iter().map(|p| p.ty.clone()).collect(),
            new_params
                .iter()
                .map(|p| p.pattern.get_identifier())
                .collect(),
            new_params.iter().map(|p| p.mutable).collect(),
            vec![],
            return_ty.clone().into(),
        );

        // Reset loop depth — closures introduce a new function scope, so
        // `defer` inside a closure body should not be flagged as "defer in loop"
        // even when the closure is lexically inside a loop.
        let saved_loop_depth = self.scopes.reset_loop_depth();
        // `Type::ignored()` defers the tail-position check to
        // `passes/fact_producers/unused_expressions.rs`, which honors `#[allow(unused_*)]`.
        let relax_body_to_unit = return_annotation == Annotation::Unknown && return_ty.is_unit();
        let body_ty = if relax_body_to_unit {
            Type::ignored()
        } else {
            return_ty.clone()
        };
        let new_body = self.infer_function_body(body, &body_ty, &return_annotation, &return_ty);
        self.scopes.restore_loop_depth(saved_loop_depth);

        self.check_deferred_map_key_bounds(store);

        self.scopes.pop();

        self.unify(expected_ty, &base_fn_ty, &span);

        Expression::Lambda {
            params: new_params,
            return_annotation,
            body: new_body.into(),
            ty: base_fn_ty,
            span,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_function_call(
        &mut self,
        expression: Box<Expression>,
        args: Vec<Expression>,
        spread: Box<Option<Expression>>,
        type_args: Vec<Annotation>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let callee_path = expression.unwrap_parens().as_dotted_path();

        // `Array.new` has no prelude signature (no const generics), so resolve inline.
        if callee_path.as_deref() == Some("Array.new") {
            return self.infer_array_new_call(&expression, args, type_args, span, expected_ty);
        }

        if let Some(diagnostic) = match callee_path.as_deref() {
            Some("Map.make") => Some(diagnostics::infer::map_no_make_constructor(span)),
            Some("Channel.make") => Some(diagnostics::infer::channel_no_make_constructor(span)),
            _ => None,
        } {
            self.sink.push(diagnostic);
            let new_args: Vec<Expression> = args
                .into_iter()
                .map(|arg| self.with_value_context(|s| s.infer_expression(arg, &Type::Error)))
                .collect();
            let new_spread = spread
                .map(|s| self.with_value_context(|state| state.infer_expression(s, &Type::Error)));
            return Expression::Call {
                expression,
                args: new_args,
                spread: Box::new(new_spread),
                raw_type_args: type_args,
                resolved_type_args: Vec::new(),
                ty: Type::Error,
                span,
                call_kind: None,
            };
        }

        let store = self.store;
        let callee_ty = self.new_type_var();

        let prev_context = self.scopes.set_callee_context();
        let callee_expression = self.infer_expression(*expression, &callee_ty);
        self.scopes.restore_use_context(prev_context);

        let forall_ty = self.resolve_callee_forall_type(&callee_expression, &type_args);
        let (callee_ty, raw_type_args, resolved_type_args) =
            self.instantiate_callee_type(&forall_ty, &type_args, &callee_expression, &span);

        if let Some(underlying_fn) = self.try_as_type_conversion(&callee_expression, &callee_ty) {
            return self.infer_type_conversion_call(
                callee_expression,
                callee_ty,
                underlying_fn,
                args,
                spread,
                raw_type_args,
                resolved_type_args,
                span,
                expected_ty,
            );
        }

        let needs_variadic_check = spread.is_some()
            || matches!(
                args.last(),
                Some(Expression::Range {
                    start: None,
                    end: Some(_),
                    inclusive: false,
                    ..
                })
            );

        let resolved_callee = callee_ty.resolve_in(&self.env);
        let variadic_elem_var = resolved_callee.is_variadic();
        let callee_param_count = resolved_callee.get_function_params().map_or(0, |p| p.len());
        let variadic_elem_ty = if needs_variadic_check {
            variadic_elem_var.clone()
        } else {
            None
        };

        let (param_types, param_mutability, return_ty, bounds) =
            self.extract_call_signature(callee_ty, &args, &callee_expression);

        if self.is_panic_call(&callee_expression)
            && self.scopes.is_value_context()
            && !expected_ty.is_unit()
            && !expected_ty.is_ignored()
            && !expected_ty.is_never()
            && !expected_ty.is_variable()
        {
            self.sink
                .push(diagnostics::infer::panic_in_expression_position(span));
        }

        if self.is_generic_callee(&callee_expression) && !expected_ty.is_ignored() {
            let resolved_expected = expected_ty.resolve_in(&self.env);
            if !resolved_expected.is_variable()
                && (self.is_enum_type(store, &return_ty.resolve_in(&self.env))
                    || !resolved_expected.contains_unknown())
            {
                let peeled = store.deep_resolve_alias(&resolved_expected);
                let _ = self.speculatively(|this| {
                    InferCtx::new(this, store).try_unify(&peeled, &return_ty, &span)
                });
            }
        }

        let call_kind = self.classify_call(&callee_expression);

        let substring_range_idx =
            self.substring_carve_out_param_idx(call_kind, &callee_expression, &param_types);
        let new_args = if let Some(idx) = substring_range_idx {
            let mut adjusted = param_types.clone();
            adjusted[idx] = self.new_type_var();
            self.infer_call_arguments(args, &adjusted)
        } else {
            self.infer_call_arguments(args, &param_types)
        };
        self.check_call_arity(&param_types, &new_args, &callee_expression, &span);
        self.check_mut_param_arguments(
            &new_args,
            &param_types,
            &param_mutability,
            &callee_expression,
        );

        self.check_range_to_for_variadic(&new_args, &variadic_elem_ty);

        if let Some(idx) = substring_range_idx
            && let Some(arg) = new_args.get(idx)
        {
            self.validate_substring_range_arg(arg);
        }

        let callee_is_unresolved = callee_expression
            .get_type()
            .resolve_in(&self.env)
            .is_error();

        let new_spread = (*spread).map(|spread_expr| match variadic_elem_ty {
            Some(elem_ty) => {
                let expected = if elem_ty.is_unknown() {
                    let var = self.new_type_var();
                    self.type_slice(var)
                } else {
                    self.type_slice(elem_ty.clone())
                };
                let inferred =
                    self.with_value_context(|s| s.infer_expression(spread_expr, &expected));
                if param_mutability.last().copied().unwrap_or(false) {
                    let callee_label = callee_label(&callee_expression);
                    self.check_arg_against_mut_param(&inferred, &elem_ty, &callee_label);
                }
                inferred
            }
            None => {
                if !callee_is_unresolved {
                    self.sink
                        .push(diagnostics::infer::spread_on_non_variadic(span));
                }
                self.with_value_context(|s| s.infer_expression(spread_expr, &Type::Error))
            }
        });

        // Bridge multi-hop aliases by re-resolving the expected type through
        // the store before the final unify (forward-declared intermediates
        // leave gaps in the cached `underlying_ty` chain).
        let resolved_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        let expected_is_map = matches!(
            resolved_expected.as_compound(),
            Some((CompoundKind::Map, _))
        );
        self.unify(&resolved_expected, &return_ty, &span);
        self.unify_trait_bounds(&bounds, &param_types, &new_args, &span);

        let resolved_return = store.deep_resolve_alias(&return_ty.resolve_in(&self.env));
        if let Some((CompoundKind::Map, arguments)) = resolved_return.as_compound()
            && let Some(key) = arguments.first()
        {
            let check_concrete = matches!(
                call_kind,
                CallKind::NativeConstructor(NativeTypeKind::Map)
                    | CallKind::NativeMethod(NativeTypeKind::Map)
                    | CallKind::NativeMethodIdentifier(NativeTypeKind::Map)
            ) && !expected_is_map;
            self.scopes
                .defer_map_key_check(key.clone(), span, check_concrete);
        }

        self.check_native_mutating_call(&callee_expression, &span);
        self.check_native_equals_ufcs(&callee_expression, &new_args);

        let return_check_recorded = self.is_generic_callee(&callee_expression)
            && type_args.is_empty()
            && !self.is_enum_type(store, &resolved_return);
        if return_check_recorded {
            let module_id = self.cursor.module_id.clone();
            self.facts
                .generic_call_checks
                .push(crate::facts::GenericCallCheck {
                    ty: return_ty.clone(),
                    span,
                    module_id,
                });
        }

        // A zero-variadic-arg call can't infer its `VarArgs<T>` parameter from args.
        // Record the element type; the deferred pass rejects it only if it stays
        // unbound. Skip when the return-type check above already records it.
        if type_args.is_empty()
            && new_spread.is_none()
            && let Some(elem_ty) = &variadic_elem_var
            && new_args.len() < callee_param_count
        {
            let already_covered = return_check_recorded
                && resolved_return.contains_type(&elem_ty.resolve_in(&self.env));
            if !already_covered {
                let module_id = self.cursor.module_id.clone();
                self.facts
                    .generic_call_checks
                    .push(crate::facts::GenericCallCheck {
                        ty: elem_ty.clone(),
                        span,
                        module_id,
                    });
            }
        }

        if type_args.is_empty() && self.callee_has_phantom_type_param(&callee_expression) {
            self.sink
                .push(diagnostics::infer::cannot_infer_type_argument(span));
        }

        // Use expected_ty for generic containers (Option, Result) when it has
        // interface type parameters. This ensures coercion like `Option<Printable>`
        // from `Some(Text{...})` gets the correct type for codegen.
        let call_ty = if !expected_ty.is_variable()
            && self.is_generic_container_with_interface(store, expected_ty)
        {
            expected_ty.clone()
        } else {
            return_ty.clone()
        };

        if call_kind == CallKind::AssertType {
            self.check_redundant_assert_type(&return_ty, &new_args, span);
        }

        if callee_path.as_deref() == Some("Slice.make") {
            let module_id = self.cursor.module_id.clone();
            self.facts
                .slice_make_checks
                .push(crate::facts::SliceMakeCheck {
                    ty: call_ty.clone(),
                    span,
                    module_id,
                });
        }

        self.check_negative_size_literal(
            call_kind,
            &callee_expression,
            callee_path.as_deref(),
            &new_args,
        );

        Expression::Call {
            expression: callee_expression.into(),
            args: new_args,
            spread: Box::new(new_spread),
            raw_type_args,
            resolved_type_args,
            ty: call_ty,
            span,
            call_kind: Some(call_kind),
        }
    }

    fn check_negative_size_literal(
        &mut self,
        call_kind: CallKind,
        callee_expression: &Expression,
        callee_path: Option<&str>,
        args: &[Expression],
    ) {
        let sized = match call_kind {
            CallKind::NativeConstructor(NativeTypeKind::Slice)
                if callee_path == Some("Slice.make") =>
            {
                args.first().map(|arg| ("length", arg))
            }
            CallKind::NativeConstructor(NativeTypeKind::Channel)
                if callee_path == Some("Channel.buffered") =>
            {
                args.first().map(|arg| ("capacity", arg))
            }
            CallKind::NativeMethod(NativeTypeKind::Slice) => {
                match callee_expression.unwrap_parens() {
                    Expression::DotAccess { member, .. } if member == "reserve" => {
                        args.first().map(|arg| ("capacity", arg))
                    }
                    _ => None,
                }
            }
            CallKind::NativeMethodIdentifier(NativeTypeKind::Slice)
                if callee_path == Some("Slice.reserve") =>
            {
                args.get(1).map(|arg| ("capacity", arg))
            }
            _ => None,
        };
        if let Some((what, arg)) = sized
            && is_negative_integer_literal(arg)
        {
            self.sink.push(diagnostics::infer::negative_size_literal(
                what,
                arg.get_span(),
            ));
        }
    }

    /// Infer `Array.new<T, N>()`: the zero value of a fixed-size array.
    fn infer_array_new_call(
        &mut self,
        callee: &Expression,
        args: Vec<Expression>,
        type_args: Vec<Annotation>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;

        // Resolve (element, length) from the turbofish, else the expected type.
        let resolved = if type_args.is_empty() {
            let peeled = store.peel_alias(&expected_ty.resolve_in(&self.env));
            match peeled {
                Type::Array { length, element } => Some((element.as_ref().clone(), length)),
                _ => {
                    self.sink
                        .push(diagnostics::infer::array_new_cannot_infer_size(span));
                    None
                }
            }
        } else if type_args.len() == 2 {
            let elem = self.convert_to_type(store, &type_args[0], &span);
            match &type_args[1] {
                Annotation::Constant {
                    value,
                    span: size_span,
                    ..
                } => self
                    .check_array_size_in_bounds(*value, *size_span)
                    .then_some((elem, *value)),
                other => {
                    self.sink
                        .push(diagnostics::infer::array_size_not_literal(other.get_span()));
                    None
                }
            }
        } else {
            self.sink
                .push(diagnostics::infer::array_type_arity(type_args.len(), span));
            None
        };

        // `Array.new` takes no value arguments, but still infer any for recovery.
        if !args.is_empty() {
            self.sink
                .push(diagnostics::infer::array_new_takes_no_arguments(
                    args.len(),
                    span,
                ));
        }
        let new_args: Vec<Expression> = args
            .into_iter()
            .map(|arg| {
                let var = self.new_type_var();
                self.with_value_context(|s| s.infer_expression(arg, &var))
            })
            .collect();

        let array_ty = match resolved {
            Some((elem, len)) => {
                let array_ty = self.type_array(len, elem);
                let from_module = self.cursor.module_id.clone();
                if let Err(no_zero) = self.has_zero(&array_ty, &from_module) {
                    self.sink.push(diagnostics::infer::array_new_no_zero(
                        &no_zero.leaf_ty.stringify(),
                        span,
                    ));
                }
                array_ty
            }
            None => Type::Error,
        };

        self.unify(expected_ty, &array_ty, &span);

        let callee_ty = Type::function(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Box::new(array_ty.clone()),
        );
        let callee_expression = Expression::Identifier {
            value: "Array.new".into(),
            ty: callee_ty,
            span: callee.get_span(),
            binding_id: None,
            qualified: None,
        };

        Expression::Call {
            expression: callee_expression.into(),
            args: new_args,
            spread: Box::new(None),
            raw_type_args: type_args,
            resolved_type_args: Vec::new(),
            ty: array_ty,
            span,
            call_kind: Some(CallKind::NativeConstructor(NativeTypeKind::Array)),
        }
    }

    fn resolve_callee_forall_type(
        &mut self,
        expression: &Expression,
        type_args: &[Annotation],
    ) -> Type {
        if type_args.is_empty() {
            return expression.get_type();
        }
        self.declared_callee_type(expression)
    }

    fn declared_callee_type(&self, expression: &Expression) -> Type {
        let store = self.store;
        match expression {
            Expression::Identifier { value, .. } => self
                .lookup_type(store, value)
                .unwrap_or_else(|| expression.get_type()),
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } => {
                let receiver_ty = receiver.get_type().resolve_in(&self.env);

                if let Some(method_ty) = self
                    .get_all_methods(store, &receiver_ty.strip_refs())
                    .get(member)
                    .cloned()
                {
                    return method_ty;
                }

                let stripped = receiver_ty.strip_refs();
                if let Type::Nominal { id, .. } = &stripped {
                    let qualified = id.with_segment(member);
                    if let Some(definition) = store.get_definition(&qualified) {
                        return definition.ty().clone();
                    }
                }

                if let Some(module_id) = stripped.as_import_namespace() {
                    let qualified = Symbol::from_parts(module_id, member);
                    if let Some(definition) = store.get_definition(&qualified) {
                        return definition.ty().clone();
                    }
                }

                expression.get_type()
            }
            _ => expression.get_type(),
        }
    }

    fn is_generic_callee(&self, expression: &Expression) -> bool {
        matches!(self.declared_callee_type(expression), Type::Forall { .. })
    }

    fn callee_has_phantom_type_param(&self, expression: &Expression) -> bool {
        let Type::Forall { vars, body } = self.declared_callee_type(expression) else {
            return false;
        };
        let Type::Function(f) = body.as_ref() else {
            return false;
        };
        vars.iter().any(|var| {
            let param = Type::Parameter(var.clone());
            let in_signature = f.params.iter().any(|pt| pt.contains_type(&param))
                || f.return_type.contains_type(&param);
            let is_bounded = f.bounds.iter().any(|bound| bound.param_name == *var);
            !in_signature && !is_bounded
        })
    }

    fn instantiate_callee_type(
        &mut self,
        forall_ty: &Type,
        type_args: &[Annotation],
        callee_expression: &Expression,
        span: &Span,
    ) -> (Type, Vec<Annotation>, Vec<Type>) {
        let store = self.store;
        let Type::Forall { vars, body } = forall_ty else {
            if !type_args.is_empty() {
                self.sink.push(diagnostics::infer::type_args_on_non_generic(
                    type_args.len(),
                    *span,
                ));
            }
            let (instantiated, _) = self.instantiate(forall_ty);
            return (instantiated.resolve_in(&self.env), vec![], vec![]);
        };

        if type_args.is_empty() {
            let (instantiated, _) = self.instantiate(forall_ty);
            return (instantiated.resolve_in(&self.env), vec![], vec![]);
        }

        let declared_param_count = match body.as_ref() {
            Type::Function(f) => f.params.len(),
            _ => 0,
        };
        let is_receiver_method = matches!(callee_expression, Expression::DotAccess { .. })
            && declared_param_count > callee_expression.get_type().param_count();
        let receiver_generics_count = if is_receiver_method {
            receiver_inferred_prefix_count(body, vars)
        } else {
            0
        };

        let method_only_count = vars.len().saturating_sub(receiver_generics_count);
        let is_full_arity = type_args.len() == vars.len();
        let is_method_only_arity =
            receiver_generics_count > 0 && type_args.len() == method_only_count;

        let mut resolved_args: Vec<Type> = Vec::new();
        let mut instantiated = if is_method_only_arity {
            let mut map: SubstitutionMap = SubstitutionMap::default();
            for var in &vars[..receiver_generics_count] {
                map.insert(var.clone(), self.new_type_var());
            }
            for (var, ann) in vars[receiver_generics_count..].iter().zip(type_args.iter()) {
                let arg_ty = self.convert_to_type(store, ann, span);
                resolved_args.push(arg_ty.clone());
                map.insert(var.clone(), arg_ty);
            }
            substitute(body, &map)
        } else {
            let (instantiated, args) =
                self.instantiate_from_annotations(store, vars, body, type_args, span);
            resolved_args = args;
            instantiated
        };

        if !is_full_arity && !is_method_only_arity {
            let vars_as_str: Vec<String> = vars.iter().map(|s| s.to_string()).collect();
            self.sink.push(diagnostics::infer::generics_arity_mismatch(
                &vars_as_str,
                type_args,
                &resolved_args,
                *span,
            ));
        }

        if let Expression::DotAccess { expression, .. } = callee_expression {
            let receiver_ty = expression.get_type().resolve_in(&self.env);

            // Only strip the receiver param for instance methods (which have `self`).
            // Instance methods: `as_instance_method` already stripped `self` from
            // the callee type, so the Forall body has one more param than the callee.
            // Static methods and module free functions: no `self`, param counts match.
            let callee_params = callee_expression
                .get_type()
                .resolve_in(&self.env)
                .param_count();
            let instantiated_params = instantiated.param_count();
            let has_receiver = instantiated_params > callee_params;

            if has_receiver
                && let Type::Function(ref mut f) = instantiated
                && !f.params.is_empty()
            {
                let f = std::sync::Arc::make_mut(f);
                let receiver_param = f.remove_receiver();
                let receiver_ty_stripped = receiver_ty.strip_refs();
                if receiver_param.is_ref() && !receiver_ty.is_ref() {
                    if let Some(inner) = receiver_param.inner() {
                        self.unify(&inner, &receiver_ty_stripped, span);
                    }
                } else {
                    self.unify(&receiver_param, &receiver_ty_stripped, span);
                }
            }
        }

        // Write the substituted type back onto the callee node so its type (and
        // hover) reflects explicit type arguments, as an inferred call already does.
        let callee_ty = callee_expression.get_type();
        for (inferred, explicit) in callee_ty.get_bounds().iter().zip(instantiated.get_bounds()) {
            let _ = self.try_unify(&inferred.generic, &explicit.generic, span);
        }
        self.unify(&instantiated, &callee_ty, span);

        (instantiated, type_args.to_vec(), resolved_args)
    }

    fn extract_call_signature(
        &mut self,
        callee_ty: Type,
        args: &[Expression],
        callee_expression: &Expression,
    ) -> (Vec<Type>, Vec<bool>, Type, Vec<Bound>) {
        let arg_count = args.len();
        let callee_ty = callee_ty.resolve_in(&self.env);
        let bounds = callee_ty.get_bounds().to_vec();
        let mut param_mutability = callee_ty.get_param_mutability().to_vec();
        let is_variadic = callee_ty.is_variadic();

        let (param_types, return_ty) = match self.extract_function_type(&callee_ty) {
            Some((mut params, return_type)) => {
                if let Some(variadic_ty) = is_variadic {
                    params.pop();
                    while params.len() < arg_count {
                        params.push(variadic_ty.clone());
                    }
                    if let Some(&variadic_mut) = param_mutability.last() {
                        while param_mutability.len() < arg_count {
                            param_mutability.push(variadic_mut);
                        }
                    }
                }
                (params, return_type)
            }
            None if callee_ty.is_variable() => {
                let param_types = (0..arg_count).map(|_| self.new_type_var()).collect();
                let return_ty = self.new_type_var();
                (param_types, return_ty)
            }
            None if callee_ty.resolve_in(&self.env).is_error() => {
                let param_types = (0..arg_count).map(|_| Type::Error).collect();
                let return_ty = Type::Error;
                (param_types, return_ty)
            }
            None => {
                let callee_name = match callee_expression.unwrap_parens() {
                    Expression::Identifier {
                        value,
                        binding_id: None,
                        ..
                    } => Some(value.as_str()),
                    _ => None,
                };
                let arg_name = if args.len() == 1 {
                    match args[0].unwrap_parens() {
                        Expression::Identifier { value, .. } => Some(value.as_str()),
                        _ => None,
                    }
                } else {
                    None
                };
                self.sink.push(diagnostics::infer::not_callable(
                    &callee_ty,
                    callee_name,
                    arg_name,
                    callee_expression.get_span(),
                ));
                let param_types = (0..arg_count).map(|_| Type::Error).collect();
                let return_ty = Type::Error;
                (param_types, return_ty)
            }
        };

        (param_types, param_mutability, return_ty, bounds)
    }

    fn extract_function_type(&self, ty: &Type) -> Option<(Vec<Type>, Type)> {
        let store = self.store;
        let fn_type = |ty: &Type| -> Option<(Vec<Type>, Type)> {
            if let Type::Function(f) = ty {
                Some((f.params.clone(), (*f.return_type).clone()))
            } else {
                None
            }
        };

        if let result @ Some(_) = fn_type(ty) {
            return result;
        }

        if let Type::Nominal {
            underlying_ty: Some(underlying),
            ..
        } = ty
            && let result @ Some(_) = fn_type(underlying)
        {
            return result;
        }

        if let Type::Nominal { id, params, .. } = ty
            && let Some(def) = store.get_definition(id)
            && matches!(def.body, DefinitionBody::TypeAlias { .. })
        {
            let alias_ty = &def.ty;
            let concrete_alias_ty = match alias_ty {
                Type::Forall { vars, body } => {
                    let map: SubstitutionMap =
                        vars.iter().cloned().zip(params.iter().cloned()).collect();
                    substitute(body, &map)
                }
                other => other.clone(),
            };
            let resolved = concrete_alias_ty.resolve_in(&self.env);
            if let Type::Nominal {
                underlying_ty: Some(underlying),
                ..
            } = &resolved
            {
                return fn_type(underlying);
            }
        }

        None
    }

    fn try_as_type_conversion(&self, callee: &Expression, callee_ty: &Type) -> Option<Type> {
        let store = self.store;
        let Type::Nominal {
            id,
            underlying_ty: Some(underlying),
            ..
        } = callee_ty
        else {
            return None;
        };

        if !matches!(underlying.as_ref(), Type::Function(_)) {
            return None;
        }

        if !matches!(
            store.get_definition(id).map(|d| &d.body),
            Some(DefinitionBody::TypeAlias { .. })
        ) {
            return None;
        }

        let is_bare_type_name = match callee.unwrap_parens() {
            Expression::Identifier { binding_id, .. } => binding_id.is_none(),
            Expression::DotAccess {
                expression: base, ..
            } => base
                .get_type()
                .resolve_in(&self.env)
                .as_import_namespace()
                .is_some(),
            _ => false,
        };

        if !is_bare_type_name {
            return None;
        }

        Some(underlying.as_ref().clone())
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_type_conversion_call(
        &mut self,
        callee_expression: Expression,
        named_ty: Type,
        underlying_fn: Type,
        args: Vec<Expression>,
        spread: Box<Option<Expression>>,
        raw_type_args: Vec<Annotation>,
        resolved_type_args: Vec<Type>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if let Some(spread_expr) = *spread {
            self.sink
                .push(diagnostics::infer::spread_on_non_variadic(span));
            self.with_value_context(|s| s.infer_expression(spread_expr, &Type::Error));
        }

        if args.len() != 1 {
            let Type::Nominal { id, .. } = &named_ty else {
                unreachable!("type_conversion_underlying only fires for Constructor callees")
            };
            self.sink.push(diagnostics::infer::type_conversion_arity(
                unqualified_name(id),
                args.len(),
                span,
            ));
            let new_args: Vec<Expression> = args
                .into_iter()
                .map(|arg| self.with_value_context(|s| s.infer_expression(arg, &Type::Error)))
                .collect();
            self.unify(expected_ty, &Type::Error, &span);
            return Expression::Call {
                expression: callee_expression.into(),
                args: new_args,
                spread: Box::new(None),
                raw_type_args,
                resolved_type_args,
                ty: Type::Error,
                span,
                call_kind: Some(CallKind::Regular),
            };
        }

        let arg = args.into_iter().next().unwrap();
        let new_arg = self.with_value_context(|s| s.infer_expression(arg, &underlying_fn));

        self.unify(expected_ty, &named_ty, &span);

        Expression::Call {
            expression: callee_expression.into(),
            args: vec![new_arg],
            spread: Box::new(None),
            raw_type_args,
            resolved_type_args,
            ty: named_ty,
            span,
            call_kind: Some(CallKind::Regular),
        }
    }

    fn infer_call_arguments(
        &mut self,
        args: Vec<Expression>,
        param_types: &[Type],
    ) -> Vec<Expression> {
        args.into_iter()
            .enumerate()
            .map(|(i, arg)| {
                let expected_ty = param_types
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| self.new_type_var());
                self.with_value_context(|s| s.infer_expression(arg, &expected_ty))
            })
            .collect()
    }

    fn check_redundant_assert_type(&mut self, return_ty: &Type, args: &[Expression], span: Span) {
        let resolved_return = return_ty.resolve_in(&self.env);
        let Some(asserted_ty) = resolved_return.inner() else {
            return;
        };
        let asserted_ty = asserted_ty.resolve_in(&self.env);

        let Some(arg) = args.first() else {
            return;
        };
        let value_ty = arg.get_type().resolve_in(&self.env);
        if value_ty.is_unknown() {
            return;
        }

        if value_ty == asserted_ty {
            self.sink.push(diagnostics::infer::redundant_assert_type(
                &asserted_ty,
                span,
            ));
        }
    }

    /// Suggests postfix `f(xs...)` when a `..xs` range arg lands against a variadic callee.
    fn check_range_to_for_variadic(
        &mut self,
        args: &[Expression],
        variadic_elem_ty: &Option<Type>,
    ) {
        if variadic_elem_ty.is_none() {
            return;
        }

        let Some(arg) = args.last() else {
            return;
        };

        let Expression::Range {
            start: None,
            end: Some(inner),
            inclusive: false,
            ..
        } = arg
        else {
            return;
        };

        let inner_ty = inner.get_type().resolve_in(&self.env);
        if !inner_ty.is_slice() {
            return;
        }

        let var_name = match inner.as_ref() {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            _ => None,
        };

        self.sink.push(diagnostics::infer::range_to_for_variadic(
            arg.get_span(),
            var_name,
        ));
    }

    fn unify_trait_bounds(
        &mut self,
        bounds: &[Bound],
        signature_params: &[Type],
        args: &[Expression],
        fallback_span: &Span,
    ) {
        let store = self.store;
        for bound in bounds {
            let resolved_ty = bound.generic.resolve_in(&self.env);

            if resolved_ty.is_variable() {
                continue;
            }

            let span = args
                .iter()
                .find(|arg| arg.get_type().resolve_in(&self.env) == resolved_ty)
                .map(|arg| arg.get_span())
                .unwrap_or_else(|| *fallback_span);

            if self.dispatch_builtin_bound(bound, &resolved_ty, &span) == Dispatched::Handled {
                continue;
            }

            let interface_ty = bound.ty.resolve_in(&self.env);
            let Type::Nominal { id, params, .. } = interface_ty else {
                continue;
            };

            let Some(interface) = store.get_interface(&id).cloned() else {
                continue;
            };

            if self
                .satisfies_interface(&resolved_ty, &interface, &id, &params, &span)
                .is_ok()
                && !self.generic_absorbed_via_ref_param(&bound.generic, signature_params)
            {
                let _ = self.check_pointer_receivers(&resolved_ty, &interface, &id, &span);
            }
        }
    }

    fn infer_function_body(
        &mut self,
        body: Box<Expression>,
        body_ty: &Type,
        return_annotation: &Annotation,
        return_ty: &Type,
    ) -> Expression {
        if let Expression::Block {
            items,
            span: body_span,
            ..
        } = body.as_ref()
            && items.is_empty()
            && *return_annotation != Annotation::Unknown
            && !return_ty.is_unit()
        {
            self.sink
                .push(diagnostics::infer::empty_body_return_mismatch(
                    return_ty,
                    return_annotation.get_span(),
                ));
            return Expression::Block {
                items: vec![],
                ty: self.type_unit(),
                span: *body_span,
            };
        }

        self.infer_expression(*body, body_ty)
    }

    fn infer_function_params(
        &mut self,
        params: Vec<Binding>,
        expected_params: &[Type],
        handle_self_receiver: bool,
    ) -> Vec<Binding> {
        let store = self.store;

        // `VarArgs<T>` must be the last function parameter
        if let Some((_last, leading)) = params.split_last() {
            for binding in leading {
                if let Some(annotation @ Annotation::Constructor { name, .. }) = &binding.annotation
                    && name == "VarArgs"
                {
                    self.sink.push(diagnostics::infer::variadic_param_not_last(
                        annotation.get_span(),
                    ));
                }
            }
        }

        params
            .into_iter()
            .enumerate()
            .map(|(index, binding)| {
                let expected_param_ty = match binding.annotation {
                    // A `#[test]` handle carries a resolved type with no
                    // annotation. Honor it before falling back to the expected
                    // function type.
                    None if !binding.ty.is_uninferred() => Some(binding.ty.clone()),
                    None => expected_params.get(index).cloned(),
                    _ => None,
                };

                let binding_ty = expected_param_ty.unwrap_or_else(|| {
                    let pattern_span = &binding.pattern.get_span();

                    if handle_self_receiver
                        && let Pattern::Identifier { identifier, .. } = &binding.pattern
                        && identifier == "self"
                        && binding.annotation.is_none()
                        && let Some(impl_ty) = self.scopes.impl_receiver_type()
                    {
                        return impl_ty.clone();
                    }

                    binding
                        .annotation
                        .as_ref()
                        .map(|a| self.convert_to_type_inner(store, a, pattern_span, true, true))
                        .unwrap_or_else(|| self.new_type_var())
                });

                let (new_pattern, typed_pattern) = self.infer_pattern(
                    binding.pattern,
                    binding_ty.clone(),
                    BindingKind::Parameter {
                        mutable: binding.mutable,
                    },
                );

                Binding {
                    pattern: new_pattern,
                    annotation: binding.annotation,
                    typed_pattern: Some(typed_pattern),
                    ty: binding_ty,
                    mutable: binding.mutable,
                }
            })
            .collect()
    }

    fn infer_return_type(
        &mut self,
        annotation: &Annotation,
        expected_ty: &Type,
        span: &Span,
        default_for_unknown: Type,
    ) -> Type {
        let store = self.store;
        match annotation {
            Annotation::Unknown => {
                if let Type::Function(f) = expected_ty {
                    (*f.return_type).clone()
                } else if let Type::Nominal {
                    underlying_ty: Some(inner),
                    ..
                } = expected_ty
                    && let Type::Function(f) = inner.as_ref()
                {
                    (*f.return_type).clone()
                } else {
                    default_for_unknown
                }
            }
            _ => self.convert_to_type(store, annotation, span),
        }
    }

    fn classify_call(&self, callee: &Expression) -> CallKind {
        let store = self.store;
        let callee = callee.unwrap_parens();
        match callee {
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } => {
                let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();
                let peeled = store.deep_resolve_alias(&receiver_ty);

                let ufcs_methods = self.effective_ufcs_methods();
                let is_ufcs_member = |ty: &Type| {
                    matches!(ty, Type::Nominal { id, .. }
                        if ufcs_methods.contains(&(id.to_string(), member.to_string())))
                };
                if is_ufcs_member(&receiver_ty) || is_ufcs_member(&peeled) {
                    return CallKind::UfcsMethod;
                }

                // Native method: receiver.method() on Slice/Map/Channel/etc.
                if let Some(kind) = NativeTypeKind::from_type(&peeled) {
                    return CallKind::NativeMethod(kind);
                }

                // Cross-module tuple struct constructor (e.g. `mod.Point(1, 2)`)
                if let Some(module_id) = receiver
                    .get_type()
                    .resolve_in(&self.env)
                    .as_import_namespace()
                {
                    let qualified = Symbol::from_parts(module_id, member);
                    if matches!(
                        store.get_definition(&qualified).map(|d| &d.body),
                        Some(DefinitionBody::Struct {
                            kind: StructKind::Tuple,
                            ..
                        })
                    ) {
                        return CallKind::TupleStructConstructor;
                    }
                }
            }
            Expression::Identifier { value, .. } => {
                let qualified = self.qualify_name(value);
                let definition = store.get_definition(&qualified);
                if definition.is_none() && value == "assert_type" {
                    return CallKind::AssertType;
                }
                if self.is_tuple_struct_definition(definition, callee) {
                    return CallKind::TupleStructConstructor;
                }

                if let Some(kind) = NativeTypeKind::from_constructor_path(value) {
                    return CallKind::NativeConstructor(kind);
                }

                // Native method identifier: Slice.contains(s, x), Map.delete(m, k), etc.
                if let Some((prefix, _method)) = value.split_once('.')
                    && let Some(kind) = NativeTypeKind::from_name(prefix)
                {
                    return CallKind::NativeMethodIdentifier(kind);
                }

                // Receiver method UFCS: Type.method(receiver, args)
                if let Some(kind) = self.try_classify_receiver_ufcs(value) {
                    return kind;
                }
            }
            _ => {}
        }
        CallKind::Regular
    }

    /// Classify `Type.method(receiver, args)` as `ReceiverMethodUfcs`.
    /// Uses scope-aware name resolution instead of the old suffix-matching heuristic.
    fn try_classify_receiver_ufcs(&self, value: &str) -> Option<CallKind> {
        let store = self.store;
        let last_dot = value.rfind('.')?;
        let method = &value[last_dot + 1..];
        let type_part = &value[..last_dot];

        // Resolve type name using checker's scope-aware lookup
        let qualified_name = self.lookup_qualified_name(store, type_part)?;

        // Follow type-alias chains through Simple/Compound underlying types
        // (e.g. `type MyString = string` → look up methods on `prelude.string`).
        let method_ty = store
            .get_definition(&qualified_name)
            .and_then(|definition| match &definition.body {
                DefinitionBody::Struct { methods, .. } => methods.get(method).cloned(),
                DefinitionBody::Enum { methods, .. } => methods.get(method).cloned(),
                DefinitionBody::TypeAlias { methods, .. } => {
                    let alias_ty = &definition.ty;
                    methods.get(method).cloned().or_else(|| {
                        // Follow the alias to its underlying type.
                        let underlying = match alias_ty {
                            Type::Forall { body, .. } => body.as_ref(),
                            other => other,
                        };
                        let underlying_key: Option<String> = match underlying {
                            Type::Simple(kind) => Some(format!("prelude.{}", kind.leaf_name())),
                            Type::Compound { kind, .. } => {
                                Some(format!("prelude.{}", kind.leaf_name()))
                            }
                            _ => None,
                        };
                        underlying_key.and_then(|k| store.get_own_methods(&k)?.get(method).cloned())
                    })
                }
                _ => None,
            })?;

        let has_self = match &method_ty {
            Type::Function(f) => !f.params.is_empty(),
            Type::Forall { body, .. } => {
                if let Type::Function(f) = body.as_ref() {
                    !f.params.is_empty()
                } else {
                    false
                }
            }
            _ => false,
        };

        if !has_self {
            return None;
        }

        // If it's a UFCS-lowered method, skip — the emitter handles it differently
        if self
            .effective_ufcs_methods()
            .contains(&(qualified_name.to_string(), method.to_string()))
        {
            return None;
        }

        let is_public = store
            .get_definition(&Symbol::from_parts(&qualified_name, method))
            .map(|d| d.visibility().is_public())
            .unwrap_or(false);

        Some(CallKind::ReceiverMethodUfcs { is_public })
    }

    /// Check if a definition (or type alias target) is a multi-field tuple struct constructor.
    fn is_tuple_struct_definition(
        &self,
        definition: Option<&Definition>,
        callee: &Expression,
    ) -> bool {
        let store = self.store;
        // Direct tuple struct
        if matches!(
            definition.map(|d| &d.body),
            Some(DefinitionBody::Struct {
                kind: StructKind::Tuple,
                ..
            })
        ) {
            return true;
        }
        // Type alias → follow to the underlying struct via the callee's return type
        if matches!(
            definition.map(|d| &d.body),
            Some(DefinitionBody::TypeAlias { .. })
        ) {
            let ty = callee.get_type().resolve_in(&self.env);
            let return_ty = match ty.unwrap_forall() {
                Type::Function(f) => f.return_type.as_ref().clone(),
                _ => return false,
            };
            if let Type::Nominal { id, .. } = return_ty.resolve_in(&self.env) {
                return matches!(
                    store.get_definition(&id).map(|d| &d.body),
                    Some(DefinitionBody::Struct {
                        kind: StructKind::Tuple,
                        ..
                    })
                );
            }
        }
        false
    }

    fn is_panic_call(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier { value, .. } => value == "panic",
            _ => false,
        }
    }

    /// `Map.delete` and `Slice.copy_from` modify the receiver in place, so it
    /// needs `mut`. `append` is pure (it returns a new slice and needs no
    /// `mut`): growing in place is the reassignment `s = s.append(x)`, checked
    /// as an ordinary assignment, including the rejection of writing back to a
    /// non-addressable map-value field.
    fn check_native_mutating_call(&mut self, callee: &Expression, span: &Span) {
        let store = self.store;
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee
        else {
            return;
        };
        let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();

        let is_mutating = match receiver_ty.get_name() {
            Some("Map") => member == "delete",
            Some("Slice") => member == "copy_from",
            _ => false,
        };
        if !is_mutating {
            return;
        }
        let Some(var_name) = receiver.get_var_name() else {
            return;
        };
        if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
            self.facts.mark_alias_mutated(binding_id);
        }
        let is_deref = contains_deref(receiver);
        let binding_is_ref = self
            .scopes
            .lookup_value(&var_name)
            .map(|t| t.resolve_in(&self.env).is_ref())
            .unwrap_or(false);
        if !is_deref
            && !binding_is_ref
            && !self.scopes.lookup_mutable(&var_name)
            && !self.imports.imported_modules.contains_key(&var_name)
        {
            let is_pattern_binding = self
                .scopes
                .lookup_binding_id(&var_name)
                .and_then(|id| self.facts.bindings.get(&id))
                .is_some_and(|b| b.kind.is_pattern_position());
            let is_const = self.is_const_var(store, &var_name);
            self.sink.push(diagnostics::infer::disallowed_mutation(
                &var_name,
                *span,
                None,
                is_pattern_binding,
                is_const,
            ));
        }
    }

    fn check_mut_param_arguments(
        &mut self,
        args: &[Expression],
        param_types: &[Type],
        param_mutability: &[bool],
        callee: &Expression,
    ) {
        let callee_label = callee_label(callee);
        for (i, arg) in args.iter().enumerate() {
            if !param_mutability.get(i).copied().unwrap_or(false) {
                continue;
            }
            let Some(param_ty) = param_types.get(i) else {
                continue;
            };
            self.check_arg_against_mut_param(arg, param_ty, &callee_label);
        }
    }

    fn check_arg_against_mut_param(
        &mut self,
        arg: &Expression,
        param_ty: &Type,
        callee_label: &str,
    ) {
        let store = self.store;
        if !can_carry_mutation_across_fn_boundary(param_ty, &self.env, store) {
            return;
        }
        if let Some(source) = self.non_severing_clone_source(arg) {
            self.sink
                .push(diagnostics::infer::mut_arg_clone_does_not_sever(
                    &source,
                    callee_label,
                    arg.get_span(),
                ));
            return;
        }
        let Some(var_name) = arg.get_var_name() else {
            return;
        };
        if !self.scopes.lookup_mutable(&var_name) {
            self.sink
                .push(diagnostics::infer::immutable_argument_to_mut_param(
                    &var_name,
                    callee_label,
                    arg.get_span(),
                ));
        }
        if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
            self.facts.mark_alias_mutated(binding_id);
        }
    }

    /// Verify the substring arg is a range type over `int`; emit a `Range<int>` mismatch otherwise.
    fn validate_substring_range_arg(&mut self, arg: &Expression) {
        let store = self.store;
        let arg_ty = arg.get_type().resolve_in(&self.env);
        let arg_span = arg.get_span();
        let int_ty = self.type_int();

        if let Some(peeled) = peel_to_range_type(&arg_ty) {
            if let Some(inner) = peeled.get_type_params().and_then(|p| p.first()) {
                self.unify(&int_ty, inner, &arg_span);
            }
        } else {
            let expected = self.type_range(store, int_ty);
            self.unify(&expected, &arg_ty, &arg_span);
        }
    }

    /// Index of the `Range` param to relax for a native-string `substring` call, or `None`.
    fn substring_carve_out_param_idx(
        &self,
        call_kind: CallKind,
        callee: &Expression,
        param_types: &[Type],
    ) -> Option<usize> {
        if !matches!(
            call_kind,
            CallKind::NativeMethod(NativeTypeKind::String)
                | CallKind::NativeMethodIdentifier(NativeTypeKind::String)
        ) {
            return None;
        }
        let is_substring = match callee {
            Expression::DotAccess { member, .. } => member.as_str() == "substring",
            Expression::Identifier { value, .. } => value
                .rsplit_once('.')
                .is_some_and(|(_, method)| method == "substring"),
            _ => false,
        };
        if !is_substring {
            return None;
        }
        param_types.iter().position(|p| {
            p.resolve_in(&self.env)
                .get_name()
                .is_some_and(|n| n == "Range")
        })
    }
}

fn receiver_inferred_prefix_count(body: &Type, vars: &[EcoString]) -> usize {
    let Type::Function(f) = body else {
        return 0;
    };
    let Some(self_param) = f.params.first() else {
        return 0;
    };
    let self_ty = self_param.strip_refs();
    vars.iter()
        .take_while(|var| self_ty.contains_type(&Type::Parameter((*var).clone())))
        .count()
}

fn callee_label(expr: &Expression) -> String {
    match expr {
        Expression::Identifier { value, .. } => format!("`{}()`", value),
        Expression::DotAccess {
            expression, member, ..
        } => match expression.as_ref() {
            Expression::Identifier { value, .. } => format!("`{}.{}()`", value, member),
            _ => "the function".to_string(),
        },
        _ => "the function".to_string(),
    }
}

fn is_negative_integer_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression: inner,
            ..
        } if matches!(
            inner.unwrap_parens(),
            Expression::Literal { literal: Literal::Integer { value, .. }, .. } if *value > 0
        )
    )
}
