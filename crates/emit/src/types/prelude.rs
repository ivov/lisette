use crate::names::go_name;

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

impl PreludeType {
    const ALL: [Self; 9] = [
        Self::Option,
        Self::Result,
        Self::Partial,
        Self::Range,
        Self::RangeInclusive,
        Self::RangeFrom,
        Self::RangeTo,
        Self::RangeToInclusive,
        Self::PanicValue,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|ty| ty.go_name() == name)
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

    fn go_name(&self) -> &'static str {
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
}
