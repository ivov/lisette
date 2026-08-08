use crate::checker::EnvResolve;
use Type::{Function, Nominal};
use diagnostics::LisetteDiagnostic;
use syntax::ast::Span;
use syntax::types::{Bound, CompoundKind, Type, TypeVarId};

use crate::checker::infer::InferCtx;
use crate::checker::infer::carry_mut::can_carry_mutation_across_fn_boundary;
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
    #[expect(clippy::box_collection)] // Intentional: shrinks Result<(), UnifyError> on hot path
    Multiple(Box<Vec<UnifyError>>),
    AlreadyReported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatched {
    Handled,
    Fallthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClosureAdapter {
    Widens,
    Narrows,
}

/// A built-in container that holds values which can be widened into an interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WideningContainer {
    Slice,
    Option,
    Result,
    Partial,
    Map,
}

/// What a container has to widen, and how.
#[derive(Debug, Clone, Copy)]
enum Widening {
    /// `map` over every element.
    Elements,
    /// `map` over the single held value.
    Value,
    /// `map_err` over the held error.
    Error,
    /// No method does it, so the entries are copied over by hand.
    Entries,
}

impl WideningContainer {
    fn of(ty: &Type) -> Option<Self> {
        if ty.is_slice() {
            Some(Self::Slice)
        } else if ty.is_option() {
            Some(Self::Option)
        } else if ty.is_result() {
            Some(Self::Result)
        } else if ty.is_partial() {
            Some(Self::Partial)
        } else if ty.is_map() {
            Some(Self::Map)
        } else {
            None
        }
    }

    /// How the type argument at `position` is widened, if it can be.
    fn widening(self, position: usize) -> Option<Widening> {
        match self {
            Self::Slice => Some(Widening::Elements),
            Self::Option => Some(Widening::Value),
            Self::Result | Self::Partial if position == 0 => Some(Widening::Value),
            Self::Result | Self::Partial => Some(Widening::Error),
            Self::Map if position == 1 => Some(Widening::Entries),
            Self::Map => None,
        }
    }
}

impl InferCtx<'_> {
    /// Make two types equal. Returns `false` when they do not match.
    ///
    /// - For two concrete types, verifies that they match.
    /// - For two variable types, records that the first equals the second.
    /// - For a concrete and a variable type, records that the variable equals the concrete.
    pub(super) fn unify(&mut self, t1: &Type, t2: &Type, span: &Span) -> bool {
        match self.try_unify(t1, t2, span) {
            Ok(()) => true,
            Err(UnifyError::AlreadyReported) => false,
            Err(unify_error) => {
                let err = self.unification_diagnostic(t1, t2, span, &unify_error);
                self.sink.push(err);
                false
            }
        }
    }

    pub(super) fn unify_statement_loop(&mut self, expected_ty: &Type, span: &Span, keyword: &str) {
        let unit_ty = self.type_unit();
        if let Err(unify_error) = self.try_unify(expected_ty, &unit_ty, span) {
            if unify_error == UnifyError::AlreadyReported {
                return;
            }
            let expected = expected_ty.resolve_in(&self.env);
            let (types, _) = Type::remove_vars(&[&expected]);
            self.sink.push(diagnostics::infer::loop_produces_no_value(
                span,
                keyword,
                &types[0].to_string(),
            ));
        }
    }

    pub(super) fn try_unify(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let store = self.store;
        let r1 = self.env.shallow_resolve(t1);
        let r2 = self.env.shallow_resolve(t2);
        let r1_is_unknown = r1.is_unknown();
        let r2_is_unknown = r2.is_unknown();

        match (&r1, &r2) {
            _ if r1.is_ignored() || r2.is_ignored() || r1.is_uninferred() || r2.is_uninferred() => {
                Ok(())
            }
            _ if r1.is_receiver_placeholder() || r2.is_receiver_placeholder() => Ok(()),
            _ if self.should_unify_refs(&r1, &r2) => self.unify_refs(&r1, &r2, span),

            (Type::Var { id: i1, .. }, Type::Var { id: i2, .. }) if i1 == i2 => Ok(()),

            _ if r1_is_unknown && r2_is_unknown => Ok(()),

            (Type::Var { id, .. }, _) => self.unify_type_variable(*id, &r2, span, false),
            (_, Type::Var { id, .. }) => self.unify_type_variable(*id, &r1, span, true),

            _ if r1_is_unknown && self.is_inside_invariant_position() => {
                Err(UnifyError::TypeMismatch)
            }
            _ if r1_is_unknown => Ok(()),
            _ if r2_is_unknown => Err(UnifyError::TypeMismatch),

            _ if matches!(r2, Type::Never) => Ok(()),
            _ if matches!(r1, Type::Never) => Err(UnifyError::TypeMismatch),

            _ if matches!(r1, Type::Error) => {
                self.collapse_vars_to_error(&r2, span);
                Ok(())
            }
            _ if matches!(r2, Type::Error) => {
                self.collapse_vars_to_error(&r1, span);
                Ok(())
            }

            (Type::Parameter(name1), Type::Parameter(name2)) if name1 == name2 => Ok(()),

            (Type::ImportNamespace(m1), Type::ImportNamespace(m2)) if m1 == m2 => Ok(()),

            (Type::Simple(k1), Type::Simple(k2)) if k1 == k2 => Ok(()),

            // Go-level aliases for scalar types: byte <-> uint8, rune <-> int32.
            (Type::Simple(k1), Type::Simple(k2)) if simple_kinds_are_go_aliases(*k1, *k2) => Ok(()),

            (Nominal { id, .. }, other)
                if other.is_structural_alias_body()
                    && let Some(u) = store.underlying_type(&r1) =>
            {
                if matches!(other, Type::Simple(_)) && store.is_nominal_defined_type(id.as_str()) {
                    Err(UnifyError::TypeMismatch)
                } else {
                    self.try_unify(&u, &r2, span)
                }
            }

            (other, Nominal { id, .. })
                if other.is_structural_alias_body()
                    && let Some(u) = store.underlying_type(&r2) =>
            {
                if matches!(other, Type::Simple(_)) && store.is_nominal_defined_type(id.as_str()) {
                    Err(UnifyError::TypeMismatch)
                } else {
                    self.try_unify(&r1, &u, span)
                }
            }

            (Type::Compound { kind: k1, args: a1 }, Type::Compound { kind: k2, args: a2 })
                if k1 == k2 && a1.len() == a2.len() =>
            {
                // Compound type arguments are invariant (same rule as generic
                // user types). Track depth so that interface coercion is
                // rejected inside generic positions.
                let a1 = a1.clone();
                let a2 = a2.clone();
                self.in_invariant_position(|this| this.unify_pairs(a1.iter().zip(a2.iter()), span))
            }

            (Nominal { .. }, Nominal { .. }) => self.unify_constructors(&r1, &r2, span),

            (Function(_), Function(_)) => self.unify_functions(&r1, &r2, span),

            (Type::Tuple(elems1), Type::Tuple(elems2)) => {
                if elems1.len() != elems2.len() {
                    return Err(UnifyError::ArityMismatch);
                }
                let elems1 = elems1.clone();
                let elems2 = elems2.clone();
                self.unify_pairs(elems1.iter().zip(elems2.iter()), span)
            }

            (
                Type::Array {
                    length: length1,
                    element: element1,
                },
                Type::Array {
                    length: length2,
                    element: element2,
                },
            ) => {
                if length1 != length2 {
                    return Err(UnifyError::ArityMismatch);
                }
                let element1 = element1.as_ref().clone();
                let element2 = element2.as_ref().clone();
                self.try_unify(&element1, &element2, span)
            }

            // Bridge the size-erased `prelude.Array` method-host self-type to the
            // real `Type::Array` by unifying the element and ignoring the size.
            (Type::Array { element, .. }, Nominal { id, params, .. })
            | (Nominal { id, params, .. }, Type::Array { element, .. })
                if id.as_str() == "prelude.Array" =>
            {
                let element = element.as_ref().clone();
                match params.first().cloned() {
                    Some(nominal_elem) => self.try_unify(&element, &nominal_elem, span),
                    None => Ok(()),
                }
            }

            (
                interface_ty @ Nominal { .. },
                actual @ (Type::Simple(_) | Type::Compound { .. } | Type::Array { .. }),
            ) => self.try_satisfy_interface(actual, interface_ty, span),

            (Nominal { .. }, Function(_)) if let Some(u) = store.underlying_type(&r1) => {
                self.try_unify(&u, &r2, span)
            }

            (Function(_), Nominal { .. }) if let Some(u) = store.underlying_type(&r2) => {
                self.try_unify(&r1, &u, span)
            }

            _ => Err(UnifyError::TypeMismatch),
        }
    }

    fn should_unify_refs(&self, t1: &Type, t2: &Type) -> bool {
        let either_is_ref = t1.is_ref() || t2.is_ref();
        let both_concrete = !t1.is_variable() && !t2.is_variable();
        let neither_is_interface = !self.store.is_interface(t1) && !self.store.is_interface(t2);
        let neither_is_unknown = !t1.is_unknown() && !t2.is_unknown();
        let neither_is_error = !t1.is_error() && !t2.is_error();
        let neither_is_never = !t1.is_never() && !t2.is_never();
        let neither_is_alias = !self.is_transparent_alias(t1) && !self.is_transparent_alias(t2);

        either_is_ref
            && both_concrete
            && neither_is_interface
            && neither_is_unknown
            && neither_is_error
            && neither_is_never
            && neither_is_alias
    }

    fn is_transparent_alias(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        self.store
            .get_definition(id)
            .is_some_and(|definition| definition.is_transparent_type_alias())
    }

    fn unify_refs(&mut self, t1: &Type, t2: &Type, span: &Span) -> Result<(), UnifyError> {
        match (t1.is_ref(), t2.is_ref()) {
            (true, true) => self.try_unify(&t1.strip_refs(), &t2.strip_refs(), span),
            (true, false) | (false, true) => Err(UnifyError::TypeMismatch),
            (false, false) => unreachable!("unify_refs called without refs"),
        }
    }

    fn collapse_vars_to_error(&mut self, ty: &Type, span: &Span) {
        let resolved = self.env.shallow_resolve(ty);
        match resolved {
            Type::Var { id, .. } => {
                let _ = self.unify_type_variable(id, &Type::Error, span, false);
            }
            Type::Nominal { params, .. } => {
                for p in params {
                    self.collapse_vars_to_error(&p, span);
                }
            }
            Function(f) => {
                for p in &f.params {
                    self.collapse_vars_to_error(&p.ty, span);
                }
                self.collapse_vars_to_error(&f.return_type, span);
            }
            Type::Tuple(elems) => {
                for e in elems {
                    self.collapse_vars_to_error(&e, span);
                }
            }
            Type::Compound { args, .. } => {
                for a in args {
                    self.collapse_vars_to_error(&a, span);
                }
            }
            Type::Forall { body, .. } => {
                self.collapse_vars_to_error(&body, span);
            }
            _ => {}
        }
    }

    fn unify_type_variable(
        &mut self,
        id: TypeVarId,
        other_ty: &Type,
        span: &Span,
        var_on_right: bool,
    ) -> Result<(), UnifyError> {
        match self.env.state(id).clone() {
            VarState::Bound(ty) => {
                if var_on_right {
                    self.try_unify(other_ty, &ty, span)
                } else {
                    self.try_unify(&ty, other_ty, span)
                }
            }
            VarState::Unbound => {
                if self.env.occurs(id, other_ty) {
                    return Err(UnifyError::InfiniteType);
                }
                self.env.bind(id, other_ty.clone());
                Ok(())
            }
        }
    }

    fn unify_constructors(&mut self, t1: &Type, t2: &Type, span: &Span) -> Result<(), UnifyError> {
        let store = self.store;
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
            if store.get_interface(symbol2).is_none()
                && !store.is_nominal_defined_type(symbol1.as_str())
                && let Some(underlying) = store.underlying_type(t1)
                && self.try_unify(&underlying, t2, span).is_ok()
            {
                return Ok(());
            }
            if store.get_interface(symbol1).is_none()
                && !store.is_nominal_defined_type(symbol2.as_str())
                && let Some(underlying) = store.underlying_type(t2)
                && self.try_unify(t1, &underlying, span).is_ok()
            {
                return Ok(());
            }
            return self.try_coerce_or_satisfy_interface(t1, t2, span);
        }

        if params1.len() != params2.len() {
            return Err(UnifyError::TypeMismatch);
        }

        // Generics are invariant: Box<Dog> is not Box<Animal>
        // even if Dog satisfies Animal. Track depth so we reject
        // interface coercion inside generic type params. All generic types
        // are treated uniformly, including prelude types (Option, Result,
        // Slice, Map, Ref).
        //
        // Bail on the first error rather than collecting via `unify_pairs`:
        // continuing past a failed pair would bind subsequent type variables
        // and erase their original names from the diagnostic.
        self.in_invariant_position(|this| {
            let mut result = Ok(());
            for (p1, p2) in params1.iter().zip(params2) {
                if let Err(e) = this.try_unify(p1, p2, span) {
                    result = Err(e);
                    break;
                }
            }
            result
        })
    }

    fn try_coerce_or_satisfy_interface(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let store = self.store;
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

        if self.is_inside_invariant_position() {
            return Err(UnifyError::TypeMismatch);
        }

        // Allow Option<T> where a Go interface is expected: unwrap and unify
        // the inner type with the interface directly.
        if symbol1 == "prelude.Option"
            && params1.len() == 1
            && symbol2.starts_with("go:")
            && store.get_interface(symbol2).is_some()
        {
            return self.try_unify(&params1[0], t2, span);
        }
        if symbol2 == "prelude.Option"
            && params2.len() == 1
            && symbol1.starts_with("go:")
            && store.get_interface(symbol1).is_some()
        {
            return self.try_unify(t1, &params2[0], span);
        }

        self.try_satisfy_interface(t2, t1, span)
    }

    fn try_satisfy_interface(
        &mut self,
        actual: &Type,
        interface_ty: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        if self.is_inside_invariant_position() {
            return Err(UnifyError::TypeMismatch);
        }
        if !self.store.is_interface(interface_ty) {
            return Err(UnifyError::TypeMismatch);
        }
        self.satisfies_interface(actual, interface_ty, span)
            .and_then(|()| self.check_pointer_receivers(actual, interface_ty, span))
            .map_err(|_| UnifyError::AlreadyReported)
    }

    fn unify_pairs<'a>(
        &mut self,
        pairs: impl Iterator<Item = (&'a Type, &'a Type)>,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let mut errors = Vec::new();

        for (t1, t2) in pairs {
            if let Err(e) = self.try_unify(t1, t2, span) {
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

    fn unify_functions(&mut self, t1: &Type, t2: &Type, span: &Span) -> Result<(), UnifyError> {
        let (Function(f1), Function(f2)) = (t1, t2) else {
            unreachable!("unify_functions called with non-Function types")
        };

        if f1.params.len() != f2.params.len() {
            return Err(UnifyError::ArityMismatch);
        }

        let (params_result, return_type_result) = self.in_invariant_position(|this| {
            let params_result = this.unify_pairs(
                f1.params
                    .iter()
                    .zip(&f2.params)
                    .map(|(left, right)| (&left.ty, &right.ty)),
                span,
            );
            let return_type_result = this.try_unify(&f1.return_type, &f2.return_type, span);
            (params_result, return_type_result)
        });

        if params_result.is_ok()
            && f1.params.iter().zip(&f2.params).any(|(left, right)| {
                left.mutable != right.mutable
                    && (self.mut_reaches_caller(&left.ty) || self.mut_reaches_caller(&right.ty))
            })
        {
            return Err(UnifyError::TypeMismatch);
        }

        for bound in &f1.bounds {
            self.check_function_bound(bound, &f1.params, span);
        }
        for bound in &f2.bounds {
            self.check_function_bound(bound, &f2.params, span);
        }

        if !self.bounds_equivalent(&f1.bounds, &f2.bounds) {
            return Err(UnifyError::TypeMismatch);
        }

        match (params_result, return_type_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e1), Ok(())) => Err(e1),
            (Ok(()), Err(e2)) => Err(e2),
            (Err(e1), Err(e2)) => Err(UnifyError::Multiple(Box::new(vec![e1, e2]))),
        }
    }

    fn mut_reaches_caller(&self, ty: &Type) -> bool {
        let resolved = self.store.peel_alias(&ty.resolve_in(&self.env));
        can_carry_mutation_across_fn_boundary(&resolved, &self.env, self.store)
            || self.store.is_interface(&resolved)
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

    fn check_function_bound(
        &mut self,
        bound: &Bound,
        signature_params: &[syntax::types::FunctionParameter],
        span: &Span,
    ) {
        let store = self.store;
        let resolved_ty = bound.generic.resolve_in(&self.env);

        if resolved_ty.is_variable() {
            return;
        }

        if self.dispatch_builtin_bound(bound, &resolved_ty, span) == Dispatched::Handled {
            return;
        }

        let interface_ty = bound.ty.resolve_in(&self.env);
        if !store.is_interface(&interface_ty) {
            return;
        }

        if self
            .satisfies_interface(&resolved_ty, &interface_ty, span)
            .is_ok()
            && !self.generic_absorbed_via_ref_param(
                &bound.generic,
                signature_params.iter().map(|param| &param.ty),
            )
        {
            let _ = self.check_pointer_receivers(&resolved_ty, &interface_ty, span);
        }
    }

    /// Built-in bound recognition; falls through to the interface path on miss.
    pub(super) fn dispatch_builtin_bound(
        &mut self,
        bound: &Bound,
        resolved_generic: &Type,
        span: &Span,
    ) -> Dispatched {
        let store = self.store;
        let bound_ty = bound.ty.resolve_in(&self.env);
        let Some(builtin) = bound_ty
            .get_qualified_id()
            .and_then(BuiltinBound::from_qualified_id)
        else {
            return Dispatched::Fallthrough;
        };

        self.check_builtin_bound_argument(store, resolved_generic, builtin, *span, None);
        Dispatched::Handled
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
                if let (Some(expected_len), Some(actual_len)) =
                    (expected.array_len(), actual.array_len())
                {
                    return diagnostics::infer::array_length_mismatch(
                        expected_len,
                        actual_len,
                        *span,
                    );
                }

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

                let (expected_name, actual_name) = Type::stringify_pair(expected, actual);

                LisetteDiagnostic::error("Type mismatch")
                    .with_infer_code("type_mismatch")
                    .with_span_label(
                        span,
                        format!("expected `{}`, found `{}`", expected_name, actual_name),
                    )
                    .with_help("The function types must have the same number of parameters")
            }

            UnifyError::TypeMismatch | UnifyError::Multiple(_) => {
                let (expected_name, actual_name) = Type::stringify_pair(expected, actual);
                let help = self.help(expected, actual, &expected_name, &actual_name);

                LisetteDiagnostic::error("Type mismatch")
                    .with_infer_code("type_mismatch")
                    .with_span_label(
                        span,
                        format!("expected `{}`, found `{}`", expected_name, actual_name),
                    )
                    .with_help(help)
            }

            UnifyError::AlreadyReported => {
                unreachable!("AlreadyReported should be filtered before creating diagnostic")
            }
        }
    }

    fn help(
        &self,
        expected: &Type,
        actual: &Type,
        expected_name: &str,
        actual_name: &str,
    ) -> String {
        if actual.is_unknown() {
            return format!(
                "The `Unknown` type cannot be used directly. Use `assert_type` to narrow it down to a concrete type. Example: `let value = assert_type<{}>(...)?`",
                expected_name
            );
        }

        if expected.is_unknown() {
            return format!(
                "The `Unknown` type cannot be used directly. Use `assert_type` to narrow it down to a concrete type.  Example: `let value = assert_type<{}>(...)?`",
                actual_name
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

        if result_error_help_applies(expected, actual) {
            return "Wrap the value: `Err(...)`".to_string();
        }

        if actual.wraps("Result", expected) {
            return "Unwrap the inner value with `?` or using `match`".to_string();
        }

        if array_to_slice_help_applies(expected, actual) {
            return format!(
                "Call `.to_slice()` to copy the elements into a new slice, or change the receiving type to `{actual_name}`"
            );
        }

        if actual.wraps("Slice", expected) {
            return "Index into the slice, e.g. `items[0]`".to_string();
        }

        if expected.wraps("Slice", actual) {
            return "Wrap the value in a slice literal".to_string();
        }

        if self.store.contains_unknown(expected) && !self.store.contains_unknown(actual) {
            use syntax::types::CompoundKind::{Map, Slice};
            return match expected.as_compound() {
                Some((Map, args))
                    if args
                        .get(1)
                        .is_some_and(|ty| self.store.resolves_to_unknown(ty)) =>
                {
                    format!(
                        "Build the map with `Map.new()` plus indexed assignment: `let mut m: {} = Map.new(); m[k] = v`",
                        expected_name
                    )
                }
                Some((Slice, args))
                    if args
                        .first()
                        .is_some_and(|ty| self.store.resolves_to_unknown(ty)) =>
                {
                    format!("Annotate the slice literal: `let xs: {} = [v1, v2, ...]`", expected_name)
                }
                _ if self.store.resolve_to_function_type(expected).is_some()
                    && self.store.resolve_to_function_type(actual).is_some() =>
                {
                    "Function types must match exactly, and `Unknown` matches only `Unknown`. Declare the function with the expected signature and narrow with `assert_type` inside, or wrap it in a closure that narrows at the call site".to_string()
                }
                _ => format!(
                    "`Unknown` matches only `Unknown`, never a concrete type. Build `{expected_name}` from a value already annotated as `Unknown`, e.g. `let value: Unknown = ...`, or change the expected type to `{actual_name}`"
                ),
            };
        }

        if self.store.is_interface(actual) && !self.store.is_interface(expected) {
            return format!(
                "An interface value does not carry its concrete type. Narrow it with `assert_type`, e.g. `let value = assert_type<{}>(value)?`",
                expected_name
            );
        }

        if self.store.is_numeric_compatible_with(expected, actual) {
            return format!("Cast with `as`, e.g. `value as {}`", expected_name);
        }

        if let Some(Type::Function(function)) = self.store.resolve_to_function_type(expected)
            && function.return_type.as_ref() == actual
        {
            return "Remove the `()` so that the type matches".to_string();
        }

        if differs_only_in_param_mutability(expected, actual) {
            return format!(
                "`mut` belongs to the function type when the parameter carries mutation back to the caller. Match `mut` on each differing parameter, or expect `{actual_name}` instead"
            );
        }

        match self.closure_adapter(expected, actual) {
            Some(ClosureAdapter::Widens) => {
                return "Function types must match exactly. Wrap the value in a closure to convert at the call site, e.g. `|value| callee(value)`".to_string();
            }
            Some(ClosureAdapter::Narrows) => {
                return "Function types must match exactly. Wrap the value in a closure that narrows the interface value with `assert_type`, or change the signature to match".to_string();
            }
            None => {}
        }

        if let Some(widening) = self.container_widening_help(expected, actual, expected_name) {
            return widening;
        }

        format!(
            "Change the type annotation to `{}` or convert the value to `{}`",
            actual_name, expected_name
        )
    }

    fn container_widening_help(
        &self,
        expected: &Type,
        actual: &Type,
        expected_name: &str,
    ) -> Option<String> {
        let container = WideningContainer::of(expected)?;
        if WideningContainer::of(actual) != Some(container) {
            return None;
        }

        let (expected_args, actual_args) = (expected.get_type_params()?, actual.get_type_params()?);
        if expected_args.len() != actual_args.len() {
            return None;
        }

        let mut differing = expected_args
            .iter()
            .zip(actual_args)
            .enumerate()
            .filter(|(_, (expected_arg, actual_arg))| expected_arg != actual_arg);
        let (position, (expected_element, actual_element)) = differing.next()?;
        if differing.next().is_some() {
            return None;
        }

        if !self.store.is_interface(expected_element) || !self.widens_into_interface(actual_element)
        {
            return None;
        }

        let cast = format!("|value| value as {expected_element}");
        Some(match container.widening(position)? {
            Widening::Elements => format!("Use `.map({cast})` to widen each element"),
            Widening::Value => format!("Use `.map({cast})` to widen the value"),
            Widening::Error => format!("Use `.map_err({cast})` to widen the error"),
            Widening::Entries => format!(
                "Copy the entries into a new `{expected_name}`, widening each value: `widened[key] = value as {expected_element}`"
            ),
        })
    }

    fn widens_into_interface(&self, ty: &Type) -> bool {
        !matches!(ty, Type::Parameter(_))
            && !self.store.is_interface(ty)
            && !self.store.resolves_to_unknown(ty)
    }

    fn closure_adapter(&self, expected: &Type, actual: &Type) -> Option<ClosureAdapter> {
        if !matches!((expected, actual), (Function(_), Function(_))) {
            return None;
        }

        let (expected_positions, actual_positions) = (expected.children(), actual.children());
        if expected_positions.len() != actual_positions.len() {
            return None;
        }
        let return_index = expected_positions.len().checked_sub(1)?;

        let mut adapter = None;
        for (index, (left, right)) in expected_positions.iter().zip(&actual_positions).enumerate() {
            if left == right {
                continue;
            }
            if !self.store.is_interface(left) && !self.store.is_interface(right) {
                return None;
            }
            let narrows = if index == return_index {
                self.store.is_interface(right)
            } else {
                self.store.is_interface(left)
            };
            if narrows {
                adapter = Some(ClosureAdapter::Narrows);
            } else if adapter.is_none() {
                adapter = Some(ClosureAdapter::Widens);
            }
        }
        adapter
    }

    /// Whether the emitter absorbs this bounded generic into a pointer type argument
    /// via a top-level `Ref<T>` param (`with_absorbed_ref_generics`), so the pointer
    /// satisfies the interface. Decided from params alone, like the emitter.
    pub(super) fn generic_absorbed_via_ref_param<'a>(
        &self,
        generic: &Type,
        params: impl IntoIterator<Item = &'a Type>,
    ) -> bool {
        let is_absorbed_param = |param: &Type| matches!(param.as_compound(), Some((CompoundKind::Ref, [inner, ..])) if inner == generic);

        let Type::Var { id, .. } = generic else {
            return params.into_iter().any(is_absorbed_param);
        };

        let mut absorbed = false;
        for param in params {
            if is_absorbed_param(param) {
                absorbed = true;
            } else if self.env.occurs(*id, param) {
                return false;
            }
        }
        absorbed
    }
}

fn result_error_help_applies(expected: &Type, actual: &Type) -> bool {
    expected.get_name().is_some_and(|name| name == "Result")
        && expected
            .get_type_params()
            .and_then(|params| params.get(1))
            .is_some_and(|error_type| error_type == actual)
}

fn array_to_slice_help_applies(expected: &Type, actual: &Type) -> bool {
    if !expected.is_slice() {
        return false;
    }
    let Type::Array { element, .. } = actual else {
        return false;
    };
    let Some(expected_element) = expected.get_type_params().and_then(|params| params.first())
    else {
        return false;
    };
    expected_element == element.as_ref()
}

/// Whether two function types are the same but for `mut` on one or more parameters.
fn differs_only_in_param_mutability(expected: &Type, actual: &Type) -> bool {
    let (Function(expected), Function(actual)) = (expected, actual) else {
        return false;
    };
    if expected.params.len() != actual.params.len()
        || expected.return_type != actual.return_type
        || expected.bounds != actual.bounds
    {
        return false;
    }
    let pairs = || expected.params.iter().zip(&actual.params);
    pairs().all(|(left, right)| left.ty == right.ty)
        && pairs().any(|(left, right)| left.mutable != right.mutable)
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
