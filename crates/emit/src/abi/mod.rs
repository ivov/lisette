pub(crate) mod callable;
pub(crate) mod catalog;
pub(crate) mod coercion;
pub(crate) mod layout;
pub(crate) mod transition;

use crate::Planner;
use crate::names::go_name::PRELUDE_ERROR_ID;
use crate::types::go_type::GoType;
use callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use syntax::ast::{Expression, IdentifierResolution};
use syntax::types::Type;

impl Planner<'_> {
    /// ABI of a value after it has crossed into Lisette expression space.
    pub(crate) fn value_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        let peeled = self.facts.peel_alias(return_ty);
        if is_prelude_container_type(&peeled)
            || peeled.tuple_arity().is_some_and(|arity| arity >= 2)
        {
            CallableReturnAbi::Tagged
        } else {
            CallableReturnAbi::Direct
        }
    }

    /// Natural physical ABI of a Lisette-authored callable return.
    pub(crate) fn callable_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        self.classify_direct_emission(return_ty)
            .unwrap_or_else(|| self.value_return_abi(return_ty))
    }

    /// Lowered shape for a Lisette return type, or `None` to keep it tagged.
    pub(crate) fn classify_direct_emission(&self, return_ty: &Type) -> Option<CallableReturnAbi> {
        let peeled = self.facts.peel_alias(return_ty);
        if peeled.is_result() && self.err_slot_is_nilable(&peeled) {
            return Some(if peeled.ok_type().is_unit() {
                CallableReturnAbi::BareError
            } else {
                CallableReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                }
            });
        }
        if peeled.is_partial() && self.err_slot_is_nilable(&peeled) {
            return Some(CallableReturnAbi::Partial {
                payload: PayloadLayout::Packed,
            });
        }
        if peeled.is_option() {
            let encoding = if self.facts.is_nullable_option(&peeled) {
                OptionReturnAbi::Nullable
            } else {
                OptionReturnAbi::CommaOk {
                    payload: PayloadLayout::Packed,
                }
            };
            return Some(CallableReturnAbi::Option(encoding));
        }
        if let Some(arity) = peeled.tuple_arity()
            && arity >= 2
        {
            return Some(CallableReturnAbi::Tuple { arity });
        }
        None
    }

    /// True when the err slot of a `Result`/`Partial` lowers to a Go
    /// nilable type, so `nil` typechecks as the no-error sentinel.
    fn err_slot_is_nilable(&self, fallible_ty: &Type) -> bool {
        let err = self.facts.peel_alias(&fallible_ty.err_type());
        matches!(&err, Type::Nominal { id, .. } if id.as_str() == PRELUDE_ERROR_ID)
            || self.facts.is_nilable_go_type(&err)
    }

    /// Render the lowered Go return type.
    pub(crate) fn render_lowered_return_ty(
        &mut self,
        shape: &CallableReturnAbi,
        return_ty: &Type,
    ) -> String {
        let go_type = self.lowered_return_go_type(shape, return_ty);
        self.use_rendered_go_type(go_type)
    }

    /// Render a lowered return type together with its package requirements.
    pub(crate) fn lowered_return_go_type(
        &self,
        shape: &CallableReturnAbi,
        return_ty: &Type,
    ) -> GoType {
        let peeled = self.facts.peel_alias(return_ty);
        match shape {
            CallableReturnAbi::Tagged | CallableReturnAbi::Direct => self.go_type(&peeled),
            CallableReturnAbi::BareError => self.go_type(&peeled.err_type()),
            CallableReturnAbi::Result { .. } | CallableReturnAbi::Partial { .. } => {
                go_result_list(&[
                    self.go_type(&peeled.ok_type()),
                    self.go_type(&peeled.err_type()),
                ])
            }
            CallableReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => {
                go_result_list(&[self.go_type(&peeled.ok_type()), GoType::new("bool")])
            }
            CallableReturnAbi::Option(OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_)) => {
                self.go_type(&peeled.ok_type())
            }
            CallableReturnAbi::Tuple { .. } => go_result_list(&self.tuple_slot_go_types(&peeled)),
        }
    }

    fn tuple_slot_go_types(&self, tuple_ty: &Type) -> Vec<GoType> {
        tuple_element_types(tuple_ty)
            .iter()
            .map(|t| {
                if self.facts.is_nullable_option(t) {
                    let inner = self.facts.peel_alias(t).ok_type();
                    self.go_type(&inner)
                } else {
                    self.go_type(t)
                }
            })
            .collect()
    }
}

fn go_result_list(slots: &[GoType]) -> GoType {
    let code = format!(
        "({})",
        slots
            .iter()
            .map(|slot| slot.code.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    GoType::with_dependencies(code, slots)
}

pub(crate) fn tuple_element_types(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Tuple(elements) => elements.clone(),
        Type::Nominal { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

/// Prelude fn refs emit with tagged Go return (`Option[T]`); user fns
/// and lambdas emit with the lowered ABI (`(T, bool)`).
pub(crate) fn is_tagged_shape_fn_value(expression: &Expression) -> bool {
    let inner = expression.unwrap_parens();
    if is_prelude_container_constructor(inner) {
        return true;
    }
    matches!(
        inner,
        Expression::Identifier {
            resolution: IdentifierResolution::Definition(q),
            ..
        } if q.starts_with("prelude.")
    )
}

pub(crate) fn is_prelude_container_type(ty: &Type) -> bool {
    ty.is_option() || ty.is_result() || ty.is_partial()
}

pub(crate) fn is_prelude_container_constructor(expression: &Expression) -> bool {
    expression.as_option_constructor().is_some()
        || expression.as_result_constructor().is_some()
        || expression.as_partial_constructor().is_some()
}
