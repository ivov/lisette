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

#[derive(Clone, Debug)]
pub(crate) enum Position {
    Tail,
    Statement,
    Expression,
    Assign(String),
}

impl Position {
    pub(crate) fn is_tail(&self) -> bool {
        matches!(self, Position::Tail)
    }

    pub(crate) fn is_expression(&self) -> bool {
        matches!(self, Position::Expression)
    }

    pub(crate) fn assign_target(&self) -> Option<&str> {
        match self {
            Position::Assign(var) => Some(var),
            _ => None,
        }
    }
}

pub(crate) struct LoopContext {
    pub(crate) result_var: String,
    pub(crate) label: Option<String>,
}

pub(crate) struct ArmPosition {
    pub(crate) position: Position,
    pub(crate) needs_return: bool,
    result_var: Option<String>,
}

impl ArmPosition {
    pub(crate) fn from_position(position: Position) -> Self {
        Self {
            position,
            needs_return: false,
            result_var: None,
        }
    }

    pub(crate) fn with_result_var(var: String) -> Self {
        Self {
            position: Position::Assign(var.clone()),
            needs_return: true,
            result_var: Some(var),
        }
    }

    pub(crate) fn result_var(&self) -> Option<&str> {
        self.result_var.as_deref()
    }
}
