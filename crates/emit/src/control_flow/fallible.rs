use crate::Emitter;
use crate::names::go_name;
use syntax::ast::Expression;
use syntax::types::Type;

pub(crate) const OPTION_SOME_FIELD: &str = "SomeVal";
pub(crate) const RESULT_OK_FIELD: &str = "OkVal";
pub(crate) const RESULT_ERR_FIELD: &str = "ErrVal";
pub(crate) const PARTIAL_OK_FIELD: &str = "OkVal";
pub(crate) const PARTIAL_ERR_FIELD: &str = "ErrVal";

pub(crate) const RESULT_OK_TAG: &str = "lisette.ResultOk";
pub(crate) const OPTION_SOME_TAG: &str = "lisette.OptionSome";
pub(crate) const PARTIAL_OK_TAG: &str = "lisette.PartialOk";
pub(crate) const PARTIAL_ERR_TAG: &str = "lisette.PartialErr";
const RESULT_OK_CTOR: &str = "lisette.MakeResultOk";
const OPTION_SOME_CTOR: &str = "lisette.MakeOptionSome";
const RESULT_ERR_CTOR: &str = "lisette.MakeResultErr";
const OPTION_NONE_CTOR: &str = "lisette.MakeOptionNone";
pub(crate) const PARTIAL_OK_CTOR: &str = "lisette.MakePartialOk";
pub(crate) const PARTIAL_BOTH_CTOR: &str = "lisette.MakePartialBoth";

pub(crate) struct Fallible {
    kind: FallibleKind,
    ok_ty: Type,
    err_ty: Option<Type>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FallibleKind {
    Result,
    Option,
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
            Some(Self {
                kind: FallibleKind::Result,
                ok_ty: args.first()?.clone(),
                err_ty: args.get(1).cloned(),
            })
        } else if ty.is_option() {
            Some(Self {
                kind: FallibleKind::Option,
                ok_ty: ty.ok_type(),
                err_ty: None,
            })
        } else {
            None
        }
    }

    pub(crate) fn is_result(&self) -> bool {
        self.kind == FallibleKind::Result
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
        &self.ok_ty
    }

    pub(crate) fn err_ty(&self) -> Option<&Type> {
        self.err_ty.as_ref()
    }

    pub(crate) fn struct_name(&self) -> &'static str {
        match self.kind {
            FallibleKind::Result => "Result",
            FallibleKind::Option => "Option",
        }
    }

    pub(crate) fn success_tag(&self) -> &'static str {
        match self.kind {
            FallibleKind::Result => RESULT_OK_TAG,
            FallibleKind::Option => OPTION_SOME_TAG,
        }
    }

    pub(crate) fn ok_field(&self) -> &'static str {
        match self.kind {
            FallibleKind::Result => RESULT_OK_FIELD,
            FallibleKind::Option => OPTION_SOME_FIELD,
        }
    }

    pub(crate) fn ok_constructor(&self) -> &'static str {
        match self.kind {
            FallibleKind::Result => RESULT_OK_CTOR,
            FallibleKind::Option => OPTION_SOME_CTOR,
        }
    }

    pub(crate) fn err_constructor(&self) -> &'static str {
        match self.kind {
            FallibleKind::Result => RESULT_ERR_CTOR,
            FallibleKind::Option => OPTION_NONE_CTOR,
        }
    }

    pub(crate) fn err_constructor_takes_arg(&self) -> bool {
        self.kind == FallibleKind::Result
    }

    pub(crate) fn make_success(&self, value: &str, inner_ty: &str, err_ty: Option<&str>) -> String {
        let pkg = go_name::GO_STDLIB_PKG;
        match self.kind {
            FallibleKind::Option => {
                format!("{pkg}.MakeOptionSome[{}]({})", inner_ty, value)
            }
            FallibleKind::Result => {
                let err_ty = err_ty.expect("Result must have error type");
                format!("{pkg}.MakeResultOk[{}, {}]({})", inner_ty, err_ty, value)
            }
        }
    }
}

/// Helper for emitting fallible type wrappers with access to type strings.
///
/// This struct consolidates the duplicated wrapper emission logic by providing
/// a unified interface for generating Result/Option constructors.
pub(crate) struct FallibleEmitter<'a, 'e> {
    pub(crate) emitter: &'a mut Emitter<'e>,
    fallible: &'a Fallible,
}

impl<'a, 'e> FallibleEmitter<'a, 'e> {
    pub(crate) fn new(emitter: &'a mut Emitter<'e>, fallible: &'a Fallible) -> Self {
        Self { emitter, fallible }
    }

    /// Get the Go type string for the ok type.
    pub(crate) fn ok_type_string(&mut self) -> String {
        self.emitter.go_type_as_string(self.fallible.ok_ty())
    }

    /// Get the Go type string for the error type (Result only).
    pub(crate) fn err_type_string(&mut self) -> Option<String> {
        self.fallible
            .err_ty()
            .map(|t| self.emitter.go_type_as_string(t))
    }

    /// Get the ok type string from the current return context, falling back to the fallible's ok type.
    pub(crate) fn contextual_ok_type_string(&mut self) -> String {
        if let Some(ty) = &self.emitter.current_return_context {
            self.emitter.go_type_as_string(&ty.ok_type())
        } else {
            self.ok_type_string()
        }
    }

    /// Format the full type string (e.g., `lisette.Result[int, error]`).
    pub(crate) fn full_type_string(&mut self) -> String {
        let pkg = go_name::GO_STDLIB_PKG;
        let inner_ty = self.ok_type_string();
        if self.fallible.is_result() {
            let err_ty = self.emitter.go_type(
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
        }
    }

    /// Emit a success wrapper (MakeResultOk or MakeOptionSome).
    pub(crate) fn emit_success(&mut self, value: &str) -> String {
        let inner_ty = self.ok_type_string();
        let err_ty = self.err_type_string();
        self.fallible
            .make_success(value, &inner_ty, err_ty.as_deref())
    }

    /// Emit a failure wrapper (MakeResultErr or MakeOptionNone).
    pub(crate) fn emit_failure(&mut self, error_value: Option<&str>) -> String {
        let pkg = go_name::GO_STDLIB_PKG;
        let inner_ty = self.ok_type_string();
        if self.fallible.is_result() {
            let err_ty = self.err_type_string().expect("Result must have error type");
            format!(
                "{pkg}.MakeResultErr[{}, {}]({})",
                inner_ty,
                err_ty,
                error_value.unwrap_or("")
            )
        } else {
            format!("{pkg}.MakeOptionNone[{}]()", inner_ty)
        }
    }

    /// Emit a failure wrapper using the contextual ok type (from return context).
    pub(crate) fn emit_contextual_failure(&mut self, error_value: Option<&str>) -> String {
        let pkg = go_name::GO_STDLIB_PKG;
        let inner_ty = self.contextual_ok_type_string();
        if self.fallible.is_result() {
            let err_ty = self.err_type_string().expect("Result must have error type");
            format!(
                "{pkg}.MakeResultErr[{}, {}]({})",
                inner_ty,
                err_ty,
                error_value.unwrap_or("")
            )
        } else {
            format!("{pkg}.MakeOptionNone[{}]()", inner_ty)
        }
    }

    /// Format a constructor call with the appropriate type parameters.
    pub(crate) fn format_constructor_call(
        &mut self,
        constructor: &str,
        arg: Option<&str>,
    ) -> String {
        let inner_ty = self.ok_type_string();
        let arg_str = arg.unwrap_or("");
        if self.fallible.is_result() {
            let err_ty = self.emitter.go_type(
                self.fallible
                    .err_ty()
                    .expect("Result type must have an error type"),
            );
            format!("{}[{}, {}]({})", constructor, inner_ty, err_ty, arg_str)
        } else {
            format!("{}[{}]({})", constructor, inner_ty, arg_str)
        }
    }
}
