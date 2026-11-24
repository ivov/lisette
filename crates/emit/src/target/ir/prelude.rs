use crate::target::ir::go_expression::BinaryOp;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use syntax::ast::{ConstructorPatternResolution, Expression, Pattern};
use syntax::go_names::ENUM_TAG_FIELD;
use syntax::program;
use syntax::types::{Type, unqualified_name};

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartialVariant {
    Ok,
    Both,
    Err,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreludeVariant {
    Some,
    None,
    Ok,
    Err,
    Partial(PartialVariant),
}

impl PreludeVariant {
    fn from_resolution(enum_name: &str, variant_name: &str) -> Option<Self> {
        Some(match (enum_name, variant_name) {
            ("prelude.Option", "Some") => Self::Some,
            ("prelude.Option", "None") => Self::None,
            ("prelude.Result", "Ok") => Self::Ok,
            ("prelude.Result", "Err") => Self::Err,
            ("prelude.Partial", "Ok") => Self::Partial(PartialVariant::Ok),
            ("prelude.Partial", "Both") => Self::Partial(PartialVariant::Both),
            ("prelude.Partial", "Err") => Self::Partial(PartialVariant::Err),
            _ => return None,
        })
    }
}

pub(crate) fn prelude_variant(pattern: &Pattern) -> Option<(PreludeVariant, &[Pattern])> {
    let Pattern::EnumVariant {
        fields,
        rest: false,
        resolution:
            ConstructorPatternResolution::EnumVariant {
                enum_name,
                variant_name,
            },
        ..
    } = pattern
    else {
        return None;
    };
    let variant = PreludeVariant::from_resolution(enum_name, unqualified_name(variant_name))?;
    Some((variant, fields))
}

pub(crate) fn prelude_constructor(expression: &Expression) -> Option<PreludeVariant> {
    let (enum_name, variant_name) = program::prelude_constructor(expression)?;
    PreludeVariant::from_resolution(enum_name, variant_name)
}

pub(crate) fn is_prelude_container_type(ty: &Type) -> bool {
    ty.is_option() || ty.is_result() || ty.is_partial()
}
