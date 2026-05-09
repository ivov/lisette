use std::io;
use std::time::Duration;

use bytes::{BufMut, BytesMut};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{DuplexStream, ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use tower_lsp::lsp_types::*;
use tower_lsp::{LspService, Server};

use lsp::Backend;

/// LSP message codec implementing Content-Length framing.
struct LspCodec;

impl Decoder for LspCodec {
    type Item = Value;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let Some(header_end) = src.windows(4).position(|w| w == b"\r\n\r\n") else {
            return Ok(None);
        };
        let Ok(header) = std::str::from_utf8(&src[..header_end]) else {
            return Ok(None);
        };
        let Some(len) = header
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: ")?.parse().ok())
        else {
            return Ok(None);
        };

        if src.len() < header_end + 4 + len {
            return Ok(None);
        }

        let _ = src.split_to(header_end + 4);
        let body = src.split_to(len);
        serde_json::from_slice(&body).map(Some).map_err(Into::into)
    }
}

impl Encoder<Value> for LspCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let body = serde_json::to_vec(&item)?;
        dst.put_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        dst.put_slice(&body);
        Ok(())
    }
}

/// A test client for communicating with the LSP server.
pub struct TestClient {
    reader: FramedRead<ReadHalf<DuplexStream>, LspCodec>,
    writer: FramedWrite<WriteHalf<DuplexStream>, LspCodec>,
    next_id: i64,
    buffered: Vec<Value>,
}

impl TestClient {
    /// Spawn a new LSP server and return a connected client.
    pub async fn new() -> Self {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server);
        let (client_read, client_write) = tokio::io::split(client);

        let (service, socket) = LspService::new(|client| Backend::new(client, None));
        tokio::spawn(Server::new(server_read, server_write, socket).serve(service));

        Self {
            reader: FramedRead::new(client_read, LspCodec),
            writer: FramedWrite::new(client_write, LspCodec),
            next_id: 1,
            buffered: Vec::new(),
        }
    }

    async fn request<T: for<'de> Deserialize<'de>>(&mut self, method: &str, params: Value) -> T {
        let id = self.next_id;
        self.next_id += 1;

        self.writer
            .send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await
            .unwrap();

        loop {
            let msg = self.reader.next().await.unwrap().unwrap();
            if msg.get("id") == Some(&json!(id)) {
                return serde_json::from_value(msg.get("result").cloned().unwrap_or(Value::Null))
                    .unwrap();
            }
            self.buffered.push(msg);
        }
    }

    async fn notify(&mut self, method: &str, params: Value) {
        self.writer
            .send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
            .unwrap();
    }

    pub async fn initialize(&mut self) -> InitializeResult {
        let result = self
            .request(
                "initialize",
                json!({"processId": null, "capabilities": {}, "rootUri": null}),
            )
            .await;
        self.notify("initialized", json!({})).await;
        result
    }

    pub async fn initialize_with_root(&mut self, root: &std::path::Path) -> InitializeResult {
        let root_uri = Url::from_file_path(root).unwrap().to_string();
        let result = self
            .request(
                "initialize",
                json!({"processId": null, "capabilities": {}, "rootUri": root_uri}),
            )
            .await;
        self.notify("initialized", json!({})).await;
        result
    }

    pub async fn await_diagnostics(&mut self) -> Vec<Diagnostic> {
        // Check buffered notifications first
        for msg in self.buffered.drain(..) {
            if msg.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && let Some(params) = msg.get("params")
                && let Ok(result) =
                    serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
            {
                return result.diagnostics;
            }
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match tokio::time::timeout_at(deadline, self.reader.next()).await {
                Ok(Some(Ok(msg))) => {
                    if msg.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                        && let Some(params) = msg.get("params")
                        && let Ok(result) =
                            serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
                    {
                        return result.diagnostics;
                    }
                }
                _ => return Vec::new(),
            }
        }
    }

    pub async fn open(&mut self, uri: &str, content: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {"uri": uri, "languageId": "lisette", "version": 1, "text": content}
            }),
        )
        .await;
        self.wait_for_server().await;
    }

    pub async fn change(&mut self, uri: &str, content: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": [{"text": content}]
            }),
        )
        .await;
        self.wait_for_server().await;
    }

    async fn wait_for_server(&mut self) {
        tokio::task::yield_now().await;
    }

    pub async fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<Hover> {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
        .await
    }

    pub async fn goto_definition(
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
        .await
    }

    pub async fn references(
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
        .await
    }

    pub async fn completion(
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
        .await
    }

    pub async fn signature_help(
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
        .await
    }

    pub async fn prepare_rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<PrepareRenameResponse> {
        self.request(
            "textDocument/prepareRename",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character}
            }),
        )
        .await
    }

    pub async fn rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        self.request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "newName": new_name
            }),
        )
        .await
    }

    pub async fn formatting(&mut self, uri: &str) -> Option<Vec<TextEdit>> {
        self.request(
            "textDocument/formatting",
            json!({
                "textDocument": {"uri": uri},
                "options": {"tabSize": 4, "insertSpaces": true}
            }),
        )
        .await
    }

    pub async fn document_symbol(&mut self, uri: &str) -> Option<DocumentSymbolResponse> {
        self.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        )
        .await
    }

    pub async fn shutdown(&mut self) {
        let _: Value = self.request("shutdown", json!(null)).await;
    }
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

pub fn completion_labels(response: &CompletionResponse) -> Vec<String> {
    match response {
        CompletionResponse::Array(items) => items.iter().map(|i| i.label.clone()).collect(),
        CompletionResponse::List(list) => list.items.iter().map(|i| i.label.clone()).collect(),
    }
}

pub fn symbol_names(response: &DocumentSymbolResponse) -> Vec<String> {
    match response {
        DocumentSymbolResponse::Flat(s) => s.iter().map(|s| s.name.clone()).collect(),
        DocumentSymbolResponse::Nested(s) => s.iter().map(|s| s.name.clone()).collect(),
    }
}
