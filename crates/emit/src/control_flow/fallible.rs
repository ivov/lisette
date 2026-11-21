use crate::Planner;
use crate::abi::transition;
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::Statement;
use crate::ir::go_expression::BinaryOp;
use crate::ir::values::GoExpression;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::matching::{PreludeVariant, prelude_constructor};
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(crate) struct TaggedVariant {
    pub(crate) name: &'static str,
    pub(crate) tag: &'static str,
    pub(crate) constructor: &'static str,
    pub(crate) fields: &'static [&'static str],
}

const OPTION_VARIANTS: &[TaggedVariant] = &[
    TaggedVariant {
        name: "Some",
        tag: "OptionSome",
        constructor: "MakeOptionSome",
        fields: &["SomeVal"],
    },
    TaggedVariant {
        name: "None",
        tag: "OptionNone",
        constructor: "MakeOptionNone",
        fields: &[],
    },
];

const RESULT_VARIANTS: &[TaggedVariant] = &[
    TaggedVariant {
        name: "Ok",
        tag: "ResultOk",
        constructor: "MakeResultOk",
        fields: &["OkVal"],
    },
    TaggedVariant {
        name: "Err",
        tag: "ResultErr",
        constructor: "MakeResultErr",
        fields: &["ErrVal"],
    },
];

const PARTIAL_VARIANTS: &[TaggedVariant] = &[
    TaggedVariant {
        name: "Ok",
        tag: "PartialOk",
        constructor: "MakePartialOk",
        fields: &["OkVal"],
    },
    TaggedVariant {
        name: "Err",
        tag: "PartialErr",
        constructor: "MakePartialErr",
        fields: &["ErrVal"],
    },
    TaggedVariant {
        name: "Both",
        tag: "PartialBoth",
        constructor: "MakePartialBoth",
        fields: &["OkVal", "ErrVal"],
    },
];

pub(crate) fn tagged_payload_fields(ty: &Type) -> Option<&'static [TaggedVariant]> {
    if ty.is_option() {
        Some(OPTION_VARIANTS)
    } else if ty.is_result() {
        Some(RESULT_VARIANTS)
    } else if ty.is_partial() {
        Some(PARTIAL_VARIANTS)
    } else {
        None
    }
}

pub(crate) fn option_variant(name: &str) -> &'static TaggedVariant {
    variant_named(OPTION_VARIANTS, name)
}

pub(crate) fn result_variant(name: &str) -> &'static TaggedVariant {
    variant_named(RESULT_VARIANTS, name)
}

pub(crate) fn partial_variant(name: &str) -> &'static TaggedVariant {
    variant_named(PARTIAL_VARIANTS, name)
}

fn variant_named(variants: &'static [TaggedVariant], name: &str) -> &'static TaggedVariant {
    variants
        .iter()
        .find(|variant| variant.name == name)
        .expect("prelude containers declare this variant")
}

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

    fn variants(&self) -> &'static [TaggedVariant] {
        match self {
            Self::Result { .. } => RESULT_VARIANTS,
            Self::Option { .. } => OPTION_VARIANTS,
        }
    }

    fn success(&self) -> &'static TaggedVariant {
        &self.variants()[0]
    }

    fn failure(&self) -> &'static TaggedVariant {
        &self.variants()[1]
    }

    pub(crate) fn failure_condition(&self, value: &GoExpression) -> GoExpression {
        GoExpression::binary(
            GoExpression::value_field(value.clone(), ENUM_TAG_FIELD.to_string()),
            BinaryOp::Ne,
            GoExpression::generated(GeneratedPackage::Prelude, self.success().tag),
        )
    }

    pub(crate) fn ok_field(&self) -> &'static str {
        self.success().fields[0]
    }

    pub(crate) fn ok_constructor(&self) -> &'static str {
        self.success().constructor
    }

    pub(crate) fn err_constructor(&self) -> &'static str {
        self.failure().constructor
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
        let (setup, value) = self
            .propagated_error_coercion(operand, &from, &to)
            .lower(self, value);
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

    pub(crate) fn fallible_call(
        &mut self,
        fallible: &Fallible,
        constructor: &str,
        arguments: Vec<GoExpression>,
    ) -> GoExpression {
        let type_args = self.format_type_arg_iter(
            [Some(fallible.ok_ty()), fallible.err_ty()]
                .into_iter()
                .flatten(),
        );
        prelude_call(constructor, type_args, arguments)
    }

    fn fallible_success(&mut self, fallible: &Fallible, value: GoExpression) -> GoExpression {
        self.fallible_call(fallible, fallible.ok_constructor(), vec![value])
    }

    fn contextual_failure(
        &mut self,
        fallible: &Fallible,
        error: Option<GoExpression>,
    ) -> GoExpression {
        let ok_ty = match self.return_ctx().ty() {
            Some(ty) => self.facts.ok_payload(ty),
            None => fallible.ok_ty().clone(),
        };
        let err_ty = fallible.is_result().then(|| {
            self.contextual_err_ty(fallible)
                .expect("Result must have error type")
        });
        let type_args =
            self.format_type_arg_iter([Some(&ok_ty), err_ty.as_ref()].into_iter().flatten());
        let arguments = error.filter(|_| fallible.is_result()).into_iter().collect();
        prelude_call(fallible.err_constructor(), type_args, arguments)
    }
}
