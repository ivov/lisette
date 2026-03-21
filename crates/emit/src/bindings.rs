use rustc_hash::FxHashMap as HashMap;

use super::go::escape_reserved;

#[derive(Default)]
pub(crate) struct Bindings {
    map: HashMap<String, String>,
    stack: Vec<HashMap<String, String>>,
}

impl Bindings {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&mut self) {
        self.map.clear();
        self.stack.clear();
    }

    pub(crate) fn add(&mut self, key: impl Into<String>, value: impl Into<String>) -> String {
        let go_value = escape_reserved(&value.into()).into_owned();
        self.map.insert(key.into(), go_value.clone());
        go_value
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.map.get(name).map(|s| s.as_str())
    }

    pub(crate) fn has_go_name(&self, go_name: &str) -> bool {
        self.map.values().any(|v| v == go_name)
    }

    pub(crate) fn save(&mut self) {
        self.stack.push(self.map.clone());
    }

    pub(crate) fn restore(&mut self) {
        if let Some(saved) = self.stack.pop() {
            self.map = saved;
        }
    }

    pub(crate) fn snapshot(&self) -> HashMap<String, String> {
        self.map.clone()
    }

    pub(crate) fn restore_snapshot(&mut self, snapshot: HashMap<String, String>) {
        self.map = snapshot;
    }
}
