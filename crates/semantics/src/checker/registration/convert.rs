use rustc_hash::FxHashMap as HashMap;

use crate::checker::EnvResolve;
use syntax::EcoString;
use syntax::ast::{Annotation, Generic, Span};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{SubstitutionMap, Type, substitute};

use crate::checker::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    /// Resolves a generic-bound annotation. Bound-only markers like
    /// `Comparable` are admitted here; the same names in value position
    /// are flagged inside `convert_to_type`.
    pub fn convert_bound_to_type(
        &mut self,
        store: &Store,
        annotation: &Annotation,
        span: &Span,
    ) -> Type {
        self.bound_position_depth += 1;
        let result = self.convert_to_type(store, annotation, span);
        self.bound_position_depth -= 1;
        result
    }

    pub fn convert_to_type(&mut self, store: &Store, annotation: &Annotation, span: &Span) -> Type {
        match annotation {
            Annotation::Unknown => self.new_type_var(),

            Annotation::Function {
                params,
                return_type,
                ..
            } => {
                let new_params: Vec<Type> = params
                    .iter()
                    .map(|param| self.convert_to_type(store, param, span))
                    .collect();
                // For function type annotations, omitted return type means Unit (`()`),
                // not a type variable. This ensures `fn(T)` is `fn(T) -> ()`.
                let new_return_type = if matches!(return_type.as_ref(), Annotation::Unknown) {
                    self.type_unit()
                } else {
                    self.convert_to_type(store, return_type, span)
                };

                Type::Function {
                    param_mutability: vec![false; new_params.len()],
                    params: new_params,
                    bounds: Default::default(),
                    return_type: new_return_type.into(),
                }
            }

            Annotation::Constructor {
                name: type_name,
                params,
                span: annotation_span,
            } => {
                // Unit is internal — `()` desugars to Constructor { name: "Unit" }.
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
                            *annotation_span,
                        ));
                    }
                    return Type::Parameter(type_name.into());
                }

                let Some((qualified_name, ty)) =
                    self.resolve_type_with_arity(store, type_name, params.len())
                else {
                    if type_name == "Self" {
                        self.sink.push(diagnostics::infer::self_type_not_supported(
                            *annotation_span,
                        ));
                    } else {
                        self.sink.push(diagnostics::infer::type_not_found(
                            type_name,
                            *annotation_span,
                        ));
                    }
                    return Type::Error;
                };

                self.track_name_usage(
                    store,
                    &qualified_name,
                    annotation_span,
                    type_name.len() as u32,
                );

                if self.bound_position_depth == 0
                    && let Some(builtin) =
                        crate::checker::infer::BuiltinBound::from_qualified_id(&qualified_name)
                {
                    self.sink
                        .push(diagnostics::infer::bound_only_in_value_position(
                            builtin.label(),
                            *annotation_span,
                        ));
                    return Type::Error;
                }

                let (generics, body) = match &ty {
                    Type::Forall { vars, body } => (vars.clone(), body.as_ref().clone()),
                    _ => (vec![], ty.clone()),
                };

                if generics.len() != params.len() {
                    let actual_types: Vec<Type> = params
                        .iter()
                        .map(|arg| self.convert_to_type(store, arg, span))
                        .collect();
                    let generics_as_str: Vec<String> =
                        generics.iter().map(|s| s.to_string()).collect();
                    self.sink.push(diagnostics::infer::generics_arity_mismatch(
                        &generics_as_str,
                        params,
                        &actual_types,
                        *span,
                    ));
                }

                let concrete_args: Vec<Type> = params
                    .iter()
                    .map(|arg| self.convert_to_type(store, arg, span))
                    .collect();
                let map: SubstitutionMap = generics
                    .iter()
                    .cloned()
                    .zip(concrete_args.iter().cloned())
                    .collect();
                let resolved_ty = substitute(&body, &map);

                // Reject Ref<InterfaceType> — Go pointer-to-interface is invalid
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
                            *annotation_span,
                        ));
                    }
                }

                if qualified_name == "prelude.Map"
                    && !params.is_empty()
                    && let Some(key_ty) = resolved_ty
                        .get_type_params()
                        .and_then(|p| p.first().cloned())
                {
                    self.check_map_key_comparable(store, &key_ty, *annotation_span);
                }

                // Preserve alias name in emitter output. Guard against re-wrapping bodies whose
                // id already matches (function aliases are pre-wrapped by populate_type_alias).
                let body_differs = match &resolved_ty {
                    Type::Nominal { id, .. } => id.as_str() != qualified_name.as_str(),
                    Type::Simple(_) | Type::Compound { .. } => true,
                    _ => false,
                };
                if body_differs
                    && let Some(Definition {
                        body:
                            DefinitionBody::TypeAlias {
                                annotation: alias_ann,
                                ..
                            },
                        ..
                    }) = store.get_definition(&qualified_name)
                    && !alias_ann.is_opaque()
                {
                    return Type::Nominal {
                        id: qualified_name.into(),
                        params: concrete_args,
                        underlying_ty: Some(Box::new(resolved_ty)),
                    };
                }

                resolved_ty
            }

            Annotation::Tuple { elements, .. } => {
                let element_types = elements
                    .iter()
                    .map(|e| self.convert_to_type(store, e, span))
                    .collect();
                Type::Tuple(element_types)
            }

            Annotation::Opaque { .. } => {
                unreachable!("Annotation::Opaque should not be converted to a type")
            }
        }
    }

    pub(super) fn resolve_type_with_arity(
        &mut self,
        store: &Store,
        type_name: &str,
        expected_arity: usize,
    ) -> Option<(String, Type)> {
        let arity_of = |ty: &Type| match ty {
            Type::Forall { vars, .. } => vars.len(),
            _ => 0,
        };

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

    pub fn instantiate_from_annotations(
        &mut self,
        store: &Store,
        generics: &[EcoString],
        body: &Type,
        type_args: &[Annotation],
        span: &Span,
    ) -> Type {
        let args: Vec<Type> = type_args
            .iter()
            .map(|arg_ann| self.convert_to_type(store, arg_ann, span))
            .collect();

        let map: SubstitutionMap = generics
            .iter()
            .zip(args.iter())
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();

        substitute(body, &map)
    }

    /// Check that a map key type is comparable.
    /// Only rejects concrete non-comparable types (Slice, Map, Function).
    /// Type parameters are allowed here — they may be instantiated with comparable types.
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

                // Single uppercase letter not declared as a type param — always a typo.
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
                .current_mut()
                .type_params
                .get_or_insert_with(HashMap::default)
                .insert(name.clone(), generics.len() + i);
            self.sink
                .push(diagnostics::infer::undeclared_impl_type_param(
                    name,
                    *param_span,
                    receiver_name,
                ));
        }
    }

    fn check_map_key_comparable(&mut self, store: &Store, key_ty: &Type, span: Span) {
        let resolved = key_ty.resolve_in(&self.env);

        if self.is_lis(store) && resolved.resolves_to_unknown() {
            self.sink.push(diagnostics::infer::unknown_as_map_key(span));
            return;
        }

        let reason = if matches!(&resolved, Type::Function { .. }) {
            "functions"
        } else if resolved.has_name("Slice") {
            "slices"
        } else if resolved.has_name("Map") {
            "maps"
        } else {
            return;
        };

        self.sink.push(diagnostics::infer::non_comparable_map_key(
            &resolved, reason, span,
        ));
    }
}
