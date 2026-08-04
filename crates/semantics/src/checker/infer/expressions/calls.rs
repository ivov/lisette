use crate::checker::EnvResolve;
use ecow::EcoString;
use syntax::ast::CallTypeArguments;
use syntax::ast::{Annotation, Expression, IdentifierResolution, Literal, Span, UnaryOperator};
use syntax::program::{CallKind, NativeTypeKind};
use syntax::types::{
    Bound, CompoundKind, FunctionParameter, SubstitutionMap, Symbol, Type, substitute,
    unqualified_name,
};

use super::super::unify::Dispatched;
use super::struct_call::same_nominal;
use crate::checker::infer::InferCtx;
use crate::checker::scopes::DeferredMapKeyCheck;

struct TypeConversionCall {
    callee: Expression,
    target_ty: Type,
    underlying_fn: Type,
    args: Vec<Expression>,
    spread: Option<Box<Expression>>,
    type_arguments: CallTypeArguments,
    span: Span,
}

struct PseudoConstructorCall {
    expression: Box<Expression>,
    args: Vec<Expression>,
    spread: Option<Box<Expression>>,
    type_args: Vec<Annotation>,
}

struct CallSignature {
    parameters: Vec<FunctionParameter>,
    variadic: Option<VariadicParameter>,
    return_type: Type,
    bounds: Vec<Bound>,
}

struct VariadicParameter {
    parameter: FunctionParameter,
    first_index: usize,
}

enum DeferredCallCheckTarget {
    GenericCall,
    SliceMake,
}

impl InferCtx<'_> {
    fn check_call_arity(
        &mut self,
        parameters: &[FunctionParameter],
        args: &[Expression],
        callee_expression: &Expression,
        span: &Span,
    ) {
        if parameters.len() == args.len() {
            return;
        }
        let expected: Vec<Type> = parameters
            .iter()
            .map(|param| param.ty.resolve_in(&self.env))
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

impl InferCtx<'_> {
    pub(super) fn infer_function_call(
        &mut self,
        call: Expression,
        expected_ty: &Type,
    ) -> Expression {
        let Expression::Call {
            expression,
            args,
            spread,
            type_arguments,
            span,
            ..
        } = call
        else {
            unreachable!("infer_function_call called with non-Call expression");
        };
        let type_args = type_arguments.into_annotations();
        let callee_path = expression.unwrap_parens().as_dotted_path();

        // `Array.new` has no prelude signature (no const generics), so resolve inline.
        if callee_path.as_deref() == Some("Array.new") {
            return self.infer_array_new_call(&expression, args, type_args, span, expected_ty);
        }

        let pseudo_constructor_diagnostic = match callee_path.as_deref() {
            Some("Map.make") => Some(diagnostics::infer::map_no_make_constructor(span)),
            Some("Channel.make") => Some(diagnostics::infer::channel_no_make_constructor(span)),
            _ => None,
        };
        if let Some(diagnostic) = pseudo_constructor_diagnostic {
            return self.reject_pseudo_constructor(
                diagnostic,
                PseudoConstructorCall {
                    expression,
                    args,
                    spread,
                    type_args,
                },
                span,
            );
        }

        let store = self.store;
        let callee_ty = self.new_type_var();

        let callee_expression = self
            .with_use_context(crate::checker::scopes::UseContext::Callee, |state| {
                state.infer_expression(*expression, &callee_ty)
            });

        let forall_ty = self.resolve_callee_forall_type(&callee_expression, &type_args);
        let (callee_ty, type_arguments) =
            self.instantiate_callee_type(&forall_ty, &type_args, &callee_expression, &span);

        if let Some(underlying_fn) = self.try_as_type_conversion(&callee_expression, &callee_ty) {
            return self.infer_type_conversion_call(
                TypeConversionCall {
                    callee: callee_expression,
                    target_ty: callee_ty,
                    underlying_fn,
                    args,
                    spread,
                    type_arguments,
                    span,
                },
                expected_ty,
            );
        }

        let CallSignature {
            parameters,
            variadic,
            return_type: return_ty,
            bounds,
        } = self.extract_call_signature(callee_ty, &args, &callee_expression);

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
                    || !store.contains_unknown(&resolved_expected))
            {
                let peeled = store.deep_resolve_alias(&resolved_expected);
                let _ = self.speculatively(|this| {
                    InferCtx::new(this, store).try_unify(&peeled, &return_ty, &span)
                });
            }
        }

        let call_kind = self.classify_call(&callee_expression);

        let substring_range_idx =
            self.substring_carve_out_param_idx(call_kind, &callee_expression, &parameters);
        let new_args = if let Some(idx) = substring_range_idx {
            let mut adjusted = parameters.clone();
            adjusted[idx] = adjusted[idx].with_type(self.new_type_var());
            self.infer_call_arguments(args, &adjusted)
        } else {
            self.infer_call_arguments(args, &parameters)
        };
        self.check_call_arity(&parameters, &new_args, &callee_expression, &span);
        self.check_mut_param_arguments(&new_args, &parameters, &callee_expression);

        self.check_range_to_for_variadic(
            &new_args,
            variadic.as_ref().map(|variadic| &variadic.parameter),
        );

        if let Some(idx) = substring_range_idx
            && let Some(arg) = new_args.get(idx)
        {
            self.validate_substring_range_arg(arg);
        }

        let callee_is_unresolved = callee_expression
            .get_type()
            .resolve_in(&self.env)
            .is_error();

        let new_spread = spread.map(|spread_expr| {
            self.infer_spread_argument(
                spread_expr,
                variadic.as_ref(),
                &callee_expression,
                callee_is_unresolved,
                span,
            )
        });

        let resolved_expected = store.deep_resolve_alias(&expected_ty.resolve_in(&self.env));
        let expected_is_map = matches!(
            resolved_expected.as_compound(),
            Some((CompoundKind::Map, _))
        );
        self.unify(expected_ty, &return_ty, &span);
        self.unify_trait_bounds(&bounds, &parameters, &new_args, &span);

        let resolved_return = store.deep_resolve_alias(&return_ty.resolve_in(&self.env));
        if call_kind == CallKind::TupleStructConstructor
            && let Type::Nominal { id, .. } = &resolved_return
        {
            let written_name = callee_path.as_deref().unwrap_or(id.as_str());
            self.register_construction_obligations(written_name, &resolved_return, span);
        }
        if let Some((CompoundKind::Map, arguments)) = resolved_return.as_compound()
            && let Some(key) = arguments.first()
        {
            let check = if matches!(
                call_kind,
                CallKind::NativeConstructor(NativeTypeKind::Map)
                    | CallKind::NativeMethod(NativeTypeKind::Map)
                    | CallKind::NativeMethodIdentifier(NativeTypeKind::Map)
            ) && !expected_is_map
            {
                DeferredMapKeyCheck::Comparable {
                    key: key.clone(),
                    span,
                }
            } else {
                DeferredMapKeyCheck::Bounds {
                    key: key.clone(),
                    span,
                }
            };
            self.scopes.defer_map_key_check(check);
        }

        self.check_native_mutating_call(&callee_expression, &span);
        self.check_native_equals_ufcs(&callee_expression, &new_args);

        let return_check_recorded = self.is_generic_callee(&callee_expression)
            && type_args.is_empty()
            && !self.is_enum_type(store, &resolved_return);
        if return_check_recorded {
            self.record_generic_call_check(
                return_ty.clone(),
                span,
                DeferredCallCheckTarget::GenericCall,
            );
        }

        // A zero-variadic-arg call can't infer its `VarArgs<T>` parameter from args.
        // Record the element type; the deferred pass rejects it only if it stays
        // unbound. Skip when the return-type check above already records it.
        if type_args.is_empty()
            && new_spread.is_none()
            && let Some(variadic) = &variadic
            && new_args.len() <= variadic.first_index
        {
            let already_covered = return_check_recorded
                && resolved_return.contains_type(&variadic.parameter.ty.resolve_in(&self.env));
            if !already_covered {
                self.record_generic_call_check(
                    variadic.parameter.ty.clone(),
                    span,
                    DeferredCallCheckTarget::GenericCall,
                );
            }
        }

        if type_args.is_empty() && self.callee_has_phantom_type_param(&callee_expression) {
            self.sink
                .push(diagnostics::infer::cannot_infer_type_argument(span));
        }

        // Widen to the expected interface container for codegen, only when the return is the same container.
        let call_ty = if !expected_ty.is_variable()
            && same_nominal(&resolved_expected, &resolved_return)
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
            self.record_generic_call_check(
                call_ty.clone(),
                span,
                DeferredCallCheckTarget::SliceMake,
            );
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
            spread: new_spread.map(Box::new),
            type_arguments,
            ty: call_ty,
            span,
            call_kind,
        }
    }

    /// Error-recovery rebuild for `Map.make`/`Channel.make`, which have no constructor.
    fn reject_pseudo_constructor(
        &mut self,
        diagnostic: diagnostics::LisetteDiagnostic,
        call: PseudoConstructorCall,
        span: Span,
    ) -> Expression {
        self.sink.push(diagnostic);
        let new_args: Vec<Expression> = call
            .args
            .into_iter()
            .map(|arg| self.with_value_context(|s| s.infer_expression(arg, &Type::Error)))
            .collect();
        let new_spread = call
            .spread
            .map(|s| self.with_value_context(|state| state.infer_expression(*s, &Type::Error)));
        Expression::Call {
            expression: call.expression,
            args: new_args,
            spread: new_spread.map(Box::new),
            type_arguments: CallTypeArguments::checked_without_types(call.type_args),
            ty: Type::Error,
            span,
            call_kind: CallKind::Unresolved,
        }
    }

    fn infer_spread_argument(
        &mut self,
        spread_expr: Box<Expression>,
        variadic: Option<&VariadicParameter>,
        callee_expression: &Expression,
        callee_is_unresolved: bool,
        span: Span,
    ) -> Expression {
        match variadic {
            Some(variadic) => {
                let expected = if variadic.parameter.ty.is_unknown() {
                    let var = self.new_type_var();
                    self.type_slice(var)
                } else {
                    self.type_slice(variadic.parameter.ty.clone())
                };
                let inferred =
                    self.with_value_context(|s| s.infer_expression(*spread_expr, &expected));
                if variadic.parameter.mutable {
                    let callee_label = callee_label(callee_expression);
                    self.check_arg_against_mut_param(
                        &inferred,
                        &variadic.parameter.ty,
                        &callee_label,
                    );
                }
                inferred
            }
            None => {
                if !callee_is_unresolved {
                    self.sink
                        .push(diagnostics::infer::spread_on_non_variadic(span));
                }
                self.with_value_context(|s| s.infer_expression(*spread_expr, &Type::Error))
            }
        }
    }

    fn record_generic_call_check(&mut self, ty: Type, span: Span, target: DeferredCallCheckTarget) {
        let package_id = self.cursor.package_id.clone();
        match target {
            DeferredCallCheckTarget::GenericCall => {
                self.facts
                    .deferred
                    .generic_calls
                    .push(crate::facts::GenericCallCheck {
                        ty,
                        span,
                        package_id,
                    });
            }
            DeferredCallCheckTarget::SliceMake => {
                self.facts
                    .deferred
                    .slice_makes
                    .push(crate::facts::SliceMakeCheck {
                        ty,
                        span,
                        package_id,
                    });
            }
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
                let from_package = self.cursor.package_id.clone();
                if let Err(no_zero) = self.has_zero(&array_ty, &from_package) {
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

        let callee_ty = Type::function(Vec::new(), Vec::new(), Box::new(array_ty.clone()));
        let callee_expression = Expression::Identifier {
            value: "Array.new".into(),
            ty: callee_ty,
            span: callee.get_span(),
            resolution: IdentifierResolution::Unresolved,
        };

        Expression::Call {
            expression: callee_expression.into(),
            args: new_args,
            spread: None,
            type_arguments: CallTypeArguments::checked_without_types(type_args),
            ty: array_ty,
            span,
            call_kind: CallKind::NativeConstructor(NativeTypeKind::Array),
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

    fn declared_callee_type(&mut self, expression: &Expression) -> Type {
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
                        return definition.ty.clone();
                    }
                }

                if let Some(package_id) = stripped.as_import_namespace() {
                    let qualified = Symbol::from_parts(package_id, member);
                    if let Some(definition) = store.get_definition(&qualified) {
                        return definition.ty.clone();
                    }
                }

                expression.get_type()
            }
            _ => expression.get_type(),
        }
    }

    fn is_generic_callee(&mut self, expression: &Expression) -> bool {
        matches!(self.declared_callee_type(expression), Type::Forall { .. })
    }

    fn callee_has_phantom_type_param(&mut self, expression: &Expression) -> bool {
        !phantom_type_params(&self.declared_callee_type(expression)).is_empty()
    }

    fn instantiate_callee_type(
        &mut self,
        forall_ty: &Type,
        type_args: &[Annotation],
        callee_expression: &Expression,
        span: &Span,
    ) -> (Type, CallTypeArguments) {
        let store = self.store;
        let Type::Forall { vars, body } = forall_ty else {
            if !type_args.is_empty() {
                self.sink.push(diagnostics::infer::type_args_on_non_generic(
                    type_args.len(),
                    *span,
                ));
            }
            let (instantiated, _) = self.instantiate(forall_ty);
            return (
                instantiated.resolve_in(&self.env),
                CallTypeArguments::none(),
            );
        };

        if type_args.is_empty() {
            let (instantiated, _) = self.instantiate(forall_ty);
            return (
                instantiated.resolve_in(&self.env),
                CallTypeArguments::none(),
            );
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

        let mut resolved_args: Vec<(Annotation, Type)> = Vec::new();
        let mut instantiated = if is_method_only_arity {
            let mut map: SubstitutionMap = SubstitutionMap::default();
            for var in &vars[..receiver_generics_count] {
                map.insert(var.clone(), self.new_type_var());
            }
            for (var, ann) in vars[receiver_generics_count..].iter().zip(type_args.iter()) {
                let arg_ty = self.convert_to_type(store, ann, span);
                resolved_args.push((ann.clone(), arg_ty.clone()));
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
            let resolved_types = resolved_args
                .iter()
                .map(|(_, ty)| ty.clone())
                .collect::<Vec<_>>();
            self.sink.push(diagnostics::infer::generics_arity_mismatch(
                &vars_as_str,
                type_args,
                &resolved_types,
                *span,
            ));
        }

        if let Expression::DotAccess { expression, .. } = callee_expression {
            let receiver_ty = expression.get_type().resolve_in(&self.env);

            // Only strip the receiver param for instance methods (which have `self`).
            // Instance methods: `as_instance_method` already stripped `self` from
            // the callee type, so the Forall body has one more param than the callee.
            // Static methods and package free functions: no `self`, param counts match.
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

        (instantiated, CallTypeArguments::resolved(resolved_args))
    }

    fn extract_call_signature(
        &mut self,
        callee_ty: Type,
        args: &[Expression],
        callee_expression: &Expression,
    ) -> CallSignature {
        let arg_count = args.len();
        let callee_ty = callee_ty.resolve_in(&self.env);
        let bounds = callee_ty.get_bounds().to_vec();
        let function_ty = self.store.resolve_to_function_type(&callee_ty);
        let is_variadic = function_ty.as_ref().and_then(Type::is_variadic);

        let (parameters, variadic, return_type) = match self.extract_function_type(&callee_ty) {
            Some((mut params, return_type)) => {
                let variadic = is_variadic.map(|variadic_ty| {
                    let parameter = params
                        .pop()
                        .expect("variadic function has a trailing parameter");
                    let first_index = params.len();
                    while params.len() < arg_count {
                        params.push(parameter.with_type(variadic_ty.clone()));
                    }
                    VariadicParameter {
                        parameter: parameter.with_type(variadic_ty),
                        first_index,
                    }
                });
                (params, variadic, return_type)
            }
            None if callee_ty.is_variable() => {
                let parameters = (0..arg_count)
                    .map(|_| FunctionParameter::new(self.new_type_var(), false))
                    .collect();
                (parameters, None, self.new_type_var())
            }
            None if callee_ty.resolve_in(&self.env).is_error() => {
                let parameters = (0..arg_count)
                    .map(|_| FunctionParameter::new(Type::Error, false))
                    .collect();
                (parameters, None, Type::Error)
            }
            None => {
                let callee_name = match callee_expression.unwrap_parens() {
                    Expression::Identifier {
                        value, resolution, ..
                    } if !matches!(resolution, IdentifierResolution::Binding(_)) => {
                        Some(value.as_str())
                    }
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
                    self.store.underlying_type(&callee_ty).is_some(),
                    callee_expression.get_span(),
                ));
                let parameters = (0..arg_count)
                    .map(|_| FunctionParameter::new(Type::Error, false))
                    .collect();
                (parameters, None, Type::Error)
            }
        };

        CallSignature {
            parameters,
            variadic,
            return_type,
            bounds,
        }
    }

    fn extract_function_type(&self, ty: &Type) -> Option<(Vec<FunctionParameter>, Type)> {
        let fn_type = |ty: &Type| -> Option<(Vec<FunctionParameter>, Type)> {
            if let Type::Function(f) = ty {
                Some((f.params.clone(), (*f.return_type).clone()))
            } else {
                None
            }
        };

        fn_type(ty).or_else(|| {
            self.store
                .resolve_to_function_type(ty)
                .and_then(|resolved| fn_type(&resolved))
        })
    }

    fn try_as_type_conversion(&self, callee: &Expression, callee_ty: &Type) -> Option<Type> {
        let store = self.store;
        let Type::Nominal { id, params } = callee_ty else {
            return None;
        };
        let definition = store.get_definition(id)?;
        let underlying = definition.instantiate_alias_target(params)?;
        if !matches!(underlying, Type::Function(_)) {
            return None;
        }

        let is_bare_type_name = match callee.unwrap_parens() {
            Expression::Identifier { resolution, .. } => {
                !matches!(resolution, IdentifierResolution::Binding(_))
            }
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

        Some(underlying)
    }

    fn infer_type_conversion_call(
        &mut self,
        call: TypeConversionCall,
        expected_ty: &Type,
    ) -> Expression {
        let TypeConversionCall {
            callee: callee_expression,
            target_ty: named_ty,
            underlying_fn,
            args,
            spread,
            type_arguments,
            span,
        } = call;
        if let Some(spread_expr) = spread {
            self.sink
                .push(diagnostics::infer::spread_on_non_variadic(span));
            self.with_value_context(|s| s.infer_expression(*spread_expr, &Type::Error));
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
                spread: None,
                type_arguments,
                ty: Type::Error,
                span,
                call_kind: CallKind::Regular,
            };
        }

        let arg = args.into_iter().next().unwrap();
        let new_arg = self.with_value_context(|s| s.infer_expression(arg, &underlying_fn));

        self.unify(expected_ty, &named_ty, &span);

        Expression::Call {
            expression: callee_expression.into(),
            args: vec![new_arg],
            spread: None,
            type_arguments,
            ty: named_ty,
            span,
            call_kind: CallKind::Regular,
        }
    }

    fn infer_call_arguments(
        &mut self,
        args: Vec<Expression>,
        parameters: &[FunctionParameter],
    ) -> Vec<Expression> {
        args.into_iter()
            .enumerate()
            .map(|(i, arg)| {
                let expected_ty = parameters
                    .get(i)
                    .map(|param| param.ty.clone())
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
        variadic: Option<&FunctionParameter>,
    ) {
        if variadic.is_none() {
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
        signature_params: &[FunctionParameter],
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
                && !self.generic_absorbed_via_ref_param(
                    &bound.generic,
                    signature_params.iter().map(|param| &param.ty),
                )
            {
                let _ = self.check_pointer_receivers(&resolved_ty, &interface, &id, &span);
            }
        }
    }
}

pub(crate) fn phantom_type_params(ty: &Type) -> Vec<String> {
    let Type::Forall { vars, body } = ty else {
        return Vec::new();
    };
    let Type::Function(f) = body.as_ref() else {
        return Vec::new();
    };
    vars.iter()
        .filter(|var| {
            let param = Type::Parameter((**var).clone());
            let in_signature = f
                .params
                .iter()
                .any(|function_param| function_param.ty.contains_type(&param))
                || f.return_type.contains_type(&param);
            let is_bounded = f.bounds.iter().any(|bound| bound.param_name == **var);
            !in_signature && !is_bounded
        })
        .map(|var| var.to_string())
        .collect()
}

fn receiver_inferred_prefix_count(body: &Type, vars: &[EcoString]) -> usize {
    let Type::Function(f) = body else {
        return 0;
    };
    let Some(self_param) = f.params.first() else {
        return 0;
    };
    let self_ty = self_param.ty.strip_refs();
    vars.iter()
        .take_while(|var| self_ty.contains_type(&Type::Parameter((*var).clone())))
        .count()
}

pub(super) fn callee_label(expr: &Expression) -> String {
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
