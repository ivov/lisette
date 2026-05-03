use crate::Emitter;
use crate::names::go_name::PRELUDE_ERROR_ID;
use syntax::ast::{Annotation, Expression};
use syntax::types::{Type, unqualified_name};

/// Go ABI shape that a Lisette type lowers to at function-boundary
/// positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AbiShape {
    /// `Result<T, error>` → `(T, error)`.
    ResultTuple,
    /// `Result<(), error>` → `error`.
    BareError,
    /// `Partial<T, error>` → `(T, error)`. Same Go shape as `ResultTuple`
    /// but with three-way Ok/Err/Both source variants.
    PartialTuple,
    /// `Option<T>` → `(T, bool)` for non-nilable T (Go's comma-ok idiom).
    CommaOk,
    /// `Option<Ref<T>>` / `Option<fn>` / `Option<Iface>` → bare nilable T.
    NullableReturn,
    /// `Tuple<T1, T2, ...>` → `(T1, T2, ...)`. Arity ≥ 2.
    Tuple { arity: usize },
}

impl Emitter<'_> {
    /// Lowered shape for a Lisette return type, or `None` to keep it tagged.
    pub(crate) fn classify_direct_emission(&self, return_ty: &Type) -> Option<AbiShape> {
        let peeled = self.peel_alias(return_ty);
        if peeled.is_result() && self.err_slot_is_nilable(&peeled) {
            return Some(if peeled.ok_type().is_unit() {
                AbiShape::BareError
            } else {
                AbiShape::ResultTuple
            });
        }
        if peeled.is_partial() && self.err_slot_is_nilable(&peeled) {
            return Some(AbiShape::PartialTuple);
        }
        if peeled.is_option() {
            return Some(if self.is_nullable_option(&peeled) {
                AbiShape::NullableReturn
            } else {
                AbiShape::CommaOk
            });
        }
        if let Some(arity) = peeled.tuple_arity()
            && arity >= 2
        {
            return Some(AbiShape::Tuple { arity });
        }
        None
    }

    /// True when the err slot of a `Result`/`Partial` lowers to a Go
    /// nilable type, so `nil` typechecks as the no-error sentinel.
    fn err_slot_is_nilable(&self, fallible_ty: &Type) -> bool {
        let err = self.peel_alias(&fallible_ty.err_type());
        matches!(&err, Type::Nominal { id, .. } if id.as_str() == PRELUDE_ERROR_ID)
            || self.is_nilable_go_type(&err)
    }

    /// Render the lowered Go return type.
    pub(crate) fn render_lowered_return_ty(
        &mut self,
        shape: &AbiShape,
        return_ty: &Type,
    ) -> String {
        let peeled = self.peel_alias(return_ty);
        match shape {
            AbiShape::BareError => self.go_type_as_string(&peeled.err_type()),
            AbiShape::ResultTuple | AbiShape::PartialTuple => {
                let ok_str = self.go_type_as_string(&peeled.ok_type());
                let err_str = self.go_type_as_string(&peeled.err_type());
                format!("({}, {})", ok_str, err_str)
            }
            AbiShape::CommaOk => {
                let inner_str = self.go_type_as_string(&peeled.ok_type());
                format!("({}, bool)", inner_str)
            }
            AbiShape::NullableReturn => self.go_type_as_string(&peeled.ok_type()),
            AbiShape::Tuple { .. } => {
                let parts: Vec<String> = tuple_element_types(&peeled)
                    .iter()
                    .map(|t| self.tuple_slot_lowered_ty_string(t))
                    .collect();
                format!("({})", parts.join(", "))
            }
        }
    }

    /// Render a tuple slot's Go type, lowering `Option<NilableT>` to bare
    /// nilable `T` (the only arity-preserving slot recursion).
    pub(crate) fn tuple_slot_lowered_ty_string(&mut self, slot_ty: &Type) -> String {
        if self.is_nullable_option(slot_ty) {
            let inner = self.peel_alias(slot_ty).ok_type();
            return self.go_type_as_string(&inner);
        }
        self.go_type_as_string(slot_ty)
    }

    /// `&self` variant of `render_lowered_return_ty`, callable from the
    /// `go_type` recursion which doesn't have `&mut self`.
    pub(crate) fn lowered_return_go_type(
        &self,
        shape: &AbiShape,
        return_ty: &Type,
    ) -> crate::types::go_type::GoType {
        use crate::types::go_type::GoType;
        let peeled = self.peel_alias(return_ty);
        match shape {
            AbiShape::BareError => self.go_type(&peeled.err_type()),
            AbiShape::ResultTuple | AbiShape::PartialTuple => {
                let ok_go = self.go_type(&peeled.ok_type());
                let err_go = self.go_type(&peeled.err_type());
                let mut result = GoType::new(format!("({}, {})", ok_go.code, err_go.code));
                result.merge_from(&ok_go);
                result.merge_from(&err_go);
                result
            }
            AbiShape::CommaOk => {
                let inner_go = self.go_type(&peeled.ok_type());
                let mut result = GoType::new(format!("({}, bool)", inner_go.code));
                result.merge_from(&inner_go);
                result
            }
            AbiShape::NullableReturn => self.go_type(&peeled.ok_type()),
            AbiShape::Tuple { .. } => {
                let elems = tuple_element_types(&peeled);
                let elem_gos: Vec<GoType> = elems
                    .iter()
                    .map(|t| {
                        if self.is_nullable_option(t) {
                            let inner = self.peel_alias(t).ok_type();
                            self.go_type(&inner)
                        } else {
                            self.go_type(t)
                        }
                    })
                    .collect();
                let parts: Vec<&str> = elem_gos.iter().map(|t| t.code.as_str()).collect();
                let mut result = GoType::new(format!("({})", parts.join(", ")));
                for go in &elem_gos {
                    result.merge_from(go);
                }
                result
            }
        }
    }

    /// Annotation-side mirror of `classify_direct_emission`.
    pub(crate) fn classify_annotation_direct_emission(
        &self,
        annotation: &Annotation,
    ) -> Option<AbiShape> {
        if let Annotation::Tuple { elements, .. } = annotation
            && elements.len() >= 2
        {
            return Some(AbiShape::Tuple {
                arity: elements.len(),
            });
        }
        let Annotation::Constructor { name, params, .. } = annotation else {
            return None;
        };
        let leaf = unqualified_name(name);
        match leaf {
            "Result" if params.len() == 2 && annotation_is_go_error(&params[1]) => {
                Some(if params[0].is_unit() {
                    AbiShape::BareError
                } else {
                    AbiShape::ResultTuple
                })
            }
            "Partial" if params.len() == 2 && annotation_is_go_error(&params[1]) => {
                Some(AbiShape::PartialTuple)
            }
            "Option" if params.len() == 1 => {
                Some(if self.annotation_inner_is_nilable(&params[0]) {
                    AbiShape::NullableReturn
                } else {
                    AbiShape::CommaOk
                })
            }
            _ => None,
        }
    }

    /// Annotation-side mirror of `lowered_return_go_type`.
    pub(crate) fn lowered_return_go_type_from_annotation(
        &self,
        shape: &AbiShape,
        return_ann: &Annotation,
    ) -> crate::types::go_type::GoType {
        use crate::types::go_type::GoType;
        let constructor_params = || match return_ann {
            Annotation::Constructor { params, .. } => params,
            _ => unreachable!("Result/Option/Partial imply Constructor annotation"),
        };
        match shape {
            AbiShape::BareError => {
                let params = constructor_params();
                self.go_type_from_annotation(&params[1])
            }
            AbiShape::ResultTuple | AbiShape::PartialTuple => {
                let params = constructor_params();
                let ok_go = self.go_type_from_annotation(&params[0]);
                let err_go = self.go_type_from_annotation(&params[1]);
                let mut result = GoType::new(format!("({}, {})", ok_go.code, err_go.code));
                result.merge_from(&ok_go);
                result.merge_from(&err_go);
                result
            }
            AbiShape::CommaOk => {
                let params = constructor_params();
                let inner_go = self.go_type_from_annotation(&params[0]);
                let mut result = GoType::new(format!("({}, bool)", inner_go.code));
                result.merge_from(&inner_go);
                result
            }
            AbiShape::NullableReturn => {
                let params = constructor_params();
                self.go_type_from_annotation(&params[0])
            }
            AbiShape::Tuple { .. } => {
                let elements = match return_ann {
                    Annotation::Tuple { elements, .. } => elements,
                    _ => unreachable!("Tuple shape implies Tuple annotation"),
                };
                let elem_gos: Vec<GoType> = elements
                    .iter()
                    .map(|a| {
                        if matches!(
                            self.classify_annotation_direct_emission(a),
                            Some(AbiShape::NullableReturn)
                        ) {
                            self.go_type_from_annotation(match a {
                                Annotation::Constructor { params, .. } => &params[0],
                                _ => a,
                            })
                        } else {
                            self.go_type_from_annotation(a)
                        }
                    })
                    .collect();
                let parts: Vec<&str> = elem_gos.iter().map(|g| g.code.as_str()).collect();
                let mut result = GoType::new(format!("({})", parts.join(", ")));
                for go in &elem_gos {
                    result.merge_from(go);
                }
                result
            }
        }
    }

    /// Lowered shape of the enclosing function's return type, if any.
    pub(crate) fn current_lowered_abi(&self) -> Option<AbiShape> {
        let ctx = self.current_return_context.as_ref()?;
        if ctx.force_tagged {
            return None;
        }
        self.classify_direct_emission(&ctx.ty)
    }

    /// Annotation-side mirror of `is_nullable_option`'s inner check.
    pub(crate) fn annotation_inner_is_nilable(&self, annotation: &Annotation) -> bool {
        match annotation {
            Annotation::Function { .. } => true,
            Annotation::Constructor { name, .. } => {
                let leaf = unqualified_name(name);
                if leaf == "Ref" {
                    return true;
                }
                let resolved = self.peel_alias_id(name);
                if let Some(def) = self.ctx.definitions.get(resolved.as_str())
                    && matches!(def.body, syntax::program::DefinitionBody::TypeAlias { .. })
                    && self.resolve_to_function_type(&def.ty).is_some()
                {
                    return true;
                }
                if let Some(def) = self.ctx.definitions.get(resolved.as_str()) {
                    matches!(def.body, syntax::program::DefinitionBody::Interface { .. })
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Prelude fn refs emit with tagged Go return (`Option[T]`); user fns
    /// and lambdas emit with the lowered ABI (`(T, bool)`).
    pub(crate) fn is_tagged_shape_fn_value(expression: &Expression) -> bool {
        let inner = expression.unwrap_parens();
        if inner.as_option_constructor().is_some()
            || inner.as_result_constructor().is_some()
            || inner.as_partial_constructor().is_some()
        {
            return true;
        }
        matches!(
            inner,
            Expression::Identifier { qualified: Some(q), .. } if q.starts_with("prelude.")
        )
    }

    /// Lowered shape of a callee. Type-driven so it fires regardless of
    /// whether the callee is a direct ref, local, parameter, or field.
    pub(crate) fn classify_callee_abi(&self, callee: &Expression) -> Option<AbiShape> {
        let callee_ty = callee.get_type();
        let unwrapped = callee_ty.unwrap_forall();
        let resolved = self
            .resolve_to_function_type(unwrapped)
            .unwrap_or_else(|| unwrapped.clone());
        let Type::Function { return_type, .. } = resolved else {
            return None;
        };
        let inner = callee.unwrap_parens();
        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
        {
            if Self::is_go_receiver(receiver) {
                return None;
            }
            // Methods on native types (`xs.find(f)`) and prelude types
            // (`r.map(f)`, `opt.map(f)`) dispatch to Lisette-prelude
            // helpers whose Go signatures keep the tagged return — no
            // lowering at the call site.
            let receiver_ty = receiver.get_type();
            if crate::types::native::NativeGoType::from_type(&receiver_ty).is_some()
                || receiver_is_prelude_type(&receiver_ty)
            {
                return None;
            }
            // Type-namespace dispatch like `Option.map(opt, f)` — prelude helper, tagged return.
            if matches!(
                &**receiver,
                Expression::Identifier { qualified: Some(q), .. } if q.starts_with("prelude.")
            ) {
                return None;
            }
        }
        // Tagged-type constructors compile to `lisette.Make…(...)`,
        // not multi-return Go calls.
        if inner.as_result_constructor().is_some()
            || inner.as_option_constructor().is_some()
            || inner.as_partial_constructor().is_some()
        {
            return None;
        }
        // Prelude function refs (`assert_type(x)`) — prelude helper, tagged return.
        if let Expression::Identifier {
            qualified: Some(q), ..
        } = inner
            && q.starts_with("prelude.")
        {
            return None;
        }
        self.classify_direct_emission(&return_type)
    }
}

fn receiver_is_prelude_type(ty: &Type) -> bool {
    matches!(
        ty.strip_refs().unwrap_forall(),
        Type::Nominal { id, .. } if id.starts_with("prelude.")
    )
}

pub(crate) fn tuple_element_types(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Tuple(elements) => elements.clone(),
        Type::Nominal { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

fn annotation_is_go_error(annotation: &Annotation) -> bool {
    let Annotation::Constructor { name, .. } = annotation else {
        return false;
    };
    let leaf = unqualified_name(name);
    leaf == "error"
}
