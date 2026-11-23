use std::env;
use std::fs;
use std::process;

use lisette_lsp::session::Session;
use serde_json::{Value, json};

const URI: &str = "file:///playground/playground.lis";

fn open(source: &str) -> (Session, Vec<Value>) {
    let home = env::temp_dir().join(format!("lisette-lsp-session-home-{}", process::id()));
    fs::create_dir_all(&home).expect("create test home dir");
    deps::set_typedef_home(home);

    let session = Session::new();
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "processId": null, "rootUri": null, "capabilities": {} },
    });
    session.handle(&initialize);
    session.handle(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    let opened = session.handle(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": URI, "languageId": "lisette", "version": 1, "text": source,
        } },
    }));
    (session, opened)
}

fn request(session: &Session, method: &str, line: u32, character: u32) -> Value {
    let replies = session.handle(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": method,
        "params": {
            "textDocument": { "uri": URI },
            "position": { "line": line, "character": character },
        },
    }));
    replies
        .into_iter()
        .find(|reply| reply["id"] == 2)
        .map(|reply| reply["result"].clone())
        .expect("a reply to the request")
}

fn published_diagnostics(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .find(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == URI
        })
        .and_then(|message| message["params"]["diagnostics"].as_array().cloned())
        .expect("diagnostics for the playground document")
}

#[test]
fn hover_after_non_ascii_text_uses_utf16_positions() {
    let source = "fn main() {\n  let label = \"Grüße aus Köln 🎉\"\n  let size = label.length()\n  let _ = size\n}\n";
    let (session, _) = open(source);

    let hover = request(&session, "textDocument/hover", 2, 15);

    assert_eq!(hover["contents"]["value"], "```lisette\nstring\n```");
    assert_eq!(
        hover["range"],
        json!({ "start": { "line": 2, "character": 13 }, "end": { "line": 2, "character": 18 } })
    );
}

#[test]
fn completion_after_non_ascii_text_offers_string_methods() {
    let source = "fn main() {\n  let n = \"aüüü\".length()\n}\n";
    let (session, _) = open(source);

    let completion = request(&session, "textDocument/completion", 1, 17);

    let labels: Vec<&str> = completion
        .as_array()
        .expect("completion items")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"length"), "labels: {labels:?}");
}

#[test]
fn diagnostics_after_non_ascii_text_use_utf16_columns() {
    let source = "fn main() {\n  let label = \"Grüße aus Köln 🎉\"; let size: string = 1\n  let _ = size\n  let _ = label\n}\n";
    let (_, opened) = open(source);

    let diagnostics = published_diagnostics(&opened);

    assert_eq!(diagnostics.len(), 1, "diagnostics: {diagnostics:?}");
    assert_eq!(
        diagnostics[0]["range"],
        json!({ "start": { "line": 1, "character": 54 }, "end": { "line": 1, "character": 55 } })
    );
}

#[test]
fn did_change_publishes_new_diagnostics() {
    let (session, opened) = open("fn main() {}\n");
    assert!(published_diagnostics(&opened).is_empty());

    let changed = session.handle(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "text": "fn main() {\n  let x: string = 1\n  let _ = x\n}\n" }],
        },
    }));

    assert_eq!(published_diagnostics(&changed).len(), 1);
}

#[test]
fn unknown_request_gets_an_error_reply() {
    let (session, _) = open("fn main() {}\n");

    let replies = session.handle(&json!({ "jsonrpc": "2.0", "id": 9, "method": "no/such" }));

    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], 9);
    assert_eq!(replies[0]["error"]["code"], -32601);
}
