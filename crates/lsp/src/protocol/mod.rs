mod types;
mod uri;

use std::borrow::Cow;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use deps::BindgenSetup;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::scheduler::RequestId;
use crate::state::Backend;

pub use types::*;

pub(crate) type RpcResult<T> = Result<T, Error>;

const FILE_WATCH_REGISTRATION_ID: &str = "lisette/file-watchers";

#[derive(Debug)]
pub(crate) struct Error {
    pub(crate) code: i32,
    pub(crate) message: Cow<'static, str>,
    pub(crate) data: Option<Value>,
}

impl Error {
    const INVALID_PARAMS: i32 = -32602;

    pub(crate) fn request_failed(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code: -32803,
            message: message.into(),
            data: None,
        }
    }

    pub(crate) fn cancelled() -> Self {
        Self {
            code: -32800,
            message: "Request cancelled".into(),
            data: None,
        }
    }

    pub(crate) fn content_modified() -> Self {
        Self {
            code: -32801,
            message: "Content changed during the request".into(),
            data: None,
        }
    }

    pub(crate) fn internal(message: &'static str) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }

    pub(crate) fn invalid_params(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code: Self::INVALID_PARAMS,
            message: message.into(),
            data: None,
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: Cow::Owned(format!("Method not found: {method}")),
            data: None,
        }
    }

    fn invalid_request(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Client {
    sender: mpsc::Sender<Value>,
}

impl Client {
    pub(crate) fn respond(&self, id: RequestId, response: RpcResult<Value>) {
        match response {
            Ok(result) => {
                let _ = self
                    .sender
                    .send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            }
            Err(error) => send_error(&self.sender, json!(id), error),
        }
    }
    pub(crate) fn register_file_watchers(&self) {
        let _ = self.sender.send(json!({
            "jsonrpc": "2.0",
            "id": FILE_WATCH_REGISTRATION_ID,
            "method": "client/registerCapability",
            "params": { "registrations": [{
                "id": FILE_WATCH_REGISTRATION_ID,
                "method": "workspace/didChangeWatchedFiles",
                "registerOptions": { "watchers": [
                    { "globPattern": "**/*.lis", "kind": 7 },
                    { "globPattern": "**/lisette.toml", "kind": 7 }
                ] }
            }] }
        }));
    }

    fn handle_response(&self, message: &Value) {
        if message.get("id").and_then(Value::as_str) == Some(FILE_WATCH_REGISTRATION_ID)
            && let Some(error) = message.get("error")
        {
            self.log_message(
                MessageType::WARNING,
                format!("Could not register file watchers: {error}"),
            );
        }
    }

    pub(crate) fn publish_diagnostics(
        &self,
        uri: Url,
        diagnostics: Vec<Diagnostic>,
        version: Option<i32>,
    ) {
        self.notify(
            "textDocument/publishDiagnostics",
            PublishDiagnosticsParams {
                uri,
                diagnostics,
                version,
            },
        );
    }

    pub(crate) fn log_message(&self, kind: MessageType, message: impl Into<String>) {
        #[derive(Serialize)]
        struct Params {
            #[serde(rename = "type")]
            kind: MessageType,
            message: String,
        }

        self.notify(
            "window/logMessage",
            Params {
                kind,
                message: message.into(),
            },
        );
    }

    fn notify(&self, method: &'static str, params: impl Serialize) {
        let Ok(params) = serde_json::to_value(params) else {
            return;
        };
        let _ = self.sender.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }
}

pub fn serve<R, W>(reader: R, writer: W, bindgen_setup: Option<Arc<dyn BindgenSetup>>) -> i32
where
    R: Read,
    W: Write + Send + 'static,
{
    let (sender, outbound) = mpsc::channel();
    let client = Client {
        sender: sender.clone(),
    };
    let backend = Backend::new(client, bindgen_setup);
    let workers = backend.scheduler.start(&backend);

    let writer_thread = thread::spawn(move || -> io::Result<()> {
        let mut writer = writer;
        while let Ok(message) = outbound.recv() {
            write_message(&mut writer, &message)?;
        }
        writer.flush()
    });

    let mut reader = BufReader::new(reader);
    let mut shutdown_received = false;
    let exit_code = loop {
        let message = match read_message(&mut reader) {
            Ok(Some(message)) => message,
            Ok(None) => break 0,
            Err(_) => break 1,
        };

        let Some(method) = message.get("method").and_then(Value::as_str) else {
            if message.get("result").is_some() || message.get("error").is_some() {
                backend.client.handle_response(&message);
                continue;
            }
            if let Some(id) = message.get("id").cloned() {
                send_error(&sender, id, Error::invalid_request("Request has no method"));
            }
            continue;
        };
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if method == "exit" {
            break if shutdown_received { 0 } else { 1 };
        }

        if method == "$/cancelRequest" {
            if let Some(id) = params.get("id")
                && let Ok(id) = serde_json::from_value(id.clone())
            {
                backend.scheduler.cancel(&backend, id);
            }
            continue;
        }

        if shutdown_received {
            if let Some(id) = id {
                send_error(&sender, id, Error::invalid_request("Server has shut down"));
            }
            continue;
        }

        if is_semantic_request(method) {
            if let Some(id) = id {
                match serde_json::from_value::<RequestId>(id.clone()) {
                    Ok(id) => backend.scheduler.request(
                        id,
                        method.to_owned(),
                        params,
                        backend.workspace().generation(),
                    ),
                    Err(_) => send_error(&sender, id, Error::invalid_request("Invalid request ID")),
                }
            }
            continue;
        }

        let response = dispatch(&backend, method, params);
        if method == "shutdown" && response.is_ok() {
            shutdown_received = true;
            backend.scheduler.stop(&backend);
        }

        if let Some(id) = id {
            match response {
                Ok(result) => {
                    let _ = sender.send(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result,
                    }));
                }
                Err(error) => send_error(&sender, id, error),
            }
        }
    };

    backend.scheduler.stop(&backend);
    for worker in workers {
        let _ = worker.join();
    }
    drop(backend);
    drop(sender);
    let _ = writer_thread.join();
    exit_code
}

fn send_error(sender: &mpsc::Sender<Value>, id: Value, error: Error) {
    let mut body = json!({
        "code": error.code,
        "message": error.message,
    });
    if let Some(data) = error.data {
        body["data"] = data;
    }
    let _ = sender.send(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": body,
    }));
}

fn is_semantic_request(method: &str) -> bool {
    matches!(
        method,
        "textDocument/formatting"
            | "textDocument/hover"
            | "textDocument/inlayHint"
            | "textDocument/definition"
            | "textDocument/documentSymbol"
            | "textDocument/references"
            | "textDocument/prepareRename"
            | "textDocument/rename"
            | "textDocument/codeAction"
            | "textDocument/completion"
            | "textDocument/signatureHelp"
    )
}

pub(crate) fn dispatch(backend: &Backend, method: &str, params: Value) -> RpcResult<Value> {
    macro_rules! request {
        ($handler:ident, $params:ty) => {{
            let params = parse_params::<$params>(params)?;
            to_value(backend.$handler(params)?)
        }};
    }
    macro_rules! notification {
        ($handler:ident, $params:ty) => {{
            let params = parse_params::<$params>(params)?;
            backend.$handler(params);
            Ok(Value::Null)
        }};
    }

    match method {
        "initialize" => request!(initialize, InitializeParams),
        "initialized" => notification!(initialized, InitializedParams),
        "textDocument/didOpen" => notification!(did_open, DidOpenTextDocumentParams),
        "textDocument/didChange" => notification!(did_change, DidChangeTextDocumentParams),
        "textDocument/didSave" => notification!(did_save, DidSaveTextDocumentParams),
        "textDocument/didClose" => notification!(did_close, DidCloseTextDocumentParams),
        "workspace/didChangeWatchedFiles" => {
            notification!(did_change_watched_files, DidChangeWatchedFilesParams)
        }
        "workspace/didChangeConfiguration" => {
            notification!(did_change_configuration, DidChangeConfigurationParams)
        }
        "textDocument/formatting" => request!(formatting, DocumentFormattingParams),
        "textDocument/hover" => request!(hover, HoverParams),
        "textDocument/inlayHint" => request!(inlay_hint, InlayHintParams),
        "textDocument/definition" => request!(goto_definition, GotoDefinitionParams),
        "textDocument/documentSymbol" => request!(document_symbol, DocumentSymbolParams),
        "textDocument/references" => request!(references, ReferenceParams),
        "textDocument/prepareRename" => request!(prepare_rename, TextDocumentPositionParams),
        "textDocument/rename" => request!(rename, RenameParams),
        "textDocument/codeAction" => request!(code_action, CodeActionParams),
        "textDocument/completion" => request!(completion, CompletionParams),
        "textDocument/signatureHelp" => request!(signature_help, SignatureHelpParams),
        "shutdown" => to_value(backend.shutdown()?),
        _ => Err(Error::method_not_found(method)),
    }
}

fn parse_params<T: DeserializeOwned>(params: Value) -> RpcResult<T> {
    serde_json::from_value(params).map_err(|error| Error::invalid_params(error.to_string()))
}

fn to_value(value: impl Serialize) -> RpcResult<Value> {
    serde_json::to_value(value).map_err(|error| Error {
        code: -32603,
        message: Cow::Owned(error.to_string()),
        data: None,
    })
}

#[doc(hidden)]
pub fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(io::Error::other)
}

#[doc(hidden)]
pub fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    writer.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trip() {
        let (reader, mut writer) = io::pipe().unwrap();
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "shutdown"});
        let expected = message.clone();

        let writer_thread = thread::spawn(move || write_message(&mut writer, &message));
        let mut reader = BufReader::new(reader);

        assert_eq!(read_message(&mut reader).unwrap(), Some(expected));
        writer_thread.join().unwrap().unwrap();
    }
}
