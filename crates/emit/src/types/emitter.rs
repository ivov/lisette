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

/// Pure-analysis effects: stdlib gates and Go imports accumulated while a
/// `&Emitter` helper computes a result, applied later at the renderer
/// boundary via `Emitter::apply_effects`.
#[derive(Debug, Clone, Default)]
pub(crate) struct EmitEffects {
    pub needs_stdlib: bool,
    pub needs_fmt: bool,
    pub needs_errors: bool,
    pub needs_slices: bool,
    pub needs_strings: bool,
    pub needs_maps: bool,
    pub go_imports: Vec<String>,
}

impl EmitEffects {
    pub(crate) fn merge_from_go_type(&mut self, go_type: &crate::types::go_type::GoType) {
        if go_type.needs_stdlib {
            self.needs_stdlib = true;
        }
        self.go_imports.extend(go_type.go_imports.iter().cloned());
    }

    pub(crate) fn extend(&mut self, other: &EmitEffects) {
        if other.needs_stdlib {
            self.needs_stdlib = true;
        }
        if other.needs_fmt {
            self.needs_fmt = true;
        }
        if other.needs_errors {
            self.needs_errors = true;
        }
        if other.needs_slices {
            self.needs_slices = true;
        }
        if other.needs_strings {
            self.needs_strings = true;
        }
        if other.needs_maps {
            self.needs_maps = true;
        }
        self.go_imports.extend(other.go_imports.iter().cloned());
    }
}

/// Shape of the enclosing function body's return values.
#[derive(Clone, Debug, Default)]
pub(crate) enum ReturnContext {
    #[default]
    None,
    Tagged(Type),
    Lowered {
        return_ty: Type,
        shape: AbiShape,
    },
    TaggedBlock(Type),
}

impl ReturnContext {
    pub(crate) fn ty(&self) -> Option<&Type> {
        match self {
            ReturnContext::None => None,
            ReturnContext::Tagged(ty)
            | ReturnContext::Lowered { return_ty: ty, .. }
            | ReturnContext::TaggedBlock(ty) => Some(ty),
        }
    }

    pub(crate) fn lowered_shape(&self) -> Option<AbiShape> {
        match self {
            ReturnContext::Lowered { shape, .. } => Some(shape.clone()),
            _ => None,
        }
    }

    /// Clone the return type, asserting the function has a return context.
    /// Use after a `lowered_shape()` check that has already established this.
    pub(crate) fn expect_ty(&self) -> Type {
        self.ty()
            .cloned()
            .expect("lowered abi requires a return context")
    }
}

pub(crate) struct LoopContext {
    pub(crate) result_var: String,
    pub(crate) label: Option<String>,
}
