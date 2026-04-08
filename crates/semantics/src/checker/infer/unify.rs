use std::cell::RefCell;
use std::rc::Rc;

use Type::{Constructor, Function, Variable};
use diagnostics::LisetteDiagnostic;
use syntax::ast::Span;
use syntax::types::{Bound, Type, TypeVariableState};

use super::super::Checker;

#[derive(Debug, Clone, PartialEq)]
pub enum UnifyError {
    TypeMismatch,
    InfiniteType,
    ArityMismatch,
    #[allow(clippy::box_collection)] // Intentional: shrinks Result<(), UnifyError> on hot path
    Multiple(Box<Vec<UnifyError>>),
    AlreadyReported,
}

impl Checker<'_, '_> {
    /// Make two types equal.
    ///
    /// - For two concrete types, verifies that they match.
    /// - For two variable types, records that the first equals the second.
    /// - For a concrete and a variable type, records that the variable equals the concrete.
    pub(super) fn unify(&mut self, t1: &Type, t2: &Type, span: &Span) {
        if let Err(unify_error) = self.try_unify(t1, t2, span) {
            if unify_error == UnifyError::AlreadyReported {
                return;
            }
            let err = self.unification_diagnostic(t1, t2, span, &unify_error);
            self.sink.push(err);
        }
    }

    pub(super) fn try_unify(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let r1 = t1.shallow_resolve();
        let r2 = t2.shallow_resolve();

        match (t1, t2) {
            _ if t1.is_ignored() || t2.is_ignored() => Ok(()),
            _ if t1.is_receiver_placeholder() || t2.is_receiver_placeholder() => Ok(()),
            _ if self.should_unify_refs(t1, t2, &r1, &r2) => self.unify_refs(t1, t2, span),

            (Variable(v1), Variable(v2)) if Rc::ptr_eq(v1, v2) => Ok(()),

            _ if r1.is_unknown() => Ok(()),
            _ if r2.is_unknown() && !t1.is_variable() => Err(UnifyError::TypeMismatch),

            _ if matches!(r2, Type::Never) => Ok(()),
            _ if matches!(r1, Type::Never) => Err(UnifyError::TypeMismatch),

            (Variable(type_var), other) => self.unify_type_variable(type_var, other, span, false),
            (other, Variable(type_var)) => self.unify_type_variable(type_var, other, span, true),

            // Error after Variable: variables absorb Error via linking above;
            // non-variable vs Error succeeds silently
            _ if matches!(r1, Type::Error) || matches!(r2, Type::Error) => Ok(()),

            (Type::Parameter(name1), Type::Parameter(name2)) if name1 == name2 => Ok(()),

            (Constructor { .. }, Constructor { .. }) => self.unify_constructors(t1, t2, span),

            (Function { .. }, Function { .. }) => self.unify_functions(t1, t2, span),

            (Type::Tuple(elems1), Type::Tuple(elems2)) => {
                if elems1.len() != elems2.len() {
                    return Err(UnifyError::ArityMismatch);
                }
                self.unify_pairs(elems1.iter().zip(elems2), span)
            }

            (
                Constructor {
                    underlying_ty: Some(underlying),
                    ..
                },
                Function { .. },
            ) => self.try_unify(underlying.as_ref(), t2, span),

            (
                Function { .. },
                Constructor {
                    underlying_ty: Some(underlying),
                    ..
                },
            ) => self.try_unify(t1, underlying.as_ref(), span),

            _ => Err(UnifyError::TypeMismatch),
        }
    }

    fn should_unify_refs(&self, t1: &Type, t2: &Type, r1: &Type, r2: &Type) -> bool {
        let either_is_ref = t1.is_ref() || t2.is_ref();
        let both_concrete = !t1.is_variable() && !t2.is_variable();
        let neither_is_interface = !self.is_interface(t1) && !self.is_interface(t2);
        let neither_is_unknown = !r1.is_unknown() && !r2.is_unknown();

        either_is_ref && both_concrete && neither_is_interface && neither_is_unknown
    }

    fn is_interface(&self, ty: &Type) -> bool {
        if let Type::Constructor { id, .. } = ty {
            self.store.get_interface(id).is_some()
        } else {
            false
        }
    }

    fn unify_refs(&mut self, t1: &Type, t2: &Type, span: &Span) -> Result<(), UnifyError> {
        match (t1.is_ref(), t2.is_ref()) {
            (true, true) => self.try_unify(&t1.strip_refs(), &t2.strip_refs(), span),
            (true, false) | (false, true) => Err(UnifyError::TypeMismatch),
            (false, false) => unreachable!("unify_refs called without refs"),
        }
    }

    fn unify_type_variable(
        &mut self,
        type_var: &Rc<RefCell<TypeVariableState>>,
        other_ty: &Type,
        span: &Span,
        var_on_right: bool,
    ) -> Result<(), UnifyError> {
        let state = type_var.borrow();
        match &*state {
            TypeVariableState::Link(ty) => {
                let ty = ty.clone();
                drop(state);
                if var_on_right {
                    self.try_unify(other_ty, &ty, span)
                } else {
                    self.try_unify(&ty, other_ty, span)
                }
            }
            TypeVariableState::Unbound { id, hint } => {
                let id = *id;
                let hint = hint.clone();
                drop(state);
                if self.occurs_in(id, other_ty) {
                    return Err(UnifyError::InfiniteType);
                }

                if let Some(log) = &mut self.inference.undo_log {
                    log.push((Rc::clone(type_var), TypeVariableState::Unbound { id, hint }));
                }
                *type_var.borrow_mut() = TypeVariableState::Link(other_ty.clone());
                Ok(())
            }
        }
    }

    fn unify_constructors(&mut self, t1: &Type, t2: &Type, span: &Span) -> Result<(), UnifyError> {
        let (
            Constructor {
                id: symbol1,
                params: params1,
                ..
            },
            Constructor {
                id: symbol2,
                params: params2,
                ..
            },
        ) = (t1, t2)
        else {
            unreachable!("unify_constructors called with non-Constructor types")
        };

        if symbol1 != symbol2 {
            return self.try_coerce_or_satisfy_interface(t1, t2, span);
        }

        if params1.len() != params2.len() {
            return Err(UnifyError::TypeMismatch);
        }

        // Generics are invariant — Box<Dog> is not Box<Animal>
        // even if Dog satisfies Animal. Track depth so we reject
        // interface coercion inside generic type params. All generic types
        // are treated uniformly, including prelude types (Option, Result,
        // Slice, Map, Ref).
        self.inference.type_param_depth += 1;
        let result = self.unify_type_params(params1.iter().zip(params2), span);
        self.inference.type_param_depth -= 1;
        result
    }

    fn try_coerce_or_satisfy_interface(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: &Span,
    ) -> Result<(), UnifyError> {
        let (
            Constructor {
                id: symbol1,
                params: params1,
                ..
            },
            Constructor {
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

        if self.inference.type_param_depth > 0 {
            return Err(UnifyError::TypeMismatch);
        }

        // Allow Option<T> where a Go interface is expected: unwrap and unify
        // the inner type with the interface directly.
        if symbol1 == "prelude.Option"
            && params1.len() == 1
            && symbol2.starts_with("go:")
            && self.store.get_interface(symbol2).is_some()
        {
            return self.try_unify(&params1[0], t2, span);
        }
        if symbol2 == "prelude.Option"
            && params2.len() == 1
            && symbol1.starts_with("go:")
            && self.store.get_interface(symbol1).is_some()
        {
            return self.try_unify(&params2[0], t1, span);
        }

        if let Some(interface) = self.store.get_interface(symbol1).cloned() {
            return self
                .satisfies_interface(t2, &interface, params1, span)
                .and_then(|()| self.check_pointer_receivers(t2, &interface, span))
                .map_err(|_| UnifyError::AlreadyReported);
        }

        if let Some(interface) = self.store.get_interface(symbol2).cloned() {
            return self
                .satisfies_interface(t1, &interface, params2, span)
                .and_then(|()| self.check_pointer_receivers(t1, &interface, span))
                .map_err(|_| UnifyError::AlreadyReported);
        }

        Err(UnifyError::TypeMismatch)
    }

    fn unify_type_params<'a>(
        &mut self,
        pairs: impl Iterator<Item = (&'a Type, &'a Type)>,
        span: &Span,
    ) -> Result<(), UnifyError> {
        for (t1, t2) in pairs {
            let r1 = t1.shallow_resolve();
            let r2 = t2.shallow_resolve();

            match (t1, t2) {
                _ if t1.is_ignored() || t2.is_ignored() => {}
                _ if t1.is_receiver_placeholder() || t2.is_receiver_placeholder() => {}
                (Variable(v1), Variable(v2)) if Rc::ptr_eq(v1, v2) => {}

                _ if r1.is_unknown() => {}
                _ if r2.is_unknown() && !t1.is_variable() => {
                    return Err(UnifyError::TypeMismatch);
                }

                _ if matches!(r2, Type::Never) => {
                    if let Variable(type_var) = t1
                        && type_var.borrow().is_unbound()
                    {
                        self.unify_type_variable(type_var, &Type::Never, span, false)?;
                    }
                }
                _ if matches!(r1, Type::Never) => {
                    if let Variable(type_var) = t2
                        && type_var.borrow().is_unbound()
                    {
                        self.unify_type_variable(type_var, &Type::Never, span, false)?;
                    } else if !matches!(r2, Type::Never) && !r2.is_variable() {
                        return Err(UnifyError::TypeMismatch);
                    }
                }

                (Variable(type_var), other) | (other, Variable(type_var)) => {
                    self.unify_type_variable(type_var, other, span, false)?;
                }
                (Type::Parameter(name1), Type::Parameter(name2)) if name1 == name2 => {}
                (
                    Constructor {
                        id: id1,
                        params: p1,
                        ..
                    },
                    Constructor {
                        id: id2,
                        params: p2,
                        ..
                    },
                ) if (id1 == id2 || are_go_type_aliases(id1, id2)) && p1.len() == p2.len() => {
                    let is_user_defined = !id1.starts_with("prelude.");
                    if is_user_defined {
                        self.inference.type_param_depth += 1;
                    }
                    let r = self.unify_type_params(p1.iter().zip(p2), span);
                    if is_user_defined {
                        self.inference.type_param_depth -= 1;
                    }
                    r?;
                }
                (Function { .. }, Function { .. }) => {
                    self.unify_functions(t1, t2, span)?;
                }
                (Type::Tuple(e1), Type::Tuple(e2)) if e1.len() == e2.len() => {
                    self.unify_type_params(e1.iter().zip(e2), span)?;
                }
                _ => return Err(UnifyError::TypeMismatch),
            }
        }
        Ok(())
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

        let params_result = self.unify_pairs(params1.iter().zip(params2), span);
        let return_type_result = self.try_unify(return_type1, return_type2, span);

        for bound in bounds1.iter().chain(bounds2.iter()) {
            self.check_function_bound(bound, span);
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
        let all_resolved =
            |bounds: &[Bound]| bounds.iter().all(|b| !b.generic.resolve().is_variable());

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
            a.generic.resolve() == b.generic.resolve() && a.ty.resolve() == b.ty.resolve()
        };

        let all_in = |source: &[Bound], target: &[Bound]| {
            source.iter().all(|s| target.iter().any(|t| matches(s, t)))
        };

        all_in(bounds1, bounds2) && all_in(bounds2, bounds1)
    }

    fn check_function_bound(&mut self, bound: &Bound, span: &Span) {
        let resolved_ty = bound.generic.resolve();

        if resolved_ty.is_variable() {
            return;
        }

        let interface_ty = bound.ty.resolve();
        let Type::Constructor { id, params, .. } = interface_ty else {
            return;
        };

        let Some(interface) = self.store.get_interface(&id).cloned() else {
            return;
        };

        let _ = self.satisfies_interface(&resolved_ty, &interface, &params, span);
    }

    #[allow(clippy::only_used_in_recursion)]
    fn occurs_in(&self, type_var_id: i32, ty: &Type) -> bool {
        match ty {
            Constructor { params, .. } => params.iter().any(|p| self.occurs_in(type_var_id, p)),
            Variable(type_var) => match &*type_var.borrow() {
                TypeVariableState::Unbound { id, .. } => *id == type_var_id,
                TypeVariableState::Link(linked_ty) => self.occurs_in(type_var_id, linked_ty),
            },
            Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| self.occurs_in(type_var_id, p))
                    || self.occurs_in(type_var_id, return_type)
            }
            Type::Forall { body, .. } => self.occurs_in(type_var_id, body),
            Type::Tuple(elements) => elements.iter().any(|e| self.occurs_in(type_var_id, e)),
            Type::Parameter(_) => false,
            Type::Never | Type::Error => false,
        }
    }

    fn unification_diagnostic(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: &Span,
        error: &UnifyError,
    ) -> LisetteDiagnostic {
        let t1_normalized = t1.resolve();
        let t2_normalized = t2.resolve();
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
