use crate::checker::EnvResolve;
use crate::checker::infer::BuiltinBound;
use crate::checker::infer::expressions::comparison::{
    check_never_comparable, check_never_comparable_with_bounds, check_not_comparable_with_bounds,
};
use syntax::EcoString;
use syntax::ast::{Annotation, Generic, Span};
use syntax::program::DefinitionBody;
use syntax::types::{
    FunctionParameter, SubstitutionMap, Symbol, Type, substitute, unqualified_name,
};

use crate::checker::TaskState;
use crate::checker::scopes::DeferredMapKeyCheck;
use crate::generics::apply_bounds;
use crate::prelude::PRELUDE_PACKAGE_ID;
use crate::store::Store;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypePosition {
    Value,
    Bound,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeArgumentChecks {
    All,
    Descendants,
    Deferred,
}

impl TypeArgumentChecks {
    fn current(self) -> bool {
        self == Self::All
    }

    fn nested(self) -> Self {
        match self {
            Self::Descendants => Self::All,
            other => other,
        }
    }
}

#[derive(Clone, Copy)]
struct ConvertMode {
    variadic_allowed: bool,
    type_argument_checks: TypeArgumentChecks,
    position: TypePosition,
}

impl ConvertMode {
    fn nested(self) -> Self {
        ConvertMode {
            variadic_allowed: false,
            type_argument_checks: self.type_argument_checks.nested(),
            position: TypePosition::Value,
        }
    }
}

impl TaskState {
    /// Resolves a generic-bound annotation. Bound-only markers like
    /// `Comparable` are admitted here; the same names in value position
    /// are flagged inside `convert_to_type`.
    pub(crate) fn convert_bound_to_type(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
    ) -> Type {
        let result = self.convert_to_type_mode(
            store,
            annotation,
            span,
            ConvertMode {
                variadic_allowed: false,
                type_argument_checks: TypeArgumentChecks::Deferred,
                position: TypePosition::Bound,
            },
        );
        if !result.contains_error() {
            self.facts
                .bound_types
                .insert(annotation.get_span(), result.clone());
        }
        result
    }

    pub(crate) fn convert_to_type(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
    ) -> Type {
        self.convert_to_type_mode(
            store,
            annotation,
            span,
            ConvertMode {
                variadic_allowed: false,
                type_argument_checks: TypeArgumentChecks::All,
                position: TypePosition::Value,
            },
        )
    }

    pub(crate) fn convert_variadic_to_type(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
    ) -> Type {
        self.convert_to_type_mode(
            store,
            annotation,
            span,
            ConvertMode {
                variadic_allowed: true,
                type_argument_checks: TypeArgumentChecks::All,
                position: TypePosition::Value,
            },
        )
    }

    pub(crate) fn convert_receiver_to_type(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
    ) -> Type {
        self.convert_to_type_mode(
            store,
            annotation,
            span,
            ConvertMode {
                variadic_allowed: false,
                type_argument_checks: TypeArgumentChecks::Descendants,
                position: TypePosition::Value,
            },
        )
    }

    fn convert_to_type_mode(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
        mode: ConvertMode,
    ) -> Type {
        match annotation {
            Annotation::Unknown => self.new_type_var(),

            Annotation::Function {
                params,
                return_type,
                ..
            } => {
                let last_param = params.len().wrapping_sub(1);
                let new_params: Vec<Type> = params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        self.convert_to_type_mode(
                            store,
                            &param.annotation,
                            span,
                            ConvertMode {
                                variadic_allowed: index == last_param,
                                ..mode.nested()
                            },
                        )
                    })
                    .collect();
                // For function type annotations, omitted return type means Unit (`()`),
                // not a type variable. This ensures `fn(T)` is `fn(T) -> ()`.
                let new_return_type = if matches!(return_type.as_ref(), Annotation::Unknown) {
                    self.type_unit()
                } else {
                    self.convert_to_type_mode(store, return_type, span, mode.nested())
                };

                Type::function(
                    new_params
                        .into_iter()
                        .zip(params)
                        .map(|(ty, param)| FunctionParameter::new(ty, param.mutable))
                        .collect(),
                    Default::default(),
                    new_return_type.into(),
                )
            }

            Annotation::Constructor { .. } => {
                self.convert_constructor_annotation(store, annotation, span, mode)
            }

            Annotation::Tuple { elements, .. } => {
                let element_types = elements
                    .iter()
                    .map(|element| self.convert_to_type_mode(store, element, span, mode.nested()))
                    .collect();
                Type::Tuple(element_types)
            }

            Annotation::Constant {
                span: const_span, ..
            } => {
                self.sink
                    .push(diagnostics::infer::integer_in_type_position(*const_span));
                Type::Error
            }

            Annotation::Opaque { .. } => {
                unreachable!("Annotation::Opaque should not be converted to a type")
            }
        }
    }

    fn convert_constructor_annotation(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
        mode: ConvertMode,
    ) -> Type {
        let Annotation::Constructor {
            name: type_name,
            params,
            span: annotation_span,
        } = annotation
        else {
            unreachable!("convert_constructor_annotation called with non-Constructor annotation");
        };
        let annotation_span = *annotation_span;
        let ConvertMode {
            variadic_allowed,
            type_argument_checks,
            position,
        } = mode;

        if type_name == "VarArgs" && !variadic_allowed {
            self.sink
                .push(diagnostics::infer::variadic_type_not_allowed(
                    annotation_span,
                ));
            return Type::Error;
        }

        // Unit is internal: `()` desugars to Constructor { name: "Unit" }.
        // Return the interned unit type directly, unless a user-defined
        // type named `Unit` exists in scope.
        if type_name == "Unit"
            && params.is_empty()
            && self.resolve_type_name(store, "Unit").is_none()
        {
            return Type::unit();
        }

        if self.lookup_generic_index(type_name).is_some() {
            if !params.is_empty() {
                self.sink.push(diagnostics::infer::type_param_with_args(
                    params.len(),
                    annotation_span,
                ));
            }
            return Type::Parameter(type_name.into());
        }

        // `Array` carries a const-integer size, so it needs its own path.
        if type_name == "Array" {
            return self.convert_array_annotation(store, params, annotation_span, span, mode);
        }

        let Some((qualified_name, ty)) =
            self.resolve_type_with_arity(store, type_name, params.len())
        else {
            if type_name == "Self" {
                let receiver = self.scopes.impl_receiver_type().map(|ty| ty.stringify());
                self.sink.push(diagnostics::infer::self_type_not_supported(
                    annotation_span,
                    receiver.as_deref(),
                ));
            } else if !self.may_name_uninferred_export(store, type_name) {
                self.sink.push(diagnostics::infer::type_not_found(
                    type_name,
                    annotation_span,
                ));
            }
            return Type::Error;
        };

        if let Some((kind, help)) = self.classify_non_type_name(store, &qualified_name, type_name) {
            self.sink.push(diagnostics::infer::value_in_type_position(
                type_name,
                kind,
                annotation_span,
                help,
            ));
            return Type::Error;
        }

        self.track_name_usage(
            store,
            &qualified_name,
            &annotation_span,
            type_name.len() as u32,
        );

        if position == TypePosition::Value
            && let Some(builtin) =
                crate::checker::infer::BuiltinBound::from_qualified_id(&qualified_name)
        {
            self.sink
                .push(diagnostics::infer::bound_only_in_value_position(
                    builtin.label(),
                    annotation_span,
                ));
            return Type::Error;
        }

        let (generics, body) = match ty {
            Type::Forall { vars, body } => (vars, *body),
            other => (vec![], other),
        };

        let concrete_args: Vec<Type> = params
            .iter()
            .map(|arg| self.convert_to_type_mode(store, arg, span, mode.nested()))
            .collect();

        if generics.len() != params.len() {
            let generics_as_str: Vec<String> = generics.iter().map(|s| s.to_string()).collect();
            self.sink.push(diagnostics::infer::generics_arity_mismatch(
                &generics_as_str,
                params,
                &concrete_args,
                *span,
            ));
        }
        if type_argument_checks.current() && qualified_name != "prelude.Map" {
            self.check_type_argument_bounds(
                store,
                &qualified_name,
                &concrete_args,
                annotation_span,
            );
        }
        let resolved_ty = if generics.is_empty() && concrete_args.is_empty() {
            body
        } else {
            let map: SubstitutionMap = generics
                .iter()
                .cloned()
                .zip(concrete_args.iter().cloned())
                .collect();
            substitute(&body, &map)
        };

        // Reject Ref<InterfaceType>: Go pointer-to-interface is invalid
        if self.is_lis(store)
            && qualified_name == "prelude.Ref"
            && params.len() == 1
            && let Some(inner) = resolved_ty.inner()
        {
            let peeled_inner = store.peel_alias(&inner.resolve_in(&self.env));
            if let Some(inner_id) = peeled_inner.get_qualified_id()
                && store.get_interface(inner_id).is_some()
            {
                self.sink.push(diagnostics::infer::ref_of_interface_type(
                    &inner,
                    annotation_span,
                ));
            }
        }

        if type_argument_checks.current()
            && qualified_name == "prelude.Map"
            && let Some(key_ty) = resolved_ty
                .get_type_params()
                .and_then(|parameters| parameters.first())
        {
            self.check_map_key_comparable(store, key_ty, annotation_span);
        }

        resolved_ty
    }

    fn convert_array_annotation(
        &mut self,
        store: &Store,
        params: &[Annotation],
        annotation_span: Span,
        span: &Span,
        mode: ConvertMode,
    ) -> Type {
        if params.len() == 1 && self.cursor.package_id == PRELUDE_PACKAGE_ID {
            let element = self.convert_to_type_mode(store, &params[0], span, mode.nested());
            return Type::Nominal {
                id: Symbol::from_parts("prelude", "Array"),
                params: vec![element],
            };
        }

        if params.len() != 2 {
            self.sink.push(diagnostics::infer::array_type_arity(
                params.len(),
                annotation_span,
            ));
            for param in params {
                let _ = self.convert_to_type_mode(store, param, span, mode.nested());
            }
            return Type::Error;
        }

        let element = self.convert_to_type_mode(store, &params[0], span, mode.nested());
        if element.contains_error() {
            return Type::Error;
        }

        if let Annotation::Constant {
            value,
            span: size_span,
            ..
        } = &params[1]
        {
            if !self.check_array_size_in_bounds(*value, *size_span) {
                return Type::Error;
            }
            return Type::Array {
                length: *value,
                element: Box::new(element),
            };
        }

        self.sink.push(diagnostics::infer::array_size_not_literal(
            params[1].get_span(),
        ));
        Type::Error
    }

    pub(crate) fn check_array_size_in_bounds(&mut self, value: u64, span: Span) -> bool {
        if value > i64::MAX as u64 {
            self.sink
                .push(diagnostics::infer::array_size_too_large(value, span));
            false
        } else {
            true
        }
    }

    fn classify_non_type_name(
        &self,
        store: &Store,
        qualified_name: &str,
        type_name: &str,
    ) -> Option<(&'static str, Option<String>)> {
        let definition = store.get_definition(qualified_name)?;
        if !definition.is_value(qualified_name) {
            return None;
        }
        let body = definition.ty.unwrap_forall();

        let is_function = matches!(body, Type::Function(_));
        let enum_id = match body {
            Type::Function(f) => f.return_type.get_qualified_id(),
            other => other.get_qualified_id(),
        };
        let variant_name = unqualified_name(qualified_name);
        let parent_enum = enum_id.filter(|id| {
            store.get_definition(id).is_some_and(|d| match &d.body {
                DefinitionBody::Enum { variants, .. } => {
                    variants.iter().any(|v| v.name == variant_name)
                }
                _ => false,
            })
        });

        if let Some(enum_id) = parent_enum {
            let enum_name = unqualified_name(enum_id);
            let mut help = if is_function {
                format!(
                    "Use `{}` for the enum type, or call `{}(...)` to construct a value",
                    enum_name, type_name
                )
            } else {
                format!("Use `{}` for the enum type", enum_name)
            };
            if enum_id == "prelude.Result" && variant_name == "Err" {
                help.push_str(". If you meant an error type, use `error`");
            }
            return Some(("enum variant", Some(help)));
        }

        if is_function {
            return Some((
                "function",
                Some("Use a function type alias or write the function type directly".to_string()),
            ));
        }

        Some(("value", Some("Only a type is allowed here".to_string())))
    }

    fn resolve_type_with_arity(
        &mut self,
        store: &Store,
        type_name: &str,
        expected_arity: usize,
    ) -> Option<(String, Type)> {
        let arity_of = |ty: &Type| match ty {
            Type::Forall { vars, .. } => vars.len(),
            _ => 0,
        };

        if !type_name.contains('.')
            && is_reserved_prelude_generic(type_name)
            && let Some((pname, pty)) = self.resolve_type_from_prelude(store, type_name)
            && arity_of(&pty) == expected_arity
        {
            return Some((pname, pty));
        }

        if let Some((qname, ty)) = self.resolve_type_name(store, type_name) {
            if arity_of(&ty) == expected_arity {
                return Some((qname, ty));
            }
            if !type_name.contains('.')
                && let Some((pname, pty)) = self.resolve_type_from_prelude(store, type_name)
                && arity_of(&pty) == expected_arity
            {
                return Some((pname, pty));
            }
            return Some((qname, ty));
        }

        self.resolve_type_from_prelude(store, type_name)
    }

    /// Substitute the `body` with the resolved `type_args`, keeping each source
    /// annotation paired with its type so callers can reuse the result without
    /// re-resolving (which would re-emit diagnostics).
    pub(crate) fn instantiate_from_annotations(
        &mut self,
        store: &Store,
        generics: &[EcoString],
        body: &Type,
        type_args: &[Annotation],
        span: &Span,
    ) -> (Type, Vec<(Annotation, Type)>) {
        let args: Vec<(Annotation, Type)> = type_args
            .iter()
            .map(|annotation| {
                (
                    annotation.clone(),
                    self.convert_to_type(store, annotation, span),
                )
            })
            .collect();

        let map: SubstitutionMap = generics
            .iter()
            .zip(args.iter())
            .map(|(name, (_, ty))| (name.clone(), ty.clone()))
            .collect();

        (substitute(body, &map), args)
    }

    /// Pre-check impl annotation for undeclared type params (e.g. `impl Container<T>`
    /// without `impl<T>`). Adds them to scope to prevent cascading errors from
    /// `convert_to_type`, and emits a diagnostic with the specific fix.
    pub(crate) fn check_undeclared_impl_type_params(
        &mut self,
        annotation: &Annotation,
        generics: &[Generic],
    ) {
        let Annotation::Constructor {
            name: receiver_name,
            params,
            ..
        } = annotation
        else {
            return;
        };

        let undeclared: Vec<_> = params
            .iter()
            .filter_map(|param| {
                let Annotation::Constructor {
                    name,
                    params: sub_params,
                    span: param_span,
                } = param
                else {
                    return None;
                };

                // Single uppercase letter not declared as a type param, always a typo.
                // Multi-letter names (Key, Error, etc.) are left to `type_not_found`.
                if sub_params.is_empty()
                    && name.len() == 1
                    && name.chars().next().is_some_and(|c| c.is_uppercase())
                    && self.lookup_generic_index(name).is_none()
                {
                    Some((name.to_string(), *param_span))
                } else {
                    None
                }
            })
            .collect();

        for (i, (name, param_span)) in undeclared.iter().enumerate() {
            self.scopes
                .insert_type_param(name.clone(), generics.len() + i);
            self.sink
                .push(diagnostics::infer::undeclared_impl_type_param(
                    name,
                    *param_span,
                    receiver_name,
                ));
        }
    }

    pub(super) fn check_map_key_comparable(&mut self, store: &Store, key_ty: &Type, span: Span) {
        let resolved = key_ty.resolve_in(&self.env);

        if self.is_lis(store) && store.resolves_to_unknown(&resolved) {
            self.sink.push(diagnostics::infer::unknown_as_map_key(span));
            return;
        }

        if let Some(reason) = check_never_comparable(&self.env, store, &resolved) {
            self.sink.push(diagnostics::infer::non_comparable_map_key(
                &resolved, reason, span,
            ));
            return;
        }
        if !self.is_lis(store) {
            return;
        }

        self.check_missing_map_key_bounds(store, &resolved, span);
    }

    fn check_missing_map_key_bounds(&mut self, store: &Store, key_ty: &Type, span: Span) {
        let mut missing = Vec::new();
        let resolved = key_ty.resolve_in(&self.env);
        let _ = check_never_comparable_with_bounds(&self.env, store, &resolved, &mut |parameter| {
            if !self.parameter_satisfies_bound(parameter, BuiltinBound::Comparable) {
                missing.push(parameter.to_string());
            }
            true
        });
        missing.sort_unstable();
        missing.dedup();
        for parameter in missing {
            self.sink
                .push(diagnostics::infer::missing_map_key_bound(&parameter, span));
        }
    }

    pub(crate) fn check_deferred_map_key_bounds(&mut self, store: &Store) {
        for check in self.scopes.take_deferred_map_key_checks() {
            match check {
                DeferredMapKeyCheck::Comparable { key, span } => {
                    let resolved = key.resolve_in(&self.env);
                    if !store.resolves_to_unknown(&resolved) {
                        self.check_map_key_comparable(store, &resolved, span);
                    }
                }
                DeferredMapKeyCheck::Bounds { key, span } => {
                    self.check_missing_map_key_bounds(store, &key, span);
                }
            }
        }
    }

    fn check_type_argument_bounds(
        &mut self,
        store: &Store,
        definition_name: &str,
        arguments: &[Type],
        span: Span,
    ) {
        let Some(definition) = store.get_definition(definition_name) else {
            return;
        };
        let generics = definition.body.generics().unwrap_or_default();
        let has_equals_method = store.definition_has_equals_method(definition_name);
        for applied in apply_bounds(generics, arguments) {
            match applied
                .required
                .get_qualified_id()
                .and_then(BuiltinBound::from_qualified_id)
            {
                Some(builtin) => {
                    let hint = has_equals_method.then(|| diagnostics::infer::EquatableFieldHint {
                        type_name: unqualified_name(definition_name),
                        param_name: applied.parameter_name.as_str(),
                    });
                    self.check_builtin_bound_argument(store, &applied.argument, builtin, span, hint)
                }
                None => self.check_interface_type_argument(
                    store,
                    &applied.argument,
                    &applied.required,
                    span,
                ),
            }
        }
    }

    fn check_interface_type_argument(
        &mut self,
        store: &Store,
        argument: &Type,
        required: &Type,
        span: Span,
    ) {
        if required.contains_error() {
            return;
        }
        let resolved_required = store.deep_resolve_alias(required);
        let Some(required_id) = resolved_required.get_qualified_id() else {
            return;
        };
        if store.get_interface(required_id).is_none() {
            return;
        }
        let argument = store.deep_resolve_alias(&argument.resolve_in(&self.env));
        if argument.is_variable()
            || matches!(argument, Type::Parameter(_))
            || argument.contains_error()
            || store.contains_unknown(&argument)
        {
            return;
        }
        self.pending_interface_bound_checks
            .push((argument, required.clone(), span));
    }

    pub(crate) fn check_builtin_bound_argument(
        &mut self,
        store: &Store,
        argument: &Type,
        required: BuiltinBound,
        span: Span,
        equals_hint: Option<diagnostics::infer::EquatableFieldHint<'_>>,
    ) {
        let resolved = store.deep_resolve_alias(&argument.resolve_in(&self.env));
        if resolved.is_variable() {
            return;
        }
        if let Type::Parameter(parameter) = &resolved {
            if !self.parameter_satisfies_bound(parameter, required) {
                self.sink.push(diagnostics::infer::missing_bound_on_param(
                    parameter,
                    required.label(),
                    span,
                ));
            }
            return;
        }

        match required {
            BuiltinBound::Comparable => {
                let mut missing_parameter = None;
                let reason = check_not_comparable_with_bounds(
                    &self.env,
                    store,
                    &resolved,
                    &mut |parameter| {
                        let satisfied =
                            self.parameter_satisfies_bound(parameter, BuiltinBound::Comparable);
                        if !satisfied && missing_parameter.is_none() {
                            missing_parameter = Some(parameter.to_string());
                        }
                        satisfied
                    },
                );
                if let Some(parameter) = missing_parameter {
                    self.sink.push(diagnostics::infer::missing_bound_on_param(
                        &parameter,
                        required.label(),
                        span,
                    ));
                } else if let Some(reason) = reason {
                    self.sink.push(diagnostics::infer::not_comparable_bound(
                        reason,
                        equals_hint,
                        span,
                    ));
                }
            }
            BuiltinBound::Ordered if !store.satisfies_ordered_constraint(&resolved) => {
                self.sink
                    .push(diagnostics::infer::not_orderable_bound(span));
            }
            BuiltinBound::Ordered => {}
        }
    }
}

fn is_reserved_prelude_generic(name: &str) -> bool {
    matches!(name, "Option" | "Result" | "Partial")
}
