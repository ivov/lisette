use syntax::program::DotAccessResolution;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberSlot<'a> {
    Named(&'a str),
    TupleElement(usize),
    TupleStructField(usize),
}

impl<'a> MemberSlot<'a> {
    pub(crate) fn of(resolution: &DotAccessResolution, member: &'a str) -> Self {
        match *resolution {
            DotAccessResolution::TupleElement { index } => Self::TupleElement(index),
            DotAccessResolution::TupleStructField { index, .. } => Self::TupleStructField(index),
            DotAccessResolution::Unresolved
            | DotAccessResolution::StructField { .. }
            | DotAccessResolution::PackageMember { .. }
            | DotAccessResolution::EnumVariant { .. }
            | DotAccessResolution::InstanceMethod { .. }
            | DotAccessResolution::InstanceMethodValue { .. }
            | DotAccessResolution::StaticMethod { .. } => Self::Named(member),
        }
    }

    pub(crate) fn name(self) -> Option<&'a str> {
        match self {
            Self::Named(name) => Some(name),
            Self::TupleElement(_) | Self::TupleStructField(_) => None,
        }
    }
}
