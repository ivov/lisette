use crate::Planner;
use crate::abi::callable::CallableReturnAbi;
use crate::abi::coercion::CoercionPlan;
use crate::abi::transition;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::LoweredStatement;
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
        let variant = if self.is_result() {
            expression.as_result_constructor()
        } else {
            expression.as_option_constructor()
        };
        match variant {
            Some(Ok(())) => Some(ConstructorKind::Success),
            Some(Err(())) => Some(ConstructorKind::Failure),
            None => None,
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

    pub(crate) fn err_constructor_takes_arg(&self) -> bool {
        self.is_result()
    }

    fn make_success(
        &self,
        value: GoExpression,
        inner_ty: &str,
        err_ty: Option<&str>,
    ) -> GoExpression {
        match self {
            Self::Option { .. } => {
                prelude_call(OPTION_SOME_CTOR, format!("[{}]", inner_ty), vec![value])
            }
            Self::Result { .. } => {
                let err_ty = err_ty.expect("Result must have error type");
                prelude_call(
                    RESULT_OK_CTOR,
                    format!("[{}, {}]", inner_ty, err_ty),
                    vec![value],
                )
            }
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
        statements: &mut Vec<LoweredStatement>,
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
        statements: &mut Vec<LoweredStatement>,
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
        let mut fallible_planner = FalliblePlanner::new(self, fallible);
        vec![fallible_planner.emit_contextual_failure(error)]
    }

    pub(crate) fn failure_return(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> LoweredStatement {
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
        lowered: Option<&CallableReturnAbi>,
    ) -> Vec<LoweredStatement> {
        let Some(shape) = lowered else {
            let success = FalliblePlanner::new(self, fallible).emit_success(value);
            return vec![plain_return(success)];
        };
        let (mut statements, payload) =
            transition::lowered_payload_values(self, shape, fallible.ok_ty(), value);
        statements.push(transition::multi_value_return(
            transition::lowered_ok_values(shape, payload),
        ));
        statements
    }
}

/// Emits Result/Option success and failure constructors with resolved Go
/// type strings.
pub(crate) struct FalliblePlanner<'a, 'e> {
    pub(crate) planner: &'a mut Planner<'e>,
    fallible: &'a Fallible,
}

impl<'a, 'e> FalliblePlanner<'a, 'e> {
    pub(crate) fn new(planner: &'a mut Planner<'e>, fallible: &'a Fallible) -> Self {
        Self { planner, fallible }
    }

    fn ok_type_string(&mut self) -> String {
        self.planner.use_go_type(self.fallible.ok_ty())
    }

    fn err_type_string(&mut self) -> Option<String> {
        self.fallible.err_ty().map(|t| self.planner.use_go_type(t))
    }

    /// Ok type from the enclosing return context, with the fallible's own ok type as fallback.
    fn contextual_ok_type_string(&mut self) -> String {
        let return_ctx = self.planner.return_ctx();
        if let Some(ty) = return_ctx.ty() {
            let ok_ty = ty.ok_type();
            self.planner.use_go_type(&ok_ty)
        } else {
            self.ok_type_string()
        }
    }

    pub(crate) fn full_type_string(&mut self) -> String {
        let pkg = go_name::GO_STDLIB_PKG;
        let inner_ty = self.ok_type_string();
        let code = if self.fallible.is_result() {
            let err_ty = self.planner.use_go_type(
                self.fallible
                    .err_ty()
                    .expect("Result type must have an error type"),
            );
            format!(
                "{}.{}[{}, {}]",
                pkg,
                self.fallible.struct_name(),
                inner_ty,
                err_ty
            )
        } else {
            format!("{}.{}[{}]", pkg, self.fallible.struct_name(), inner_ty)
        };
        self.planner.use_rendered_go_type(GoType::stdlib(code))
    }

    pub(crate) fn emit_success(&mut self, value: GoExpression) -> GoExpression {
        let inner_ty = self.ok_type_string();
        let err_ty = self.err_type_string();
        self.fallible
            .make_success(value, &inner_ty, err_ty.as_deref())
    }

    pub(crate) fn emit_failure(&mut self, error_value: Option<GoExpression>) -> GoExpression {
        let inner_ty = self.ok_type_string();
        if self.fallible.is_result() {
            let err_ty = self.err_type_string().expect("Result must have error type");
            make_failure(&inner_ty, Some(&err_ty), error_value)
        } else {
            make_failure(&inner_ty, None, None)
        }
    }

    /// Emit a failure wrapper using the contextual ok and err types (from return context).
    pub(crate) fn emit_contextual_failure(
        &mut self,
        error_value: Option<GoExpression>,
    ) -> GoExpression {
        let inner_ty = self.contextual_ok_type_string();
        if self.fallible.is_result() {
            let err_ty = self
                .planner
                .contextual_err_ty(self.fallible)
                .expect("Result must have error type");
            let err_ty = self.planner.use_go_type(&err_ty);
            make_failure(&inner_ty, Some(&err_ty), error_value)
        } else {
            make_failure(&inner_ty, None, None)
        }
    }

    pub(crate) fn format_constructor_call(
        &mut self,
        constructor: &str,
        arg: Option<GoExpression>,
    ) -> GoExpression {
        let inner_ty = self.ok_type_string();
        let type_args = if self.fallible.is_result() {
            let err_ty = self
                .err_type_string()
                .expect("Result type must have an error type");
            format!("[{}, {}]", inner_ty, err_ty)
        } else {
            format!("[{}]", inner_ty)
        };
        GoExpression::call(
            GoExpression::instantiation(
                GoExpression::generated(GeneratedPackage::Prelude, constructor),
                type_args,
            ),
            arg.into_iter().collect(),
        )
    }
}

fn make_failure(
    inner_ty: &str,
    err_ty: Option<&str>,
    error_value: Option<GoExpression>,
) -> GoExpression {
    match err_ty {
        Some(err_ty) => prelude_call(
            RESULT_ERR_CTOR,
            format!("[{}, {}]", inner_ty, err_ty),
            error_value.into_iter().collect(),
        ),
        None => prelude_call(OPTION_NONE_CTOR, format!("[{}]", inner_ty), Vec::new()),
    }
}
