use crate::types::abi::AbiShape;
use syntax::types::Type;

#[derive(Clone)]
pub(crate) struct LineIndex {
    pub(crate) path: String,
    pub(crate) line_offsets: Vec<u32>,
}

impl LineIndex {
    pub(crate) fn from_source(path: String, source: &str) -> Self {
        let mut line_offsets = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_offsets.push((i + 1) as u32);
            }
        }
        Self { path, line_offsets }
    }

    pub(crate) fn line_for_offset(&self, byte_offset: u32) -> usize {
        match self.line_offsets.binary_search(&byte_offset) {
            Ok(line) => line + 1,
            Err(line) => line,
        }
    }

    pub(crate) fn col_for_offset(&self, byte_offset: u32) -> usize {
        let line = self.line_for_offset(byte_offset);
        let line_start = self.line_offsets[line - 1];
        (byte_offset - line_start + 1) as usize
    }
}

#[derive(Default)]
pub(crate) struct EmitFlags {
    pub(crate) needs_fmt: bool,
    pub(crate) needs_stdlib: bool,
    pub(crate) needs_errors: bool,
    pub(crate) needs_slices: bool,
    pub(crate) needs_strings: bool,
    pub(crate) needs_maps: bool,
}

/// Where the current expression's value goes.
#[derive(Clone, Debug)]
pub(crate) enum Destination {
    Tail,
    Statement,
    Expression,
    Assign {
        var: String,
        target_ty: Option<Type>,
    },
}

impl Destination {
    pub(crate) fn is_tail(&self) -> bool {
        matches!(self, Destination::Tail)
    }

    pub(crate) fn assign_target(&self) -> Option<&str> {
        match self {
            Destination::Assign { var, .. } => Some(var),
            _ => None,
        }
    }

    pub(crate) fn assign_target_ty(&self) -> Option<&Type> {
        match self {
            Destination::Assign { target_ty, .. } => target_ty.as_ref(),
            _ => None,
        }
    }
}

/// Shape of the enclosing function body's return values.
#[derive(Clone, Default)]
pub(crate) enum ReturnMode {
    #[default]
    None,
    Tagged(Type),
    Lowered {
        return_ty: Type,
        shape: AbiShape,
    },
    TaggedBlock(Type),
}

impl ReturnMode {
    pub(crate) fn ty(&self) -> Option<&Type> {
        match self {
            ReturnMode::None => None,
            ReturnMode::Tagged(ty)
            | ReturnMode::Lowered { return_ty: ty, .. }
            | ReturnMode::TaggedBlock(ty) => Some(ty),
        }
    }

    pub(crate) fn lowered_shape(&self) -> Option<AbiShape> {
        match self {
            ReturnMode::Lowered { shape, .. } => Some(shape.clone()),
            _ => None,
        }
    }
}

pub(crate) struct LoopContext {
    pub(crate) result_var: String,
    pub(crate) label: Option<String>,
}

pub(crate) enum ArmRouting {
    /// Arm bodies inherit an existing destination.
    Inherit(Destination),
    /// Arm bodies assign to a fresh result var; the match site emits
    /// `return <var>` after the arms.
    CreateAndReturn {
        var: String,
        target_ty: Option<Type>,
    },
}

impl ArmRouting {
    pub(crate) fn into_body_destination(self) -> Destination {
        match self {
            ArmRouting::Inherit(d) => d,
            ArmRouting::CreateAndReturn { var, target_ty } => {
                Destination::Assign { var, target_ty }
            }
        }
    }

    pub(crate) fn result_var(&self) -> Option<&str> {
        match self {
            ArmRouting::CreateAndReturn { var, .. } => Some(var),
            ArmRouting::Inherit(_) => None,
        }
    }
}
