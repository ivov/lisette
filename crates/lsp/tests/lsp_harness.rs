use std::io::{BufReader, PipeWriter};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use lisette_lsp::protocol::{self, *};

/// A test client for communicating with the LSP server.
pub struct TestClient {
    incoming: mpsc::Receiver<Value>,
    writer: PipeWriter,
    next_id: i64,
    buffered: Vec<Value>,
    exit_code: thread::JoinHandle<i32>,
}

impl Default for TestClient {
    fn default() -> Self {
        Self::new()
    }
}

fn init_test_typedef_home() {
    static HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("lisette-lsp-test-home-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test home dir");
        deps::set_typedef_home(dir.clone());
        dir
    });
}

impl TestClient {
    /// Spawn a new LSP server and return a connected client.
    pub fn new() -> Self {
        init_test_typedef_home();

        let (server_read, client_write) = std::io::pipe().expect("create client-to-server pipe");
        let (client_read, server_write) = std::io::pipe().expect("create server-to-client pipe");

        let exit_code = thread::spawn(move || protocol::serve(server_read, server_write, None));

        let (sender, incoming) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(client_read);
            while let Ok(Some(message)) = protocol::read_message(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });

        Self {
            incoming,
            writer: client_write,
            next_id: 1,
            buffered: Vec::new(),
            exit_code,
        }
    }

    fn try_request<T: for<'de> Deserialize<'de>>(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<T, String> {
        let id = self.next_id;
        self.next_id += 1;

        protocol::write_message(
            &mut self.writer,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .unwrap();

        loop {
            let msg = self
                .incoming
                .recv()
                .expect("server closed the connection before responding");
            if msg.get("id") == Some(&json!(id)) {
                if let Some(error) = msg.get("error") {
                    return Err(error["message"].as_str().unwrap_or_default().to_owned());
                }
                return Ok(serde_json::from_value(
                    msg.get("result").cloned().unwrap_or(Value::Null),
                )
                .unwrap());
            }
            self.buffered.push(msg);
        }
    }

    fn request<T: for<'de> Deserialize<'de>>(&mut self, method: &str, params: Value) -> T {
        match self.try_request(method, params) {
            Ok(result) => result,
            Err(error) => panic!("{method} request failed: {error}"),
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        protocol::write_message(
            &mut self.writer,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
        .unwrap();
    }

    pub fn initialize(&mut self) -> InitializeResult {
        let result = self.request(
            "initialize",
            json!({"processId": null, "capabilities": {}, "rootUri": null}),
        );
        self.notify("initialized", json!({}));
        result
    }

    pub fn initialize_with_root(&mut self, root: &std::path::Path) -> InitializeResult {
        let root_uri = Url::from_file_path(root).unwrap().to_string();
        let result = self.request(
            "initialize",
            json!({"processId": null, "capabilities": {}, "rootUri": root_uri}),
        );
        self.notify("initialized", json!({}));
        result
    }

    pub fn await_diagnostics(&mut self) -> Vec<Diagnostic> {
        for msg in self.buffered.drain(..) {
            if let Some(result) = as_publish_diagnostics(&msg) {
                return result.diagnostics;
            }
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.incoming.recv_timeout(remaining) {
                Ok(msg) => {
                    if let Some(result) = as_publish_diagnostics(&msg) {
                        return result.diagnostics;
                    }
                }
                Err(_) => return Vec::new(),
            }
        }
    }

    pub fn await_diagnostics_for(&mut self, uri: &str) -> Option<Vec<Diagnostic>> {
        let matches = |msg: &Value| {
            as_publish_diagnostics(msg).is_some_and(|result| result.uri.as_str() == uri)
        };

        if let Some(pos) = self.buffered.iter().position(matches) {
            let msg = self.buffered.remove(pos);
            return as_publish_diagnostics(&msg).map(|result| result.diagnostics);
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.incoming.recv_timeout(remaining) {
                Ok(msg) => {
                    if let Some(result) = as_publish_diagnostics(&msg) {
                        if result.uri.as_str() == uri {
                            return Some(result.diagnostics);
                        }
                        self.buffered.push(msg);
                    }
                }
                Err(_) => return None,
            }
        }
    }

    pub fn open(&mut self, uri: &str, content: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {"uri": uri, "languageId": "lisette", "version": 1, "text": content}
            }),
        );
    }

    pub fn change(&mut self, uri: &str, content: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": [{"text": content}]
            }),
        );
    }

    pub fn close(&mut self, uri: &str) {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        );
    }

    pub fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<Hover> {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
    }

    pub fn goto_definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<GotoDefinitionResponse> {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
    }

    pub fn references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        self.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "context": {"includeDeclaration": include_declaration}
            }),
        )
    }

    pub fn completion(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<CompletionResponse> {
        self.request(
            "textDocument/completion",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
    }

    pub fn signature_help(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<SignatureHelp> {
        self.request(
            "textDocument/signatureHelp",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
    }

    pub fn inlay_hint(
        &mut self,
        uri: &str,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Option<Vec<InlayHint>> {
        self.request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": start.0, "character": start.1},
                    "end": {"line": end.0, "character": end.1}
                }
            }),
        )
    }

    pub fn try_prepare_rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<PrepareRenameResponse>, String> {
        self.try_request(
            "textDocument/prepareRename",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
    }

    pub fn prepare_rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<PrepareRenameResponse> {
        self.try_prepare_rename(uri, line, character)
            .expect("prepareRename request failed")
    }

    pub fn try_rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>, String> {
        self.try_request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "newName": new_name
            }),
        )
    }

    pub fn rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        self.try_rename(uri, line, character, new_name)
            .expect("rename request failed")
    }

    pub fn code_action(
        &mut self,
        uri: &str,
        start: (u32, u32),
        end: (u32, u32),
    ) -> Option<CodeActionResponse> {
        self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": start.0, "character": start.1},
                    "end": {"line": end.0, "character": end.1}
                },
                "context": {"diagnostics": []}
            }),
        )
    }

    pub fn formatting(&mut self, uri: &str) -> Option<Vec<TextEdit>> {
        self.request(
            "textDocument/formatting",
            json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": 4, "insertSpaces": true}
            }),
        )
    }

    pub fn document_symbol(&mut self, uri: &str) -> Option<DocumentSymbolResponse> {
        self.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
    }

    pub fn shutdown(&mut self) {
        let _: Value = self.request("shutdown", json!(null));
    }

    pub fn exit(&mut self) {
        self.notify("exit", json!(null));
    }

    pub fn await_exit_code(self) -> i32 {
        self.exit_code.join().expect("server thread panicked")
    }
}

fn as_publish_diagnostics(msg: &Value) -> Option<PublishDiagnosticsParams> {
    if msg.get("method") != Some(&json!("textDocument/publishDiagnostics")) {
        return None;
    }
    serde_json::from_value(msg.get("params")?.clone()).ok()
}

pub fn hover_content(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(m) => m.value.clone(),
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value.clone(),
        HoverContents::Array(arr) => arr
            .first()
            .map(|ms| match ms {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .unwrap_or_default(),
    }
}

pub fn definition_location(response: &GotoDefinitionResponse) -> Option<Location> {
    match response {
        GotoDefinitionResponse::Scalar(loc) => Some(loc.clone()),
        GotoDefinitionResponse::Array(arr) => arr.first().cloned(),
        GotoDefinitionResponse::Link(links) => links.first().map(|l| Location {
            uri: l.target_uri.clone(),
            range: l.target_selection_range,
        }),
    }
}

/// Reads the file a definition `Location` points to and returns the text from
/// the range start to end of line, so a test can assert the jump landed on the
/// expected symbol rather than merely on some file.
pub fn definition_target_text(location: &Location) -> String {
    let path = location
        .uri
        .to_file_path()
        .expect("location uri should be a file path");
    let source = std::fs::read_to_string(&path).expect("definition file should be readable");
    let line = source
        .lines()
        .nth(location.range.start.line as usize)
        .expect("range line should exist");
    line[location.range.start.character as usize..].to_string()
}

pub fn completion_labels(response: &CompletionResponse) -> Vec<String> {
    match response {
        CompletionResponse::Array(items) => items.iter().map(|i| i.label.clone()).collect(),
        CompletionResponse::List(list) => list.items.iter().map(|i| i.label.clone()).collect(),
    }
}

pub fn doc_end(content: &str) -> (u32, u32) {
    let line = content.matches('\n').count() as u32;
    let last_line = content.rsplit('\n').next().unwrap_or("");
    let character = last_line.chars().map(|c| c.len_utf16() as u32).sum();
    (line, character)
}

/// Strips the single `~` cursor marker from a fixture and returns the clean
/// source plus the marker's LSP position.
pub fn cursor(source: &str) -> (String, u32, u32) {
    let (clean, positions) = cursors(source);
    let [(line, character)] = positions[..] else {
        panic!(
            "fixture should contain exactly one ~ marker, found {}",
            positions.len()
        );
    };
    (clean, line, character)
}

/// Strips every `~` cursor marker from a fixture and returns the clean source
/// plus each marker's LSP position (line, UTF-16 character), in source order.
pub fn cursors(source: &str) -> (String, Vec<(u32, u32)>) {
    let mut clean = String::with_capacity(source.len());
    let mut positions = Vec::new();
    let mut line = 0u32;
    let mut character = 0u32;
    for c in source.chars() {
        match c {
            '~' => positions.push((line, character)),
            '\n' => {
                line += 1;
                character = 0;
                clean.push(c);
            }
            _ => {
                character += c.len_utf16() as u32;
                clean.push(c);
            }
        }
    }
    assert!(!positions.is_empty(), "fixture should contain a ~ marker");
    (clean, positions)
}

pub fn inlay_hint_triples(hints: &[InlayHint]) -> Vec<(u32, u32, String)> {
    hints
        .iter()
        .map(|h| {
            let label = match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                InlayHintLabel::LabelParts(parts) => {
                    parts.iter().map(|p| p.value.clone()).collect()
                }
            };
            (h.position.line, h.position.character, label)
        })
        .collect()
}

pub fn symbol_names(response: &DocumentSymbolResponse) -> Vec<String> {
    match response {
        DocumentSymbolResponse::Flat(s) => s.iter().map(|s| s.name.clone()).collect(),
        DocumentSymbolResponse::Nested(s) => s.iter().map(|s| s.name.clone()).collect(),
    }
}
