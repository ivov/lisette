use crate::EmitFacts;
use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::transition;
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::Statement;
use crate::ir::values::GoExpression;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::matching::{PreludeVariant, prelude_constructor};
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::types::Type;

pub(crate) const OPTION_SOME_FIELD: &str = "SomeVal";
pub(crate) const RESULT_OK_FIELD: &str = "OkVal";
pub(crate) const RESULT_ERR_FIELD: &str = "ErrVal";
pub(crate) const PARTIAL_OK_FIELD: &str = "OkVal";
pub(crate) const PARTIAL_ERR_FIELD: &str = "ErrVal";

pub(crate) const RESULT_OK_TAG: &str = "ResultOk";
pub(crate) const RESULT_ERR_TAG: &str = "ResultErr";
pub(crate) const OPTION_SOME_TAG: &str = "OptionSome";
pub(crate) const PARTIAL_OK_TAG: &str = "PartialOk";
pub(crate) const PARTIAL_ERR_TAG: &str = "PartialErr";
const RESULT_OK_CTOR: &str = "MakeResultOk";
const OPTION_SOME_CTOR: &str = "MakeOptionSome";
const RESULT_ERR_CTOR: &str = "MakeResultErr";
const OPTION_NONE_CTOR: &str = "MakeOptionNone";
pub(crate) const PARTIAL_OK_CTOR: &str = "MakePartialOk";
pub(crate) const PARTIAL_BOTH_CTOR: &str = "MakePartialBoth";
pub(crate) const PARTIAL_ERR_CTOR: &str = "MakePartialErr";

pub(crate) enum Fallible {
    Result { ok_ty: Type, err_ty: Type },
    Option { ok_ty: Type },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstructorKind {
    Success,
    Failure,
}

impl Fallible {
    fn from_type(ty: &Type) -> Option<Self> {
        if ty.is_result() {
            let args = ty.get_type_params()?;
            Some(Self::Result {
                ok_ty: args.first()?.clone(),
                err_ty: args.get(1)?.clone(),
            })
        } else if ty.is_option() {
            Some(Self::Option {
                ok_ty: ty.ok_type(),
            })
        } else {
            None
        }
    }

    pub(crate) fn is_result(&self) -> bool {
        matches!(self, Self::Result { .. })
    }

    pub(crate) fn classify_constructor(&self, expression: &Expression) -> Option<ConstructorKind> {
        match (self.is_result(), prelude_constructor(expression)?) {
            (true, PreludeVariant::Ok) | (false, PreludeVariant::Some) => {
                Some(ConstructorKind::Success)
            }
            (true, PreludeVariant::Err) | (false, PreludeVariant::None) => {
                Some(ConstructorKind::Failure)
            }
            _ => None,
        }
    }

    pub(crate) fn ok_ty(&self) -> &Type {
        match self {
            Self::Result { ok_ty, .. } | Self::Option { ok_ty } => ok_ty,
        }
    }

    pub(crate) fn err_ty(&self) -> Option<&Type> {
        match self {
            Self::Result { err_ty, .. } => Some(err_ty),
            Self::Option { .. } => None,
        }
    }

    pub(crate) fn success_tag(&self) -> &'static str {
        match self {
            Self::Result { .. } => RESULT_OK_TAG,
            Self::Option { .. } => OPTION_SOME_TAG,
        }
    }

    pub(crate) fn ok_field(&self) -> &'static str {
        match self {
            Self::Result { .. } => RESULT_OK_FIELD,
            Self::Option { .. } => OPTION_SOME_FIELD,
        }
    }

    pub(crate) fn ok_constructor(&self) -> &'static str {
        match self {
            Self::Result { .. } => RESULT_OK_CTOR,
            Self::Option { .. } => OPTION_SOME_CTOR,
        }
    }

    pub(crate) fn err_constructor(&self) -> &'static str {
        match self {
            Self::Result { .. } => RESULT_ERR_CTOR,
            Self::Option { .. } => OPTION_NONE_CTOR,
        }
    }
}

pub(crate) fn prelude_call(
    callee: &str,
    type_arguments: GoType,
    arguments: Vec<GoExpression>,
) -> GoExpression {
    GoExpression::call(
        GoExpression::instantiation(
            GoExpression::generated(GeneratedPackage::Prelude, callee),
            type_arguments,
        ),
        arguments,
    )
}

impl Planner<'_> {
    pub(crate) fn return_fallible(&self) -> Option<Fallible> {
        self.facts.fallible(self.return_ctx().ty()?)
    }

    pub(crate) fn contextual_err_ty(&self, fallible: &Fallible) -> Option<Type> {
        self.return_fallible()
            .and_then(|returned| returned.err_ty().cloned())
            .or_else(|| fallible.err_ty().cloned())
    }

    pub(crate) fn convert_error_to_return_context(
        &mut self,
        statements: &mut Vec<Statement>,
        operand: &Expression,
        value: GoExpression,
        fallible: &Fallible,
    ) -> GoExpression {
        let (Some(from), Some(to)) = (fallible.err_ty().cloned(), self.contextual_err_ty(fallible))
        else {
            return value;
        };
        if self.facts.peel_alias(&from) == self.facts.peel_alias(&to) {
            return value;
        }
        let adaptation = &self.facts.checked(operand).propagated_error().adaptation;
        let (setup, value) =
            CoercionPlan::adapted(self, adaptation, &from, &to, None).lower(self, value);
        statements.extend(setup);
        value
    }

    pub(crate) fn failure_return_values(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> Vec<GoExpression> {
        let return_ctx = self.return_ctx();
        if let Some(shape) = return_ctx.lowered_shape() {
            let return_ty = return_ctx.expect_ty();
            return match error {
                Some(error) if fallible.is_result() => {
                    self.lowered_err_values(&shape, &return_ty, error)
                }
                _ => self.lowered_none_values(&shape, &return_ty),
            };
        }
        vec![self.contextual_failure(fallible, error)]
    }

    pub(crate) fn failure_return(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> Statement {
        let lowered = self.return_ctx().lowered_shape().is_some();
        let mut values = self.failure_return_values(fallible, error);
        if lowered {
            transition::multi_value_return(values)
        } else {
            plain_return(values.remove(0))
        }
    }

    pub(crate) fn success_return(
        &mut self,
        fallible: &Fallible,
        value: GoExpression,
    ) -> Vec<Statement> {
        let Some(shape) = self.return_ctx().lowered_shape() else {
            let success = self.fallible_success(fallible, value);
            return vec![plain_return(success)];
        };
        let (mut statements, payload) =
            self.lowered_payload_values(&shape, fallible.ok_ty(), value);
        statements.push(transition::multi_value_return(shape.ok_values(payload)));
        statements
    }

    fn fallible_type_args(&self, ok_ty: &Type, err_ty: Option<&Type>) -> GoType {
        let arguments: Vec<GoType> = [Some(ok_ty), err_ty]
            .into_iter()
            .flatten()
            .map(|ty| self.go_type(ty))
            .collect();
        GoType::type_arguments(&arguments)
    }

    pub(crate) fn fallible_call(
        &mut self,
        fallible: &Fallible,
        constructor: &str,
        arguments: Vec<GoExpression>,
    ) -> GoExpression {
        let type_args = self.fallible_type_args(fallible.ok_ty(), fallible.err_ty());
        prelude_call(constructor, type_args, arguments)
    }

    pub(crate) fn fallible_success(
        &mut self,
        fallible: &Fallible,
        value: GoExpression,
    ) -> GoExpression {
        self.fallible_call(fallible, fallible.ok_constructor(), vec![value])
    }

    fn contextual_failure(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> GoExpression {
        let ok_ty = match self.return_ctx().ty() {
            Some(ty) => self.facts.peel_alias(ty).ok_type(),
            None => fallible.ok_ty().clone(),
        };
        let err_ty = fallible.is_result().then(|| {
            self.contextual_err_ty(fallible)
                .expect("Result must have error type")
        });
        let type_args = self.fallible_type_args(&ok_ty, err_ty.as_ref());
        let arguments = error.filter(|_| fallible.is_result()).into_iter().collect();
        prelude_call(fallible.err_constructor(), type_args, arguments)
    }
}

impl EmitFacts<'_> {
    pub(crate) fn fallible(&self, ty: &Type) -> Option<Fallible> {
        Fallible::from_type(&self.peel_alias(ty))
    }
}
