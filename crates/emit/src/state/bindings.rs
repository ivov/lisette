use rustc_hash::FxHashMap as HashMap;

use crate::escape_reserved;

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
    InlineExpr(InlineExpr),
}

pub(crate) struct BindingSnapshot(HashMap<String, BindingValue>);

impl BindingValue {
    pub(crate) fn as_go_name(&self) -> Option<&str> {
        match self {
            BindingValue::GoName(name) => Some(name.as_str()),
            BindingValue::InlineExpr(_) => None,
        }
    }
}

pub(crate) struct Bindings {
    frames: Vec<HashMap<String, BindingValue>>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            frames: vec![HashMap::default()],
        }
    }
}

impl Bindings {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&mut self) {
        self.frames.truncate(1);
        self.current_mut().clear();
    }

    pub(crate) fn bind_go_name(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> String {
        let go_value = escape_reserved(&value.into()).into_owned();
        self.current_mut()
            .insert(key.into(), BindingValue::GoName(go_value.clone()));
        go_value
    }

    pub(crate) fn bind_inline_expr(&mut self, key: impl Into<String>, expression_text: InlineExpr) {
        self.current_mut()
            .insert(key.into(), BindingValue::InlineExpr(expression_text));
    }

    pub(crate) fn get(&self, name: &str) -> Option<&BindingValue> {
        self.current().get(name)
    }

    pub(crate) fn get_go_name(&self, name: &str) -> Option<&str> {
        self.current().get(name).and_then(BindingValue::as_go_name)
    }

    pub(crate) fn has_go_name(&self, go_name: &str) -> bool {
        self.current()
            .values()
            .filter_map(BindingValue::as_go_name)
            .any(|v| v == go_name)
    }

    pub(crate) fn save(&mut self) {
        self.frames.push(self.current().clone());
    }

    pub(crate) fn restore(&mut self) {
        assert!(self.frames.len() > 1, "cannot pop the base binding frame");
        let _ = self.frames.pop();
    }

    pub(crate) fn snapshot(&self) -> BindingSnapshot {
        BindingSnapshot(self.current().clone())
    }

    pub(crate) fn replace_snapshot(&mut self, snapshot: BindingSnapshot) -> BindingSnapshot {
        BindingSnapshot(std::mem::replace(self.current_mut(), snapshot.0))
    }

    pub(crate) fn remove(&mut self, key: &str) {
        self.current_mut().remove(key);
    }

    fn current(&self) -> &HashMap<String, BindingValue> {
        self.frames
            .last()
            .expect("bindings always retain a base frame")
    }

    fn current_mut(&mut self) -> &mut HashMap<String, BindingValue> {
        self.frames
            .last_mut()
            .expect("bindings always retain a base frame")
    }
}
