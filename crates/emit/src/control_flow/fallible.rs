use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::transition;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::matching::{PreludeVariant, prelude_constructor};
use crate::plan::bodies::Statement;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::types::Type;

pub(crate) const OPTION_SOME_FIELD: &str = "SomeVal";
pub(crate) const RESULT_OK_FIELD: &str = "OkVal";
pub(crate) const RESULT_ERR_FIELD: &str = "ErrVal";
pub(crate) const PARTIAL_OK_FIELD: &str = "OkVal";
pub(crate) const PARTIAL_ERR_FIELD: &str = "ErrVal";

pub(crate) const RESULT_OK_TAG: &str = "ResultOk";
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
    Success, // Some(x) or Ok(x)
    Failure, // None or Err(x)
}

impl Fallible {
    pub(crate) fn from_type(ty: &Type) -> Option<Self> {
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

    fn struct_name(&self) -> &'static str {
        match self {
            Self::Result { .. } => "Result",
            Self::Option { .. } => "Option",
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
    type_arguments: String,
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
    pub(crate) fn contextual_err_ty(&self, fallible: &Fallible) -> Option<Type> {
        if let Some(ty) = self.return_ctx().ty() {
            let peeled = self.facts.peel_alias(ty);
            if peeled.is_result() {
                return Some(peeled.err_type());
            }
        }
        fallible.err_ty().cloned()
    }

    pub(crate) fn coerce_value(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        from: &Type,
        to: &Type,
    ) -> GoExpression {
        let coercion = CoercionPlan::internal(self, from, to);
        let (setup, value) = coercion.lower(self, value);
        statements.extend(setup);
        value
    }

    pub(crate) fn convert_error_to_return_context(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        fallible: &Fallible,
    ) -> GoExpression {
        let (Some(from), Some(to)) = (fallible.err_ty().cloned(), self.contextual_err_ty(fallible))
        else {
            return value;
        };
        self.coerce_value(statements, value, &from, &to)
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
                    transition::lowered_err_values(self, &shape, &return_ty, error)
                }
                _ => transition::lowered_none_values(self, &shape, &return_ty),
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
            transition::lowered_payload_values(self, &shape, fallible.ok_ty(), value);
        statements.push(transition::multi_value_return(
            transition::lowered_ok_values(&shape, payload),
        ));
        statements
    }
}

impl Planner<'_> {
    /// `T` or `T, E`, the type arguments of a prelude `Option` or `Result`.
    fn fallible_type_args(&mut self, ok_ty: &Type, err_ty: Option<&Type>) -> String {
        let ok = self.use_go_type(ok_ty);
        match err_ty {
            Some(err_ty) => format!("{ok}, {}", self.use_go_type(err_ty)),
            None => ok,
        }
    }

    pub(crate) fn fallible_go_type(&mut self, fallible: &Fallible) -> String {
        let type_args = self.fallible_type_args(fallible.ok_ty(), fallible.err_ty());
        let code = format!(
            "{}.{}[{type_args}]",
            go_name::GO_STDLIB_PKG,
            fallible.struct_name()
        );
        self.use_rendered_go_type(GoType::stdlib(code))
    }

    pub(crate) fn fallible_call(
        &mut self,
        fallible: &Fallible,
        constructor: &str,
        arguments: Vec<GoExpression>,
    ) -> GoExpression {
        let type_args = self.fallible_type_args(fallible.ok_ty(), fallible.err_ty());
        prelude_call(constructor, format!("[{type_args}]"), arguments)
    }

    pub(crate) fn fallible_success(
        &mut self,
        fallible: &Fallible,
        value: GoExpression,
    ) -> GoExpression {
        self.fallible_call(fallible, fallible.ok_constructor(), vec![value])
    }

    /// `Err(error)` or `None`: an `Option` failure carries no error.
    pub(crate) fn fallible_failure(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> GoExpression {
        let arguments = error.filter(|_| fallible.is_result()).into_iter().collect();
        self.fallible_call(fallible, fallible.err_constructor(), arguments)
    }

    /// A failure typed by the enclosing return type, with the fallible's own types as fallback.
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
        prelude_call(
            fallible.err_constructor(),
            format!("[{type_args}]"),
            arguments,
        )
    }
}
