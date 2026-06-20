use crate::checker::EnvResolve;
use crate::checker::TypeEnv;
use crate::checker::infer::InferCtx;
use crate::checker::scopes::Scopes;
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::{DefinitionBody, EqualityUnusableReason};
use syntax::types::{CompoundKind, Type, build_substitution_map, substitute};

pub fn check_not_comparable(env: &TypeEnv, store: &Store, ty: &Type) -> Option<&'static str> {
    let resolved = store.deep_resolve_alias(ty);
    let ty = &resolved;

    if matches!(ty, Type::Function(_)) {
        return Some("functions");
    }

    if ty.has_name("Slice") {
        return Some("slices");
    }
    if ty.has_name("Map") {
        return Some("maps");
    }

    if ty.has_name("Ref") || ty.has_name("Channel") {
        return None;
    }

    if matches!(ty, Type::Var { .. }) {
        return None;
    }

    if ty.is_unknown() {
        return Some("interface values");
    }

    if let Some(underlying) = ty.get_underlying() {
        return check_not_comparable(env, store, underlying);
    }

    if matches!(ty, Type::Parameter(_)) {
        return Some("type parameters");
    }

    if let Some(name) = ty.get_qualified_id()
        && let Some(definition) = store.get_definition(name)
    {
        let type_args = ty.get_type_params().unwrap_or_default();
        let generics = match &definition.body {
            DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. } => {
                generics.as_slice()
            }
            _ => &[],
        };
        let sub_map = generics
            .iter()
            .map(|g| g.name.clone())
            .zip(type_args.iter().cloned())
            .collect();

        match &definition.body {
            DefinitionBody::Struct { fields, .. } => {
                for f in fields {
                    let field_ty = substitute(&f.ty.resolve_in(env), &sub_map);
                    if check_not_comparable(env, store, &field_ty).is_some() {
                        return Some("a struct containing non-comparable fields");
                    }
                }
            }
            DefinitionBody::Enum { variants, .. } => {
                for v in variants {
                    for f in v.fields.iter() {
                        let field_ty = substitute(&f.ty.resolve_in(env), &sub_map);
                        if check_not_comparable(env, store, &field_ty).is_some() {
                            return Some("an enum containing non-comparable fields");
                        }
                    }
                }
            }
            DefinitionBody::Interface { .. } => return Some("interface values"),
            _ => {}
        }
    }

    if let Type::Tuple(elems) = ty {
        for e in elems {
            if check_not_comparable(env, store, &e.resolve_in(env)).is_some() {
                return Some("a tuple containing non-comparable elements");
            }
        }
    }

    None
}

fn is_interface_or_unknown(store: &Store, ty: &Type) -> bool {
    let resolved = store.deep_resolve_alias(ty);
    resolved.is_unknown() || store.is_interface(&resolved)
}

pub(crate) fn type_has_usable_equals(store: &Store, ty: &Type, current_module: &str) -> bool {
    let resolved = store.deep_resolve_alias(ty);
    let Some(qualified) = resolved.get_qualified_id() else {
        return false;
    };
    store.equality_index.usable_from(qualified, current_module)
}

fn callable_unusable_equals_reason(
    store: &Store,
    ty: &Type,
    current_module: &str,
) -> Option<&'static str> {
    let id = ty.get_qualified_id()?;
    match store
        .equality_index
        .unusable_reason_from(id, current_module)?
    {
        EqualityUnusableReason::BoundMismatch => {
            Some("a type whose `equals` requires stricter bounds")
        }
        EqualityUnusableReason::UfcsLowered => Some("a type whose `equals` is not a method"),
    }
}

pub(crate) fn check_not_equatable(
    env: &TypeEnv,
    store: &Store,
    ty: &Type,
    current_module: &str,
    comparable_param: &dyn Fn(&str) -> bool,
) -> Option<&'static str> {
    let resolved = store.deep_resolve_alias(&ty.resolve_in(env));

    if let Type::Parameter(name) = &resolved
        && comparable_param(name)
    {
        return None;
    }
    if type_has_usable_equals(store, &resolved, current_module) {
        return None;
    }

    let Some(reason) = check_not_comparable(env, store, &resolved) else {
        return callable_unusable_equals_reason(store, &resolved, current_module);
    };

    match resolved.as_compound() {
        Some((CompoundKind::Slice, args)) => {
            check_not_equatable(env, store, args.first()?, current_module, comparable_param)
        }
        Some((CompoundKind::Map, args)) => {
            if map_key_not_comparable(env, store, args.first()?, comparable_param) {
                return Some("a map with a non-comparable key");
            }
            check_not_equatable(env, store, args.get(1)?, current_module, comparable_param)
        }
        _ => Some(reason),
    }
}

fn map_key_not_comparable(
    env: &TypeEnv,
    store: &Store,
    key: &Type,
    comparable_param: &dyn Fn(&str) -> bool,
) -> bool {
    let resolved = store.deep_resolve_alias(&key.resolve_in(env));
    if let Type::Parameter(name) = &resolved
        && comparable_param(name)
    {
        return false;
    }
    check_not_comparable(env, store, &resolved).is_some()
}

pub(crate) fn bound_implied(store: &Store, type_bounds: &[Type], method_bound: &Type) -> bool {
    use super::super::unify::BuiltinBound;
    let builtin = |ty: &Type| {
        ty.get_qualified_id()
            .and_then(BuiltinBound::from_qualified_id)
    };
    if let Some(method) = builtin(method_bound)
        && type_bounds
            .iter()
            .any(|tb| builtin(tb).is_some_and(|tb| tb.satisfies(method)))
    {
        return true;
    }
    type_bounds
        .iter()
        .any(|tb| bound_satisfies(store, tb, method_bound))
}

fn interface_closure_any(
    store: &Store,
    start: &Type,
    mut predicate: impl FnMut(&Type) -> bool,
) -> bool {
    let mut stack = vec![store.deep_resolve_alias(start)];
    let mut seen: Vec<Type> = Vec::new();
    while let Some(current) = stack.pop() {
        if predicate(&current) {
            return true;
        }
        if seen.contains(&current) {
            continue;
        }
        seen.push(current.clone());
        let Some(id) = current.get_qualified_id() else {
            continue;
        };
        let Some(interface) = store.get_interface(id) else {
            continue;
        };
        let map = build_substitution_map(
            &interface.generics,
            current.get_type_params().unwrap_or_default(),
        );
        for parent in &interface.parents {
            stack.push(store.deep_resolve_alias(&substitute(parent, &map)));
        }
    }
    false
}

fn bound_satisfies(store: &Store, start: &Type, target: &Type) -> bool {
    let target = store.deep_resolve_alias(target);
    interface_closure_any(store, start, |current| current == &target)
}

pub(crate) fn bounds_conflict(store: &Store, type_bounds: &[Type], impl_bound: &Type) -> bool {
    let impl_bound = store.deep_resolve_alias(impl_bound);
    let Some(impl_base) = impl_bound.get_qualified_id().map(str::to_string) else {
        return false;
    };
    type_bounds
        .iter()
        .any(|tb| closure_conflicts(store, tb, &impl_base, &impl_bound))
}

fn closure_conflicts(store: &Store, start: &Type, target_base: &str, target: &Type) -> bool {
    interface_closure_any(store, start, |current| {
        current.get_qualified_id() == Some(target_base) && current != target
    })
}

pub(crate) fn param_is_comparable(scopes: &Scopes, env: &TypeEnv, param_name: &str) -> bool {
    let mut found = false;
    scopes.for_each_bound_on_param(param_name, |bound_ty| {
        if found {
            return;
        }
        if let Some(declared) = bound_ty
            .resolve_in(env)
            .get_qualified_id()
            .and_then(super::super::unify::BuiltinBound::from_qualified_id)
            && declared.satisfies(super::super::unify::BuiltinBound::Comparable)
        {
            found = true;
        }
    });
    found
}

impl InferCtx<'_, '_> {
    pub(super) fn ensure_comparable(
        &mut self,
        ty: &Type,
        span: &Span,
        operands_match: bool,
    ) -> bool {
        let store = self.store;
        let resolved = ty.resolve_in(&self.env);
        if resolved.is_error() {
            return true;
        }
        if let Type::Parameter(name) = &resolved
            && self.parameter_satisfies_bound(name, super::super::unify::BuiltinBound::Comparable)
        {
            return true;
        }
        let Some(reason) = check_not_comparable(&self.env, store, &resolved) else {
            return true;
        };
        if is_interface_or_unknown(store, &resolved) {
            self.sink.push(diagnostics::infer::not_comparable_interface(
                &resolved, *span,
            ));
        } else if operands_match && let Some(element) = self.container_equals_element(&resolved) {
            match self.not_equatable_reason(&element) {
                Some(element_reason) => self.sink.push(
                    diagnostics::infer::not_comparable_no_equals(&resolved, element_reason, *span),
                ),
                None => self
                    .sink
                    .push(diagnostics::infer::not_comparable_use_equals(
                        &resolved, reason, *span,
                    )),
            }
        } else if operands_match
            && type_has_usable_equals(store, &resolved, self.cursor.module_id.as_str())
        {
            self.sink
                .push(diagnostics::infer::not_comparable_value_use_equals(
                    &resolved, *span,
                ));
        } else if operands_match
            && self.is_struct_or_enum(&resolved)
            && self.is_equality_derivable(&resolved)
        {
            self.sink
                .push(diagnostics::infer::not_comparable_derive_equality(
                    &resolved, *span,
                ));
        } else {
            self.sink
                .push(diagnostics::infer::not_comparable(&resolved, reason, *span));
        }
        false
    }

    pub(super) fn not_equatable_reason(&self, ty: &Type) -> Option<&'static str> {
        check_not_equatable(
            &self.env,
            self.store,
            ty,
            self.cursor.module_id.as_str(),
            &|name| {
                self.parameter_satisfies_bound(name, super::super::unify::BuiltinBound::Comparable)
            },
        )
    }

    fn is_struct_or_enum(&self, ty: &Type) -> bool {
        let resolved = self.store.deep_resolve_alias(ty);
        let Some(name) = resolved.get_qualified_id() else {
            return false;
        };
        matches!(
            self.store.get_definition(name).map(|d| &d.body),
            Some(DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. })
        )
    }

    /// Whether the `==` diagnostic may suggest `#[equality]` for this struct or enum.
    fn is_equality_derivable(&self, ty: &Type) -> bool {
        let resolved = self.store.deep_resolve_alias(ty);
        let Some(name) = resolved.get_qualified_id() else {
            return false;
        };
        let Some(definition) = self.store.get_definition(name) else {
            return false;
        };
        let type_args = resolved.get_type_params().unwrap_or_default();
        let (generics, field_types): (&[syntax::ast::Generic], Vec<Type>) = match &definition.body {
            DefinitionBody::Struct {
                kind: syntax::ast::StructKind::Tuple,
                ..
            } => return false,
            DefinitionBody::Struct {
                generics, fields, ..
            } => (generics, fields.iter().map(|f| f.ty.clone()).collect()),
            DefinitionBody::Enum {
                generics, variants, ..
            } => (
                generics,
                variants
                    .iter()
                    .flat_map(|v| v.fields.iter().map(|f| f.ty.clone()))
                    .collect(),
            ),
            _ => return false,
        };
        let sub_map = generics
            .iter()
            .map(|g| g.name.clone())
            .zip(type_args.iter().cloned())
            .collect();
        field_types.iter().all(|field_ty| {
            let substituted = substitute(&field_ty.resolve_in(&self.env), &sub_map);
            check_not_equatable(
                &self.env,
                self.store,
                &substituted,
                self.cursor.module_id.as_str(),
                &|name| {
                    self.parameter_satisfies_bound(
                        name,
                        super::super::unify::BuiltinBound::Comparable,
                    )
                },
            )
            .is_none()
        })
    }

    pub(super) fn gate_container_equals(&mut self, receiver_ty: &Type, span: Span) {
        let receiver = self.store.deep_resolve_alias(receiver_ty);
        if !receiver.is_slice() && !receiver.is_map() {
            return;
        }
        if let Some(reason) = self.not_equatable_reason(&receiver) {
            self.sink
                .push(diagnostics::infer::not_equatable(&receiver, reason, span));
        }
    }

    /// A container's `equals` lowers to a free function, not a Go method, so it cannot satisfy an interface or bound.
    pub(in crate::checker::infer) fn is_native_container(&self, ty: &Type) -> bool {
        let resolved = self.store.deep_resolve_alias(&ty.resolve_in(&self.env));
        resolved.is_slice() || resolved.is_map()
    }

    fn container_equals_element(&self, ty: &Type) -> Option<Type> {
        let resolved = self.store.deep_resolve_alias(&ty.resolve_in(&self.env));
        match resolved.as_compound() {
            Some((CompoundKind::Slice, args)) => args.first().cloned(),
            Some((CompoundKind::Map, args)) => args.get(1).cloned(),
            _ => None,
        }
    }

    /// Gates `Slice.equals(a, b)`, which parses as one identifier (not a dot access) so `infer_dot_access` never sees it.
    pub(super) fn check_native_equals_ufcs(&mut self, callee: &Expression, args: &[Expression]) {
        let Expression::Identifier { value, .. } = callee.unwrap_parens() else {
            return;
        };
        if value.as_str() != "Slice.equals" && value.as_str() != "Map.equals" {
            return;
        }
        let Some(receiver) = args.first() else {
            return;
        };
        let receiver_ty = receiver.get_type().resolve_in(&self.env).strip_refs();
        self.gate_container_equals(&receiver_ty, receiver.get_span());
    }
}
