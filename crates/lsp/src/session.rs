use std::sync::mpsc::{self, Receiver};

use serde_json::Value;

use crate::protocol::{Client, dispatch};
use crate::state::Backend;

pub struct Session {
    backend: Backend,
    outbound: Receiver<Value>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        let (sender, outbound) = mpsc::channel();
        Self {
            backend: Backend::new(Client::new(sender), None),
            outbound,
        }
    }

    pub fn handle(&self, message: &Value) -> Vec<Value> {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let response = dispatch(&self.backend, method, params);
            if let Some(id) = message.get("id").cloned() {
                self.backend.client.reply(id, response);
            }
        }
        for (key, token) in self.backend.scheduler.take_diagnostics() {
            self.backend.run_diagnostics(&key, &token);
            self.backend
                .workspace_mut()
                .finish_diagnostics(&key, &token);
        }
        self.outbound.try_iter().collect()
    }
}
