use crate::go::names::go_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreludeType {
    Option,
    Result,
    Partial,
    Range,
    RangeInclusive,
    RangeFrom,
    RangeTo,
    RangeToInclusive,
    PanicValue,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VariantInfo {
    /// The variant name (e.g., "Some", "None", "Ok", "Err")
    pub(crate) name: &'static str,
}

impl PreludeType {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "Option" => Some(Self::Option),
            "Result" => Some(Self::Result),
            "Partial" => Some(Self::Partial),
            "Range" => Some(Self::Range),
            "RangeInclusive" => Some(Self::RangeInclusive),
            "RangeFrom" => Some(Self::RangeFrom),
            "RangeTo" => Some(Self::RangeTo),
            "RangeToInclusive" => Some(Self::RangeToInclusive),
            "PanicValue" => Some(Self::PanicValue),
            _ => None,
        }
    }

    pub(crate) fn emit_type(&self, type_args: &[String]) -> String {
        let name = self.go_name();
        let pkg = go_name::GO_STDLIB_PKG;
        if type_args.is_empty() {
            format!("{pkg}.{}", name)
        } else {
            format!("{pkg}.{}[{}]", name, type_args.join(", "))
        }
    }

    pub(crate) fn go_name(&self) -> &'static str {
        match self {
            Self::Option => "Option",
            Self::Result => "Result",
            Self::Partial => "Partial",
            Self::Range => "Range",
            Self::RangeInclusive => "RangeInclusive",
            Self::RangeFrom => "RangeFrom",
            Self::RangeTo => "RangeTo",
            Self::RangeToInclusive => "RangeToInclusive",
            Self::PanicValue => "PanicValue",
        }
    }

    pub(crate) fn variants(&self) -> Option<&'static [VariantInfo]> {
        match self {
            Self::Option => Some(&[VariantInfo { name: "Some" }, VariantInfo { name: "None" }]),
            Self::Result => Some(&[VariantInfo { name: "Ok" }, VariantInfo { name: "Err" }]),
            Self::Partial => Some(&[
                VariantInfo { name: "Ok" },
                VariantInfo { name: "Err" },
                VariantInfo { name: "Both" },
            ]),
            Self::Range
            | Self::RangeInclusive
            | Self::RangeFrom
            | Self::RangeTo
            | Self::RangeToInclusive
            | Self::PanicValue => None,
        }
    }

    pub(crate) fn make_function_name(&self, variant: &str) -> String {
        format!("prelude.Make{}{}", self.go_name(), variant)
    }

    pub(crate) fn enum_types() -> &'static [PreludeType] {
        &[Self::Option, Self::Result, Self::Partial]
    }

    pub(crate) fn make_function_entries(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.variants().into_iter().flatten().map(move |v| {
            let constructor = format!("{}.{}", self.go_name(), v.name);
            let make_fn = self.make_function_name(v.name);
            (constructor, make_fn)
        })
    }
}
