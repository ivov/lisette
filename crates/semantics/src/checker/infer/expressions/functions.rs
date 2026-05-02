use rustc_hash::FxHashMap as HashMap;

use crate::checker::EnvResolve;
use syntax::ast::BindingKind;
use syntax::ast::{Annotation, Binding, Expression, Pattern, Span, StructKind};
use syntax::program::{CallKind, Definition, NativeTypeKind};
use syntax::types::{Bound, SubstitutionMap, Symbol, Type, substitute, unqualified_name};

use super::super::TaskState;
use super::super::unify::Dispatched;
use super::primitives::contains_deref;
use crate::checker::scopes::UseContext;
use crate::store::{ENTRY_MODULE_ID, Store};

impl TaskState<'_> {
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

    pub(crate) fn has_map_field_in_chain(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::DotAccess { expression, .. } => {
                self.is_map_indexed_access(expression) || self.has_map_field_in_chain(expression)
            }
            _ => false,
        }
    }

    fn is_map_indexed_access(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::IndexedAccess { expression, .. } => {
                expression.get_type().resolve_in(&self.env).has_name("Map")
            }
            _ => false,
        }
    }
}

fn has_numeric_member_in_chain(expression: &Expression) -> bool {
    let mut current = expression.unwrap_parens();
    while let Expression::DotAccess {
        expression: inner,
        member,
        ..
    } = current
    {
        if member.parse::<usize>().is_ok() {
            return true;
        }
        current = inner.unwrap_parens();
    }
    false
}

impl TaskState<'_> {
    pub(super) fn infer_function(
        &mut self,
        store: &mut Store,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
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

        for g in &generics {
            let qualified_name = self.qualify_name(&g.name);

            for b in &g.bounds {
                let bound_ty = self.convert_bound_to_type(store, b, &span);

                self.scopes
                    .current_mut()
                    .trait_bounds
                    .get_or_insert_with(HashMap::default)
                    .entry(qualified_name.clone())
                    .or_default()
                    .push(bound_ty.clone());

                bounds.push(Bound {
                    param_name: g.name.clone(),
                    generic: Type::Parameter(g.name.clone()),
                    ty: bound_ty,
                });
            }
        }

        let resolved_expected = expected_ty.resolve_in(&self.env);
        let expected_params = resolved_expected.get_function_params().unwrap_or_default();
        let new_params = self.infer_function_params(store, params, expected_params, true);

        let unit_ty = self.type_unit();
        let return_ty = self.infer_return_type(
            store,
            &return_annotation,
            &resolved_expected,
            &span,
            unit_ty,
        );

        self.scopes.current_mut().fn_return_type = Some(return_ty.clone());

        let base_fn_ty = Type::Function {
            param_mutability: new_params.iter().map(|p| p.mutable).collect(),
            params: new_params.iter().map(|p| p.ty.clone()).collect(),
            bounds,
            return_type: return_ty.clone().into(),
        };

        // `Type::ignored()` defers the tail-position check to
        // `validators/unused_expressions.rs`, which honors `#[allow(unused_*)]`.
        let has_implicit_unit_return = return_annotation == Annotation::Unknown;
        let body_ty = if has_implicit_unit_return {
            Type::ignored()
        } else {
            return_ty.clone()
        };

        let new_body =
            self.infer_function_body(store, body, &body_ty, &return_annotation, &return_ty);

        self.scopes.pop();

        let fn_forall_ty = if generics.is_empty() {
            base_fn_ty.clone()
        } else {
            Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(base_fn_ty),
            }
        };

        let (fn_ty, _) = self.instantiate(&fn_forall_ty);

        self.unify(store, expected_ty, &fn_ty, &span);

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
        store: &mut Store,
        params: Vec<Binding>,
        return_annotation: Annotation,
        body: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        self.scopes.push();

        // Resolve type variables so that a Go function alias bound via speculative
        // unification (e.g. T = tea.Cmd) is visible as its underlying function shape.
        let resolved_expected = expected_ty.resolve_in(&self.env);
        let expected_params = resolved_expected.get_function_params().unwrap_or_default();
        let new_params = self.infer_function_params(store, params, expected_params, false);

        let default_return = self.new_type_var();
        let return_ty = self.infer_return_type(
            store,
            &return_annotation,
            &resolved_expected,
            &span,
            default_return,
        );

        self.scopes.current_mut().fn_return_type = Some(return_ty.clone());

        let base_fn_ty = Type::Function {
            param_mutability: vec![false; new_params.len()],
            params: new_params.iter().map(|p| p.ty.clone()).collect(),
            bounds: vec![],
            return_type: return_ty.clone().into(),
        };

        // Reset loop depth — closures introduce a new function scope, so
        // `defer` inside a closure body should not be flagged as "defer in loop"
        // even when the closure is lexically inside a loop.
        let saved_loop_depth = self.scopes.reset_loop_depth();
        // `Type::ignored()` defers the tail-position check to
        // `validators/unused_expressions.rs`, which honors `#[allow(unused_*)]`.
        let relax_body_to_unit = return_annotation == Annotation::Unknown && return_ty.is_unit();
        let body_ty = if relax_body_to_unit {
            Type::ignored()
        } else {
            return_ty.clone()
        };
        let new_body =
            self.infer_function_body(store, body, &body_ty, &return_annotation, &return_ty);
        self.scopes.restore_loop_depth(saved_loop_depth);

        self.scopes.pop();

        let (fn_ty, _) = self.instantiate(&base_fn_ty);

        self.unify(store, expected_ty, &fn_ty, &span);

        Expression::Lambda {
            params: new_params,
            return_annotation,
            body: new_body.into(),
            ty: fn_ty,
            span,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_function_call(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        args: Vec<Expression>,
        spread: Box<Option<Expression>>,
        type_args: Vec<Annotation>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let callee_ty = self.new_type_var();

        let prev_context = self.scopes.set_callee_context();
        let callee_expression = self.infer_expression(store, *expression, &callee_ty);
        self.scopes.restore_use_context(prev_context);

        let forall_ty = self.resolve_callee_forall_type(store, &callee_expression, &type_args);
        let (callee_ty, new_type_args) =
            self.instantiate_callee_type(store, &forall_ty, &type_args, &callee_expression, &span);

        if let Some(underlying_fn) =
            self.try_as_type_conversion(store, &callee_expression, &callee_ty)
        {
            return self.infer_type_conversion_call(
                store,
                callee_expression,
                callee_ty,
                underlying_fn,
                args,
                spread,
                new_type_args,
                span,
                expected_ty,
            );
        }

        let variadic_elem_ty = if spread.is_some() {
            callee_ty.resolve_in(&self.env).is_variadic()
        } else {
            None
        };

        let (param_types, param_mutability, return_ty, bounds) =
            self.extract_call_signature(store, callee_ty, &args, &callee_expression);

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

        // Speculatively unify the expected type with the return type before
        // checking arguments, so e.g. `Some(Text{})` with expected `Option<Printable>`
        // constrains T = Printable and the argument coerces via interface satisfaction.
        // Also fires for Go-imported named types so that `Some(tea.Quit)` in
        // context `Option<tea.Cmd>` constrains T = tea.Cmd instead of
        // collapsing to the Cmd alias's underlying function shape. Guarded
        // to named types to avoid affecting numeric literal inference.
        if self.is_generic_callee(store, &callee_expression)
            && !expected_ty.resolve_in(&self.env).is_variable()
            && !expected_ty.is_ignored()
            && self.is_enum_type(store, &return_ty.resolve_in(&self.env))
            && (self.has_interface_type_param(store, expected_ty)
                || self.has_go_named_type_param(expected_ty)
                || self.has_fn_type_param(expected_ty)
                || self.has_unknown_type_param(expected_ty))
        {
            let _ =
                self.speculatively(|this| this.try_unify(store, expected_ty, &return_ty, &span));
        }

        let new_args = self.infer_call_arguments(store, args, &param_types);
        self.check_call_arity(&param_types, &new_args, &callee_expression, &span);
        self.check_mut_param_arguments(&new_args, &param_mutability, &callee_expression);

        let new_spread = (*spread).map(|spread_expr| match variadic_elem_ty {
            Some(elem_ty) => {
                let expected = if elem_ty.is_unknown() {
                    let var = self.new_type_var();
                    self.type_slice(var)
                } else {
                    self.type_slice(elem_ty)
                };
                let inferred =
                    self.with_value_context(|s| s.infer_expression(store, spread_expr, &expected));
                if param_mutability.last().copied().unwrap_or(false) {
                    let is_external = self.is_external_callee(&callee_expression);
                    self.check_arg_against_mut_param(&inferred, is_external);
                }
                inferred
            }
            None => {
                self.sink
                    .push(diagnostics::infer::spread_on_non_variadic(span));
                self.with_value_context(|s| s.infer_expression(store, spread_expr, &Type::Error))
            }
        });

        // Capture whether expected_ty is unresolved BEFORE
        // unification, because unify will resolve a fresh variable to the
        // concrete return type (e.g. Slice<int>). A fresh variable means the
        // caller doesn't consume the result (non-last block item).
        let expected_was_variable = expected_ty.resolve_in(&self.env).is_variable();

        self.unify(store, expected_ty, &return_ty, &span);
        self.unify_trait_bounds(store, &bounds, &new_args, &span);

        // Native mutating methods (append, extend, delete) are rewritten by
        // the emitter into mutations of the receiver binding. Require `mut`
        // on the receiver when the call mutates:
        //   - delete: always mutates (no return value)
        //   - append/extend: mutates only when the result is not consumed
        let result_unused = prev_context != UseContext::Value && {
            let resolved = expected_ty.resolve_in(&self.env);
            resolved.is_unit() || resolved.is_ignored() || expected_was_variable
        };
        self.check_native_mutating_call(store, &callee_expression, result_unused, &span);

        if self.is_generic_callee(store, &callee_expression)
            && type_args.is_empty()
            && !self.is_enum_type(store, &return_ty.resolve_in(&self.env))
        {
            self.facts
                .generic_call_checks
                .push(crate::facts::GenericCallCheck {
                    return_ty: return_ty.clone(),
                    span,
                });
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

        let call_kind = self.classify_call(store, &callee_expression);

        Expression::Call {
            expression: callee_expression.into(),
            args: new_args,
            spread: Box::new(new_spread),
            type_args: new_type_args,
            ty: call_ty,
            span,
            call_kind: Some(call_kind),
        }
    }

    fn resolve_callee_forall_type(
        &mut self,
        store: &Store,
        expression: &Expression,
        type_args: &[Annotation],
    ) -> Type {
        if type_args.is_empty() {
            return expression.get_type();
        }

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

    fn is_generic_callee(&self, store: &Store, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier { value, .. } => self
                .lookup_type(store, value)
                .map(|ty| matches!(ty, Type::Forall { .. }))
                .unwrap_or(false),
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } => {
                let receiver_ty = receiver.get_type().resolve_in(&self.env);
                self.get_all_methods(store, &receiver_ty.strip_refs())
                    .get(member)
                    .map(|ty| matches!(ty, Type::Forall { .. }))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    fn instantiate_callee_type(
        &mut self,
        store: &mut Store,
        forall_ty: &Type,
        type_args: &[Annotation],
        callee_expression: &Expression,
        span: &Span,
    ) -> (Type, Vec<Annotation>) {
        let Type::Forall { vars, body } = forall_ty else {
            if !type_args.is_empty() {
                self.sink.push(diagnostics::infer::type_args_on_non_generic(
                    type_args.len(),
                    *span,
                ));
            }
            let (instantiated, _) = self.instantiate(forall_ty);
            return (instantiated.resolve_in(&self.env), vec![]);
        };

        if type_args.is_empty() {
            let (instantiated, _) = self.instantiate(forall_ty);
            return (instantiated.resolve_in(&self.env), vec![]);
        }

        // For DotAccess method calls, accept type args that provide only the
        // method-own generics (excluding receiver/impl generics).
        let receiver_generics_count =
            if let Expression::DotAccess { expression, .. } = callee_expression {
                let receiver_ty = expression
                    .get_type()
                    .resolve_in(&self.env)
                    .strip_refs()
                    .clone();
                self.get_receiver_generics_count(store, &receiver_ty)
            } else {
                0
            };

        let method_only_count = vars.len().saturating_sub(receiver_generics_count);
        let is_full_arity = type_args.len() == vars.len();
        let is_method_only_arity =
            receiver_generics_count > 0 && type_args.len() == method_only_count;

        if !is_full_arity && !is_method_only_arity {
            let actual_types: Vec<Type> = type_args
                .iter()
                .map(|arg| self.convert_to_type(store, arg, span))
                .collect();
            let vars_as_str: Vec<String> = vars.iter().map(|s| s.to_string()).collect();
            self.sink.push(diagnostics::infer::generics_arity_mismatch(
                &vars_as_str,
                type_args,
                &actual_types,
                *span,
            ));
        }

        let mut instantiated = if is_method_only_arity {
            let mut map: SubstitutionMap = SubstitutionMap::default();
            for var in &vars[..receiver_generics_count] {
                map.insert(var.clone(), self.new_type_var());
            }
            for (var, ann) in vars[receiver_generics_count..].iter().zip(type_args.iter()) {
                map.insert(var.clone(), self.convert_to_type(store, ann, span));
            }
            substitute(body, &map)
        } else {
            self.instantiate_from_annotations(store, vars, body, type_args, span)
        };

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
                && let Type::Function {
                    ref mut params,
                    ref mut param_mutability,
                    ..
                } = instantiated
                && !params.is_empty()
            {
                let receiver_param = params.remove(0);
                if !param_mutability.is_empty() {
                    param_mutability.remove(0);
                }
                let receiver_ty_stripped = receiver_ty.strip_refs();
                if receiver_param.is_ref() && !receiver_ty.is_ref() {
                    if let Some(inner) = receiver_param.inner() {
                        self.unify(store, &inner, &receiver_ty_stripped, span);
                    }
                } else {
                    self.unify(store, &receiver_param, &receiver_ty_stripped, span);
                }
            }
            self.unify(store, &instantiated, &callee_expression.get_type(), span);
        }

        (instantiated, type_args.to_vec())
    }

    fn extract_call_signature(
        &mut self,
        store: &Store,
        callee_ty: Type,
        args: &[Expression],
        callee_expression: &Expression,
    ) -> (Vec<Type>, Vec<bool>, Type, Vec<Bound>) {
        let arg_count = args.len();
        let callee_ty = callee_ty.resolve_in(&self.env);
        let bounds = callee_ty.get_bounds().to_vec();
        let mut param_mutability = callee_ty.get_param_mutability().to_vec();
        let is_variadic = callee_ty.is_variadic();

        let (param_types, return_ty) = match self.extract_function_type(store, &callee_ty) {
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

    fn extract_function_type(&self, store: &Store, ty: &Type) -> Option<(Vec<Type>, Type)> {
        let fn_type = |ty: &Type| -> Option<(Vec<Type>, Type)> {
            if let Type::Function {
                params,
                return_type,
                ..
            } = ty
            {
                Some((params.clone(), (**return_type).clone()))
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
            && let Some(Definition::TypeAlias { ty: alias_ty, .. }) = store.get_definition(id)
        {
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

    fn try_as_type_conversion(
        &self,
        store: &Store,
        callee: &Expression,
        callee_ty: &Type,
    ) -> Option<Type> {
        let Type::Nominal {
            id,
            underlying_ty: Some(underlying),
            ..
        } = callee_ty
        else {
            return None;
        };

        if !matches!(underlying.as_ref(), Type::Function { .. }) {
            return None;
        }

        if !matches!(store.get_definition(id), Some(Definition::TypeAlias { .. })) {
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
        store: &mut Store,
        callee_expression: Expression,
        named_ty: Type,
        underlying_fn: Type,
        args: Vec<Expression>,
        spread: Box<Option<Expression>>,
        type_args: Vec<Annotation>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if let Some(spread_expr) = *spread {
            self.sink
                .push(diagnostics::infer::spread_on_non_variadic(span));
            self.with_value_context(|s| s.infer_expression(store, spread_expr, &Type::Error));
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
                .map(|arg| {
                    self.with_value_context(|s| s.infer_expression(store, arg, &Type::Error))
                })
                .collect();
            self.unify(store, expected_ty, &Type::Error, &span);
            return Expression::Call {
                expression: callee_expression.into(),
                args: new_args,
                spread: Box::new(None),
                type_args,
                ty: Type::Error,
                span,
                call_kind: Some(CallKind::Regular),
            };
        }

        let arg = args.into_iter().next().unwrap();
        let new_arg = self.with_value_context(|s| s.infer_expression(store, arg, &underlying_fn));

        self.unify(store, expected_ty, &named_ty, &span);

        Expression::Call {
            expression: callee_expression.into(),
            args: vec![new_arg],
            spread: Box::new(None),
            type_args,
            ty: named_ty,
            span,
            call_kind: Some(CallKind::Regular),
        }
    }

    fn infer_call_arguments(
        &mut self,
        store: &mut Store,
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
                self.with_value_context(|s| s.infer_expression(store, arg, &expected_ty))
            })
            .collect()
    }

    fn unify_trait_bounds(
        &mut self,
        store: &Store,
        bounds: &[Bound],
        args: &[Expression],
        fallback_span: &Span,
    ) {
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

            if self.dispatch_builtin_bound(store, bound, &resolved_ty, &span) == Dispatched::Handled
            {
                continue;
            }

            let interface_ty = bound.ty.resolve_in(&self.env);
            let Type::Nominal { id, params, .. } = interface_ty else {
                continue;
            };

            let Some(interface) = store.get_interface(&id).cloned() else {
                continue;
            };

            let _ = self.satisfies_interface(store, &resolved_ty, &interface, &params, &span);
        }
    }

    fn infer_function_body(
        &mut self,
        store: &mut Store,
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

        self.infer_expression(store, *body, body_ty)
    }

    fn infer_function_params(
        &mut self,
        store: &mut Store,
        params: Vec<Binding>,
        expected_params: &[Type],
        handle_self_receiver: bool,
    ) -> Vec<Binding> {
        params
            .into_iter()
            .enumerate()
            .map(|(index, binding)| {
                let expected_param_ty = match binding.annotation {
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
                        .map(|a| self.convert_to_type(store, a, pattern_span))
                        .unwrap_or_else(|| self.new_type_var())
                });

                let (new_pattern, typed_pattern) = self.infer_pattern(
                    store,
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
        store: &Store,
        annotation: &Annotation,
        expected_ty: &Type,
        span: &Span,
        default_for_unknown: Type,
    ) -> Type {
        match annotation {
            Annotation::Unknown => {
                if let Type::Function { return_type, .. } = expected_ty {
                    (**return_type).clone()
                } else if let Type::Nominal {
                    underlying_ty: Some(inner),
                    ..
                } = expected_ty
                    && let Type::Function { return_type, .. } = inner.as_ref()
                {
                    (**return_type).clone()
                } else {
                    default_for_unknown
                }
            }
            _ => self.convert_to_type(store, annotation, span),
        }
    }

    fn classify_call(&self, store: &Store, callee: &Expression) -> CallKind {
        let callee = callee.unwrap_parens();
        match callee {
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } => {
                let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();

                // UFCS method: receiver.method() where method is a free function
                if let Type::Nominal { id, .. } = &receiver_ty
                    && self
                        .ufcs_methods
                        .contains(&(id.to_string(), member.to_string()))
                {
                    return CallKind::UfcsMethod;
                }

                // Native method: receiver.method() on Slice/Map/Channel/etc.
                if let Some(kind) = NativeTypeKind::from_type(&receiver_ty) {
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
                        store.get_definition(&qualified),
                        Some(Definition::Struct {
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
                if self.is_tuple_struct_definition(store, definition, callee) {
                    return CallKind::TupleStructConstructor;
                }

                // Native constructor: Channel.new, Map.new, Slice.new
                let constructor_kind = match value.as_str() {
                    "Channel.new" | "Channel.buffered" => Some(NativeTypeKind::Channel),
                    "Map.new" => Some(NativeTypeKind::Map),
                    "Slice.new" => Some(NativeTypeKind::Slice),
                    _ => None,
                };
                if let Some(kind) = constructor_kind {
                    return CallKind::NativeConstructor(kind);
                }

                // Native method identifier: Slice.contains(s, x), Map.delete(m, k), etc.
                if let Some((prefix, _method)) = value.split_once('.')
                    && let Some(kind) = NativeTypeKind::from_name(prefix)
                {
                    return CallKind::NativeMethodIdentifier(kind);
                }

                // Receiver method UFCS: Type.method(receiver, args)
                if let Some(kind) = self.try_classify_receiver_ufcs(store, value) {
                    return kind;
                }
            }
            _ => {}
        }
        CallKind::Regular
    }

    /// Classify `Type.method(receiver, args)` as `ReceiverMethodUfcs`.
    /// Uses scope-aware name resolution instead of the old suffix-matching heuristic.
    fn try_classify_receiver_ufcs(&self, store: &Store, value: &str) -> Option<CallKind> {
        let last_dot = value.rfind('.')?;
        let method = &value[last_dot + 1..];
        let type_part = &value[..last_dot];

        // Resolve type name using checker's scope-aware lookup
        let qualified_name = self.lookup_qualified_name(store, type_part)?;

        // Follow type-alias chains through Simple/Compound underlying types
        // (e.g. `type MyString = string` → look up methods on `prelude.string`).
        let method_ty = store
            .get_definition(&qualified_name)
            .and_then(|definition| match definition {
                Definition::Struct { methods, .. } => methods.get(method).cloned(),
                Definition::Enum { methods, .. } => methods.get(method).cloned(),
                Definition::TypeAlias {
                    methods,
                    ty: alias_ty,
                    ..
                } => {
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
            Type::Function { params, .. } => !params.is_empty(),
            Type::Forall { body, .. } => {
                if let Type::Function { params, .. } = body.as_ref() {
                    !params.is_empty()
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
            .ufcs_methods
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
        store: &Store,
        definition: Option<&Definition>,
        callee: &Expression,
    ) -> bool {
        // Direct tuple struct
        if matches!(
            definition,
            Some(Definition::Struct {
                kind: StructKind::Tuple,
                ..
            })
        ) {
            return true;
        }
        // Type alias → follow to the underlying struct via the callee's return type
        if matches!(definition, Some(Definition::TypeAlias { .. })) {
            let ty = callee.get_type().resolve_in(&self.env);
            let return_ty = match ty.unwrap_forall() {
                Type::Function { return_type, .. } => return_type.as_ref().clone(),
                _ => return false,
            };
            if let Type::Nominal { id, .. } = return_ty.resolve_in(&self.env) {
                return matches!(
                    store.get_definition(&id),
                    Some(Definition::Struct {
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

    fn is_external_callee(&self, expression: &Expression) -> bool {
        if let Expression::DotAccess {
            expression: base, ..
        } = expression
            && let Expression::Identifier { value, .. } = base.as_ref()
        {
            return self
                .imports
                .prefix_to_module
                .get(value.as_ref())
                .is_some_and(|module_id| module_id.starts_with("go:"));
        }
        false
    }

    /// Check that native mutating methods (append, extend,
    /// delete) are called on mutable receivers. The emitter rewrites these into
    /// mutations, so the checker must enforce `mut` on the binding.
    ///
    /// - `delete`: always mutates (returns unit, modifies map in-place)
    /// - `append`/`extend`: mutates when the return value is discarded (the
    ///   emitter rewrites to `s = append(s, ...)` in statement position)
    fn check_native_mutating_call(
        &mut self,
        store: &Store,
        callee: &Expression,
        result_unused: bool,
        span: &Span,
    ) {
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee
        else {
            return;
        };
        let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();

        // append/extend on a map entry field generates an invalid write-back
        // (Go map values aren't addressable, so `m[k].field = append(...)` fails).
        // Newtype .0 access is excluded — the emitter treats it as non-lvalue.
        if matches!(receiver_ty.get_name(), Some("Slice"))
            && (member == "append" || member == "extend")
            && self.has_map_field_in_chain(receiver)
            && !has_numeric_member_in_chain(receiver)
        {
            self.sink
                .push(diagnostics::infer::map_field_chain_assignment(*span));
            return;
        }

        let is_mutating = match receiver_ty.get_name() {
            Some("Slice") => {
                // append/extend only mutate when the result is discarded
                (member == "append" || member == "extend") && result_unused
            }
            Some("Map") => member == "delete",
            _ => false,
        };
        if !is_mutating {
            return;
        }
        let Some(var_name) = receiver.get_var_name() else {
            return;
        };
        if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
            self.facts.mark_mutated(binding_id);
        }
        let is_deref = contains_deref(receiver);
        let binding_is_ref = self
            .scopes
            .lookup_value(&var_name)
            .map(|t| t.resolve_in(&self.env).is_ref())
            .unwrap_or(false);
        if !is_deref && !binding_is_ref && !self.scopes.lookup_mutable(&var_name) {
            let is_match_arm = self
                .scopes
                .lookup_binding_id(&var_name)
                .and_then(|id| self.facts.bindings.get(&id))
                .is_some_and(|b| b.kind.is_match_arm());
            let is_const = self.is_const_var(store, &var_name);
            self.sink.push(diagnostics::infer::disallowed_mutation(
                &var_name,
                *span,
                None,
                is_match_arm,
                is_const,
            ));
        }
    }

    fn check_mut_param_arguments(
        &mut self,
        args: &[Expression],
        param_mutability: &[bool],
        callee: &Expression,
    ) {
        let is_external = self.is_external_callee(callee);
        for (i, arg) in args.iter().enumerate() {
            if param_mutability.get(i).copied().unwrap_or(false) {
                self.check_arg_against_mut_param(arg, is_external);
            }
        }
    }

    fn check_arg_against_mut_param(&mut self, arg: &Expression, is_external: bool) {
        let Some(var_name) = arg.get_var_name() else {
            return;
        };
        if !self.scopes.lookup_mutable(&var_name) {
            self.sink
                .push(diagnostics::infer::immutable_argument_to_mut_param(
                    &var_name,
                    arg.get_span(),
                    is_external,
                ));
        }
        if let Some(binding_id) = self.scopes.lookup_binding_id(&var_name) {
            self.facts.mark_mutated(binding_id);
        }
    }
}
