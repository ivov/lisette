use rustc_hash::FxHashMap as HashMap;

#[derive(Clone, Debug)]
pub(crate) struct InlineExpr {
    text: String,
    /// Emitter vars the text references, recorded as uses on substitution.
    refs: Vec<String>,
    contains_deferred_evaluation: bool,
}

impl InlineExpr {
    pub(crate) fn new(
        text: impl Into<String>,
        refs: Vec<String>,
        contains_deferred_evaluation: bool,
    ) -> Self {
        Self {
            text: text.into(),
            refs,
            contains_deferred_evaluation,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn refs(&self) -> &[String] {
        &self.refs
    }

    pub(crate) fn contains_deferred_evaluation(&self) -> bool {
        self.contains_deferred_evaluation
    }
}

#[derive(Clone, Debug)]
pub(crate) enum BindingValue {
    GoName(String),
    GoConst(String),
    InlineExpr(InlineExpr),
}

pub(crate) type BindingUndo = Vec<(String, Option<BindingValue>)>;

pub(crate) struct BindingSnapshot {
    bindings: HashMap<String, BindingValue>,
    undo: BindingUndo,
}

impl BindingSnapshot {
    pub(crate) fn new(bindings: HashMap<String, BindingValue>, undo: BindingUndo) -> Self {
        Self { bindings, undo }
    }

    pub(crate) fn into_inner(self) -> (HashMap<String, BindingValue>, BindingUndo) {
        (self.bindings, self.undo)
    }
}

impl BindingValue {
    pub(crate) fn as_go_name(&self) -> Option<&str> {
        match self {
            BindingValue::GoName(name) | BindingValue::GoConst(name) => Some(name.as_str()),
            BindingValue::InlineExpr(_) => None,
        }
    }

    pub(crate) fn is_go_const(&self) -> bool {
        matches!(self, BindingValue::GoConst(_))
    }
}
