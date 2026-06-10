use rustc_hash::FxHashMap as HashMap;

use crate::escape_reserved;

#[derive(Clone, Debug)]
pub(crate) struct InlineExpr {
    text: String,
    /// Emitter vars the text references, recorded as uses on substitution.
    refs: Vec<String>,
}

impl InlineExpr {
    pub(crate) fn with_refs(text: impl Into<String>, refs: Vec<String>) -> Self {
        Self {
            text: text.into(),
            refs,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn refs(&self) -> &[String] {
        &self.refs
    }
}

#[derive(Clone, Debug)]
pub(crate) enum BindingValue {
    GoName(String),
    InlineExpr(InlineExpr),
}

impl BindingValue {
    pub(crate) fn as_go_name(&self) -> Option<&str> {
        match self {
            BindingValue::GoName(name) => Some(name.as_str()),
            BindingValue::InlineExpr(_) => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct Bindings {
    map: HashMap<String, BindingValue>,
    stack: Vec<HashMap<String, BindingValue>>,
}

impl Bindings {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&mut self) {
        self.map.clear();
        self.stack.clear();
    }

    pub(crate) fn bind_go_name(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> String {
        let go_value = escape_reserved(&value.into()).into_owned();
        self.map
            .insert(key.into(), BindingValue::GoName(go_value.clone()));
        go_value
    }

    pub(crate) fn bind_inline_expr(&mut self, key: impl Into<String>, expression_text: InlineExpr) {
        self.map
            .insert(key.into(), BindingValue::InlineExpr(expression_text));
    }

    pub(crate) fn get(&self, name: &str) -> Option<&BindingValue> {
        self.map.get(name)
    }

    pub(crate) fn get_go_name(&self, name: &str) -> Option<&str> {
        self.map.get(name).and_then(BindingValue::as_go_name)
    }

    pub(crate) fn has_go_name(&self, go_name: &str) -> bool {
        self.map
            .values()
            .filter_map(BindingValue::as_go_name)
            .any(|v| v == go_name)
    }

    pub(crate) fn save(&mut self) {
        self.stack.push(self.map.clone());
    }

    pub(crate) fn restore(&mut self) {
        if let Some(saved) = self.stack.pop() {
            self.map = saved;
        }
    }

    pub(crate) fn snapshot(&self) -> HashMap<String, BindingValue> {
        self.map.clone()
    }

    pub(crate) fn restore_snapshot(&mut self, snapshot: HashMap<String, BindingValue>) {
        self.map = snapshot;
    }

    pub(crate) fn remove(&mut self, key: &str) {
        self.map.remove(key);
    }
}
