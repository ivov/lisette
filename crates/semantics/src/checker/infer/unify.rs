use crate::checker::EnvResolve;
use crate::store::Store;
use Type::{Function, Nominal};
use diagnostics::LisetteDiagnostic;
use syntax::ast::Span;
use syntax::types::{Bound, Type, TypeVarId};

use super::super::TaskState;
use crate::checker::type_env::VarState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinBound {
    Ordered,
    Comparable,
}

impl BuiltinBound {
    pub(crate) fn from_qualified_id(qualified: &str) -> Option<Self> {
        match qualified {
            "go:cmp.Ordered" | "prelude.Ordered" => Some(Self::Ordered),
            "prelude.Comparable" => Some(Self::Comparable),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ordered => "cmp.Ordered",
            Self::Comparable => "Comparable",
        }
    }

    /// True when a parameter declared `T: self` satisfies a callee that
    /// requires `T: target`. Encodes Go's `cmp.Ordered ⊂ Comparable`.
    pub(crate) fn satisfies(self, target: Self) -> bool {
        self == target || (self == Self::Ordered && target == Self::Comparable)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnifyError {
    TypeMismatch,
    InfiniteType,
    ArityMismatch,
    #[allow(clippy::box_collection)] // Intentional: shrinks Result<(), UnifyError> on hot path
    Multiple(Box<Vec<UnifyError>>),
    AlreadyReported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatched {
    Handled,
    Fallthrough,
}

impl TaskState<'_> {
    /// Make two types equal.
    ///
    /// - For two concrete types, verifies that they match.
    /// - For two variable types, records that the first equals the second.
    /// - For a concrete and a variable type, records that the variable equals the concrete.
    pub(super) fn unify(&mut self, store: &Store, t1: &Type, t2: &Type, span: &Span) {
        if let Err(unify_error) = self.try_unify(store, t1, t2, span) {
            if unify_error == UnifyError::AlreadyReported {
                return;
            }
            let err = self.unification_diagnostic(t1, t2, span, &unify_error);
            self.sink.push(err);
        }
    }

    pub(super) fn try_unify(
        &mut self,
        store: &Store,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let r1 = self.env.shallow_resolve(t1);
        let r2 = self.env.shallow_resolve(t2);
        let r1_is_unknown = r1.is_unknown();
        let r2_is_unknown = r2.is_unknown();

        match (&r1, &r2) {
            _ if r1.is_ignored() || r2.is_ignored() => Ok(()),
            _ if r1.is_receiver_placeholder() || r2.is_receiver_placeholder() => Ok(()),
            _ if self.should_unify_refs(store, &r1, &r2, &r1, &r2) => {
                self.unify_refs(store, &r1, &r2, span)
            }

            (Type::Var { id: i1, .. }, Type::Var { id: i2, .. }) if i1 == i2 => Ok(()),

            _ if r1_is_unknown && r2_is_unknown => Ok(()),

            (Type::Var { id, .. }, _) => self.unify_type_variable(store, *id, &r2, span, false),
            (_, Type::Var { id, .. }) => self.unify_type_variable(store, *id, &r1, span, true),

            _ if r1_is_unknown && self.scopes.is_inside_type_param() => {
                Err(UnifyError::TypeMismatch)
            }
            _ if r1_is_unknown => Ok(()),
            _ if r2_is_unknown => Err(UnifyError::TypeMismatch),

            _ if matches!(r2, Type::Never) => Ok(()),
            _ if matches!(r1, Type::Never) => Err(UnifyError::TypeMismatch),

            _ if matches!(r1, Type::Error) => {
                self.collapse_vars_to_error(store, &r2, span);
                Ok(())
            }
            _ if matches!(r2, Type::Error) => {
                self.collapse_vars_to_error(store, &r1, span);
                Ok(())
            }

            (Type::Parameter(name1), Type::Parameter(name2)) if name1 == name2 => Ok(()),

            (Type::ImportNamespace(m1), Type::ImportNamespace(m2)) if m1 == m2 => Ok(()),

            (Type::Simple(k1), Type::Simple(k2)) if k1 == k2 => Ok(()),

            // Go-level aliases for scalar types: byte <-> uint8, rune <-> int32.
            (Type::Simple(k1), Type::Simple(k2)) if simple_kinds_are_go_aliases(*k1, *k2) => Ok(()),

            // Alias follow-through: `type MyFoo = Foo` stores a Nominal
            // alias with Foo as `underlying_ty`. When the other side is a
            // Simple/Compound, follow the alias to the underlying type.
            (
                Nominal {
                    underlying_ty: Some(underlying),
                    ..
                },
                Type::Simple(_) | Type::Compound { .. },
            ) => {
                let u = underlying.as_ref().clone();
                self.try_unify(store, &u, &r2, span)
            }

            (
                Type::Simple(_) | Type::Compound { .. },
                Nominal {
                    underlying_ty: Some(underlying),
                    ..
                },
            ) => {
                let u = underlying.as_ref().clone();
                self.try_unify(store, &r1, &u, span)
            }

            // Simple/Compound vs Nominal interface: synthesise a nominal
            // equivalent for the native type so interface coercion can check
            // it (e.g. `string` satisfying `fmt.Stringer`).
            (Type::Simple(kind), Nominal { .. }) => {
                let synth = Type::Nominal {
                    id: format!("prelude.{}", kind.leaf_name()).into(),
                    params: vec![],
                    underlying_ty: None,
                };
                self.try_unify(store, &synth, &r2, span)
            }
            (Nominal { .. }, Type::Simple(kind)) => {
                let synth = Type::Nominal {
                    id: format!("prelude.{}", kind.leaf_name()).into(),
                    params: vec![],
                    underlying_ty: None,
                };
                self.try_unify(store, &r1, &synth, span)
            }
            (Type::Compound { kind, args }, Nominal { .. }) => {
                let synth = Type::Nominal {
                    id: format!("prelude.{}", kind.leaf_name()).into(),
                    params: args.clone(),
                    underlying_ty: None,
                };
                self.try_unify(store, &synth, &r2, span)
            }
            (Nominal { .. }, Type::Compound { kind, args }) => {
                let synth = Type::Nominal {
                    id: format!("prelude.{}", kind.leaf_name()).into(),
                    params: args.clone(),
                    underlying_ty: None,
                };
                self.try_unify(store, &r1, &synth, span)
            }

            (Type::Compound { kind: k1, args: a1 }, Type::Compound { kind: k2, args: a2 })
                if k1 == k2 && a1.len() == a2.len() =>
            {
                // Compound type arguments are invariant (same rule as generic
                // user types). Track depth so that interface coercion is
                // rejected inside generic positions.
                let a1 = a1.clone();
                let a2 = a2.clone();
                self.scopes.increment_type_param_depth();
                let result = self.unify_pairs(store, a1.iter().zip(a2.iter()), span);
                self.scopes.decrement_type_param_depth();
                result
            }

            (Nominal { .. }, Nominal { .. }) => self.unify_constructors(store, &r1, &r2, span),

            (Function { .. }, Function { .. }) => self.unify_functions(store, &r1, &r2, span),

            (Type::Tuple(elems1), Type::Tuple(elems2)) => {
                if elems1.len() != elems2.len() {
                    return Err(UnifyError::ArityMismatch);
                }
                let elems1 = elems1.clone();
                let elems2 = elems2.clone();
                self.unify_pairs(store, elems1.iter().zip(elems2.iter()), span)
            }

            (
                Nominal {
                    underlying_ty: Some(underlying),
                    ..
                },
                Function { .. },
            ) => {
                let u = underlying.as_ref().clone();
                self.try_unify(store, &u, &r2, span)
            }

            (
                Function { .. },
                Nominal {
                    underlying_ty: Some(underlying),
                    ..
                },
            ) => {
                let u = underlying.as_ref().clone();
                self.try_unify(store, &r1, &u, span)
            }

            _ => Err(UnifyError::TypeMismatch),
        }
    }

    fn should_unify_refs(&self, store: &Store, t1: &Type, t2: &Type, r1: &Type, r2: &Type) -> bool {
        let either_is_ref = t1.is_ref() || t2.is_ref();
        let both_concrete = !t1.is_variable() && !t2.is_variable();
        let neither_is_interface = !self.is_interface(store, t1) && !self.is_interface(store, t2);
        let neither_is_unknown = !r1.is_unknown() && !r2.is_unknown();
        let neither_is_error = !r1.is_error() && !r2.is_error();

        either_is_ref
            && both_concrete
            && neither_is_interface
            && neither_is_unknown
            && neither_is_error
    }

    fn is_interface(&self, store: &Store, ty: &Type) -> bool {
        if let Type::Nominal { id, .. } = ty {
            store.get_interface(id).is_some()
        } else {
            false
        }
    }

    fn unify_refs(
        &mut self,
        store: &Store,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        match (t1.is_ref(), t2.is_ref()) {
            (true, true) => self.try_unify(store, &t1.strip_refs(), &t2.strip_refs(), span),
            (true, false) | (false, true) => Err(UnifyError::TypeMismatch),
            (false, false) => unreachable!("unify_refs called without refs"),
        }
    }

    fn collapse_vars_to_error(&mut self, store: &Store, ty: &Type, span: &Span) {
        let resolved = self.env.shallow_resolve(ty);
        match resolved {
            Type::Var { id, .. } if !id.is_reserved() => {
                let _ = self.unify_type_variable(store, id, &Type::Error, span, false);
            }
            Type::Nominal { params, .. } => {
                for p in params {
                    self.collapse_vars_to_error(store, &p, span);
                }
            }
            Function {
                params,
                return_type,
                ..
            } => {
                for p in params {
                    self.collapse_vars_to_error(store, &p, span);
                }
                self.collapse_vars_to_error(store, &return_type, span);
            }
            Type::Tuple(elems) => {
                for e in elems {
                    self.collapse_vars_to_error(store, &e, span);
                }
            }
            Type::Compound { args, .. } => {
                for a in args {
                    self.collapse_vars_to_error(store, &a, span);
                }
            }
            Type::Forall { body, .. } => {
                self.collapse_vars_to_error(store, &body, span);
            }
            _ => {}
        }
    }

    fn unify_type_variable(
        &mut self,
        store: &Store,
        id: TypeVarId,
        other_ty: &Type,
        span: &Span,
        var_on_right: bool,
    ) -> Result<(), UnifyError> {
        // Reserved sentinel ids (ignored/uninferred) unify silently with
        // anything. Their binding doesn't go into the env.
        if id.is_reserved() {
            return Ok(());
        }
        match self.env.state(id).clone() {
            VarState::Bound(ty) => {
                if var_on_right {
                    self.try_unify(store, other_ty, &ty, span)
                } else {
                    self.try_unify(store, &ty, other_ty, span)
                }
            }
            VarState::Unbound { .. } => {
                if self.env.occurs(id, other_ty) {
                    return Err(UnifyError::InfiniteType);
                }
                self.env.bind(id, other_ty.clone());
                Ok(())
            }
        }
    }

    fn unify_constructors(
        &mut self,
        store: &Store,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let (
            Nominal {
                id: symbol1,
                params: params1,
                ..
            },
            Nominal {
                id: symbol2,
                params: params2,
                ..
            },
        ) = (t1, t2)
        else {
            unreachable!("unify_constructors called with non-Constructor types")
        };

        if symbol1 != symbol2 {
            if let Nominal {
                underlying_ty: Some(u),
                ..
            } = t1
                && store.get_interface(symbol2).is_none()
                && self.try_unify(store, u, t2, span).is_ok()
            {
                return Ok(());
            }
            if let Nominal {
                underlying_ty: Some(u),
                ..
            } = t2
                && store.get_interface(symbol1).is_none()
                && self.try_unify(store, t1, u, span).is_ok()
            {
                return Ok(());
            }
            return self.try_coerce_or_satisfy_interface(store, t1, t2, span);
        }

        if params1.len() != params2.len() {
            return Err(UnifyError::TypeMismatch);
        }

        // Generics are invariant — Box<Dog> is not Box<Animal>
        // even if Dog satisfies Animal. Track depth so we reject
        // interface coercion inside generic type params. All generic types
        // are treated uniformly, including prelude types (Option, Result,
        // Slice, Map, Ref).
        //
        // Bail on the first error rather than collecting via `unify_pairs`:
        // continuing past a failed pair would bind subsequent type variables
        // and erase their original names from the diagnostic.
        self.scopes.increment_type_param_depth();
        let mut result = Ok(());
        for (p1, p2) in params1.iter().zip(params2) {
            if let Err(e) = self.try_unify(store, p1, p2, span) {
                result = Err(e);
                break;
            }
        }
        self.scopes.decrement_type_param_depth();
        result
    }

    fn try_coerce_or_satisfy_interface(
        &mut self,
        store: &Store,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let (
            Nominal {
                id: symbol1,
                params: params1,
                ..
            },
            Nominal {
                id: symbol2,
                params: params2,
                ..
            },
        ) = (t1, t2)
        else {
            unreachable!("try_coerce_or_satisfy_interface called with non-Constructor types")
        };

        if are_go_type_aliases(symbol1, symbol2) {
            return Ok(());
        }

        if self.scopes.is_inside_type_param() {
            return Err(UnifyError::TypeMismatch);
        }

        // Allow Option<T> where a Go interface is expected: unwrap and unify
        // the inner type with the interface directly.
        if symbol1 == "prelude.Option"
            && params1.len() == 1
            && symbol2.starts_with("go:")
            && store.get_interface(symbol2).is_some()
        {
            return self.try_unify(store, &params1[0], t2, span);
        }
        if symbol2 == "prelude.Option"
            && params2.len() == 1
            && symbol1.starts_with("go:")
            && store.get_interface(symbol1).is_some()
        {
            return self.try_unify(store, &params2[0], t1, span);
        }

        if let Some(interface) = store.get_interface(symbol1).cloned() {
            return self
                .satisfies_interface(store, t2, &interface, params1, span)
                .and_then(|()| self.check_pointer_receivers(store, t2, &interface, span))
                .map_err(|_| UnifyError::AlreadyReported);
        }

        if let Some(interface) = store.get_interface(symbol2).cloned() {
            return self
                .satisfies_interface(store, t1, &interface, params2, span)
                .and_then(|()| self.check_pointer_receivers(store, t1, &interface, span))
                .map_err(|_| UnifyError::AlreadyReported);
        }

        Err(UnifyError::TypeMismatch)
    }

    fn unify_pairs<'a>(
        &mut self,
        store: &Store,
        pairs: impl Iterator<Item = (&'a Type, &'a Type)>,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let mut errors = Vec::new();

        for (t1, t2) in pairs {
            if let Err(e) = self.try_unify(store, t1, t2, span) {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            Err(errors
                .into_iter()
                .next()
                .expect("single-element vec has first element"))
        } else {
            Err(UnifyError::Multiple(Box::new(errors)))
        }
    }

    fn unify_functions(
        &mut self,
        store: &Store,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let (
            Function {
                params: params1,
                return_type: return_type1,
                bounds: bounds1,
                param_mutability: mut1,
                ..
            },
            Function {
                params: params2,
                return_type: return_type2,
                bounds: bounds2,
                param_mutability: mut2,
                ..
            },
        ) = (t1, t2)
        else {
            unreachable!("unify_functions called with non-Function types")
        };

        if params1.len() != params2.len() {
            return Err(UnifyError::ArityMismatch);
        }

        // A function with `mut` params cannot unify with one without (or vice versa),
        // since that would let callers bypass the `let mut` requirement.
        if mut1 != mut2 {
            return Err(UnifyError::TypeMismatch);
        }

        let params_result = self.unify_pairs(store, params1.iter().zip(params2), span);
        let return_type_result = self.try_unify(store, return_type1, return_type2, span);

        for bound in bounds1.iter().chain(bounds2.iter()) {
            self.check_function_bound(store, bound, span);
        }

        if !self.bounds_equivalent(bounds1, bounds2) {
            return Err(UnifyError::TypeMismatch);
        }

        match (params_result, return_type_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e1), Ok(())) => Err(e1),
            (Ok(()), Err(e2)) => Err(e2),
            (Err(e1), Err(e2)) => Err(UnifyError::Multiple(Box::new(vec![e1, e2]))),
        }
    }

    fn bounds_equivalent(&self, bounds1: &[Bound], bounds2: &[Bound]) -> bool {
        // When one side has no bounds (concrete function type) and the other
        // has bounds whose generics are all resolved to concrete types, the
        // bounds are satisfied by instantiation.
        let all_resolved = |bounds: &[Bound]| {
            bounds
                .iter()
                .all(|b| !b.generic.resolve_in(&self.env).is_variable())
        };

        if bounds1.is_empty() && all_resolved(bounds2) {
            return true;
        }
        if bounds2.is_empty() && all_resolved(bounds1) {
            return true;
        }

        if bounds1.len() != bounds2.len() {
            return false;
        }

        let matches = |a: &Bound, b: &Bound| {
            a.generic.resolve_in(&self.env) == b.generic.resolve_in(&self.env)
                && a.ty.resolve_in(&self.env) == b.ty.resolve_in(&self.env)
        };

        let all_in = |source: &[Bound], target: &[Bound]| {
            source.iter().all(|s| target.iter().any(|t| matches(s, t)))
        };

        all_in(bounds1, bounds2) && all_in(bounds2, bounds1)
    }

    fn check_function_bound(&mut self, store: &Store, bound: &Bound, span: &Span) {
        let resolved_ty = bound.generic.resolve_in(&self.env);

        if resolved_ty.is_variable() {
            return;
        }

        if self.dispatch_builtin_bound(store, bound, &resolved_ty, span) == Dispatched::Handled {
            return;
        }

        let interface_ty = bound.ty.resolve_in(&self.env);
        let Type::Nominal { id, params, .. } = interface_ty else {
            return;
        };

        let Some(interface) = store.get_interface(&id).cloned() else {
            return;
        };

        let _ = self.satisfies_interface(store, &resolved_ty, &interface, &params, span);
    }

    /// Built-in bound recognition; falls through to the interface path on miss.
    pub(super) fn dispatch_builtin_bound(
        &mut self,
        store: &Store,
        bound: &Bound,
        resolved_generic: &Type,
        span: &Span,
    ) -> Dispatched {
        let bound_ty = bound.ty.resolve_in(&self.env);
        let Some(builtin) = bound_ty
            .get_qualified_id()
            .and_then(BuiltinBound::from_qualified_id)
        else {
            return Dispatched::Fallthrough;
        };

        if let Type::Parameter(param_name) = resolved_generic {
            if !self.parameter_satisfies_bound(param_name, builtin) {
                self.sink.push(diagnostics::infer::missing_bound_on_param(
                    param_name,
                    builtin.label(),
                    *span,
                ));
            }
            return Dispatched::Handled;
        }

        match builtin {
            BuiltinBound::Ordered if !resolved_generic.satisfies_ordered_constraint() => {
                self.sink
                    .push(diagnostics::infer::not_orderable_bound(*span));
            }
            BuiltinBound::Comparable => {
                if super::expressions::operators::check_not_comparable(
                    &self.env,
                    store,
                    resolved_generic,
                )
                .is_some()
                {
                    self.sink
                        .push(diagnostics::infer::not_comparable_bound(*span));
                }
            }
            BuiltinBound::Ordered => {}
        }
        Dispatched::Handled
    }

    pub(super) fn parameter_satisfies_bound(&self, param_name: &str, target: BuiltinBound) -> bool {
        let mut found = false;
        self.scopes.for_each_bound_on_param(param_name, |bound_ty| {
            if found {
                return;
            }
            if let Some(declared) = bound_ty
                .resolve_in(&self.env)
                .get_qualified_id()
                .and_then(BuiltinBound::from_qualified_id)
                && declared.satisfies(target)
            {
                found = true;
            }
        });
        found
    }

    fn unification_diagnostic(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: &Span,
        error: &UnifyError,
    ) -> LisetteDiagnostic {
        let t1_normalized = t1.resolve_in(&self.env);
        let t2_normalized = t2.resolve_in(&self.env);
        let (types, _) = Type::remove_vars(&[&t1_normalized, &t2_normalized]);
        let expected = &types[0];
        let actual = &types[1];

        match error {
            UnifyError::InfiniteType => LisetteDiagnostic::error("Infinite type")
                .with_infer_code("infinite_type")
                .with_span_label(span, "infinite type detected here"),

            UnifyError::ArityMismatch => {
                if let (Some(expected_arity), Some(actual_arity)) =
                    (expected.tuple_arity(), actual.tuple_arity())
                {
                    return LisetteDiagnostic::error("Tuple arity mismatch")
                        .with_infer_code("tuple_element_count_mismatch")
                        .with_span_label(
                            span,
                            format!(
                                "expected {} elements, found {} elements",
                                expected_arity, actual_arity
                            ),
                        )
                        .with_help(
                            "Adjust the pattern to match the number of elements in the tuple.",
                        );
                }

                LisetteDiagnostic::error("Type mismatch")
                    .with_infer_code("type_mismatch")
                    .with_span_label(span, format!("expected `{}`, found `{}`", expected, actual))
                    .with_help("The function types must have the same number of parameters")
            }

            UnifyError::TypeMismatch | UnifyError::Multiple(_) => {
                let help = self.help(expected, actual);

                LisetteDiagnostic::error("Type mismatch")
                    .with_infer_code("type_mismatch")
                    .with_span_label(span, format!("expected `{}`, found `{}`", expected, actual))
                    .with_help(help)
            }

            UnifyError::AlreadyReported => {
                unreachable!("AlreadyReported should be filtered before creating diagnostic")
            }
        }
    }

    fn help(&self, expected: &Type, actual: &Type) -> String {
        if actual.is_unknown() {
            return format!(
                "The `Unknown` type cannot be used directly. Use `assert_type` to narrow it down to a concrete type. Example: `let value = assert_type<{}>(...)?`",
                expected
            );
        }

        if expected.is_unknown() {
            return format!(
                "The `Unknown` type cannot be used directly. Use `assert_type` to narrow it down to a concrete type.  Example: `let value = assert_type<{}>(...)?`",
                actual
            );
        }

        if expected.wraps("Ref", actual) {
            return "Add `&` to create a reference".to_string();
        }

        if actual.wraps("Ref", expected) {
            return "Dereference with `*`".to_string();
        }

        if expected.wraps("Option", actual) {
            return "Wrap the value: `Some(...)`".to_string();
        }

        if actual.wraps("Option", expected) {
            return "Unwrap the inner value with `?` or using `match`".to_string();
        }

        if expected.wraps("Result", actual) {
            return "Wrap the value: `Ok(...)`".to_string();
        }

        if actual.wraps("Result", expected) {
            return "Unwrap the inner value with `?` or using `match`".to_string();
        }

        if actual.wraps("Slice", expected) {
            return "Index into the slice, e.g. `items[0]`".to_string();
        }

        if expected.wraps("Slice", actual) {
            return "Wrap the value in a slice literal".to_string();
        }

        if expected.contains_unknown() && !actual.contains_unknown() {
            use syntax::types::CompoundKind::{Map, Slice};
            return match expected.as_compound() {
                Some((Map, args)) if args.get(1).is_some_and(|v| v.resolves_to_unknown()) => {
                    format!(
                        "Build the map with `Map.new()` plus indexed assignment: `let mut m: {} = Map.new(); m[k] = v`",
                        expected
                    )
                }
                Some((Slice, args)) if args.first().is_some_and(|v| v.resolves_to_unknown()) => {
                    format!("Annotate the slice literal: `let xs: {} = [v1, v2, ...]`", expected)
                }
                _ => "The expected type contains `Unknown`. Produce the value in a context where the expected type provides the `Unknown` slot (annotation, parameter type, struct field, or return position).".to_string(),
            };
        }

        if expected.is_numeric_compatible_with(actual) {
            return format!("Cast with `as`, e.g. `value as {}`", expected);
        }

        format!(
            "Change the type annotation to `{}` or convert the value to `{}`",
            actual, expected
        )
    }
}

fn are_go_type_aliases(a: &str, b: &str) -> bool {
    matches!(
        (a, b),
        ("prelude.byte", "prelude.uint8")
            | ("prelude.uint8", "prelude.byte")
            | ("prelude.rune", "prelude.int32")
            | ("prelude.int32", "prelude.rune")
    )
}

/// Go-level aliases between scalar builtins: `byte` is an alias for `uint8`,
/// and `rune` is an alias for `int32`.
fn simple_kinds_are_go_aliases(a: syntax::types::SimpleKind, b: syntax::types::SimpleKind) -> bool {
    use syntax::types::SimpleKind;
    matches!(
        (a, b),
        (SimpleKind::Byte, SimpleKind::Uint8)
            | (SimpleKind::Uint8, SimpleKind::Byte)
            | (SimpleKind::Rune, SimpleKind::Int32)
            | (SimpleKind::Int32, SimpleKind::Rune)
    )
}
