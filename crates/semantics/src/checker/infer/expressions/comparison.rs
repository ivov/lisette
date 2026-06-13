use crate::checker::EnvResolve;
use crate::checker::TypeEnv;
use crate::checker::infer::InferCtx;
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, Type, substitute};

pub(crate) fn check_not_comparable(
    env: &TypeEnv,
    store: &Store,
    ty: &Type,
) -> Option<&'static str> {
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
        } else {
            self.sink
                .push(diagnostics::infer::not_comparable(&resolved, reason, *span));
        }
        false
    }

    pub(super) fn not_equatable_reason(&self, ty: &Type) -> Option<&'static str> {
        let resolved = self.store.deep_resolve_alias(&ty.resolve_in(&self.env));

        if let Type::Parameter(name) = &resolved
            && self.parameter_satisfies_bound(name, super::super::unify::BuiltinBound::Comparable)
        {
            return None;
        }
        // `?` returns `None` (equatable) when the type is comparable.
        let reason = check_not_comparable(&self.env, self.store, &resolved)?;

        match resolved.as_compound() {
            Some((CompoundKind::Slice, args)) => self.not_equatable_reason(args.first()?),
            Some((CompoundKind::Map, args)) => self.not_equatable_reason(args.get(1)?),
            _ => Some(reason),
        }
    }

    pub(super) fn gate_container_equals(&mut self, receiver_ty: &Type, span: Span) {
        let receiver = self.store.deep_resolve_alias(receiver_ty);
        let element = match receiver.as_compound() {
            Some((CompoundKind::Slice, args)) => args.first().cloned(),
            Some((CompoundKind::Map, args)) => args.get(1).cloned(),
            _ => return,
        };
        if let Some(element) = element
            && let Some(reason) = self.not_equatable_reason(&element)
        {
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
