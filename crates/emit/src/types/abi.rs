use crate::Emitter;
use syntax::ast::{Annotation, Expression};
use syntax::types::Type;

/// Go ABI shape that a `Result<T, error>` lowers to at function-boundary
/// positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AbiShape {
    /// `Result<T, error>` → `(T, error)`.
    ResultTuple,
    /// `Result<(), error>` → `error`.
    BareError,
}

impl Emitter<'_> {
    /// Lowered shape for a Lisette return type, or `None` to keep it tagged.
    pub(crate) fn classify_direct_emission(&self, return_ty: &Type) -> Option<AbiShape> {
        let peeled = self.peel_alias(return_ty);
        if peeled.is_result() && self.err_type_is_go_error(&peeled) {
            return Some(if peeled.ok_type().is_unit() {
                AbiShape::BareError
            } else {
                AbiShape::ResultTuple
            });
        }
        None
    }

    /// True when the err slot of a `Result` resolves to Go's `error`,
    /// peeling type aliases (`type MyErr = error`).
    fn err_type_is_go_error(&self, result_ty: &Type) -> bool {
        let err = self.peel_alias(&result_ty.err_type());
        matches!(&err, Type::Nominal { id, .. } if id.as_str() == "prelude.error")
    }

    /// Render the lowered Go return type.
    pub(crate) fn render_lowered_return_ty(
        &mut self,
        shape: &AbiShape,
        return_ty: &Type,
    ) -> String {
        match shape {
            AbiShape::BareError => "error".to_string(),
            AbiShape::ResultTuple => {
                let ok_ty = self.peel_alias(return_ty).ok_type();
                let ok_str = self.go_type_as_string(&ok_ty);
                format!("({}, error)", ok_str)
            }
        }
    }

    /// `&self` variant of `render_lowered_return_ty`, callable from the
    /// `go_type` recursion which doesn't have `&mut self`.
    pub(crate) fn lowered_return_go_type(
        &self,
        shape: &AbiShape,
        return_ty: &Type,
    ) -> crate::types::go_type::GoType {
        use crate::types::go_type::GoType;
        match shape {
            AbiShape::BareError => GoType::new("error".to_string()),
            AbiShape::ResultTuple => {
                let ok_ty = self.peel_alias(return_ty).ok_type();
                let ok_go = self.go_type(&ok_ty);
                let mut result = GoType::new(format!("({}, error)", ok_go.code));
                result.merge_from(&ok_go);
                result
            }
        }
    }

    /// Annotation-side mirror of `classify_direct_emission`.
    pub(crate) fn classify_annotation_direct_emission(
        &self,
        annotation: &Annotation,
    ) -> Option<AbiShape> {
        let Annotation::Constructor { name, params, .. } = annotation else {
            return None;
        };
        let leaf = name.rsplit('.').next().unwrap_or(name);
        if leaf != "Result" || params.len() != 2 || !annotation_is_go_error(&params[1]) {
            return None;
        }
        Some(if params[0].is_unit() {
            AbiShape::BareError
        } else {
            AbiShape::ResultTuple
        })
    }

    /// Annotation-side mirror of `lowered_return_go_type`.
    pub(crate) fn lowered_return_go_type_from_annotation(
        &self,
        shape: &AbiShape,
        return_ann: &Annotation,
    ) -> crate::types::go_type::GoType {
        use crate::types::go_type::GoType;
        match shape {
            AbiShape::BareError => GoType::new("error".to_string()),
            AbiShape::ResultTuple => {
                let ok_ann = match return_ann {
                    Annotation::Constructor { params, .. } => &params[0],
                    _ => unreachable!("ResultTuple shape implies Constructor annotation"),
                };
                let ok_go = self.go_type_from_annotation(ok_ann);
                let mut result = GoType::new(format!("({}, error)", ok_go.code));
                result.merge_from(&ok_go);
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
            && Self::is_go_receiver(receiver)
        {
            return None;
        }
        // Result/Option constructors compile to `lisette.MakeResultOk(...)`,
        // not multi-return Go calls.
        if inner.as_result_constructor().is_some() || inner.as_option_constructor().is_some() {
            return None;
        }
        self.classify_direct_emission(&return_type)
    }
}

fn annotation_is_go_error(annotation: &Annotation) -> bool {
    let Annotation::Constructor { name, .. } = annotation else {
        return false;
    };
    let leaf = name.rsplit('.').next().unwrap_or(name);
    leaf == "error"
}
