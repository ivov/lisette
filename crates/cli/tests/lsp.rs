//! Exercise the real CLI setup, which the in-process LSP harness omits.
#![cfg(unix)]

use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use std::{env, fs, iter, panic};

use lsp::protocol::{Url, read_message, write_message};
use serde_json::{Value, json};

#[test]
fn typing_a_new_go_import_does_not_launch_go_but_saving_can_prepare_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("bin")).unwrap();
    fs::create_dir(root.join("home")).unwrap();
    fs::write(
        root.join("lisette.toml"),
        concat!(
            "[project]\nname = \"example.com/lsp-no-go\"\nversion = \"0.1.0\"\n",
            "[dependencies.go]\n\"github.com/example/fake\" = \"v1.0.0\"\n"
        ),
    )
    .unwrap();
    let source = "fn main() {}\n";
    let file = root.join("src/main.lis");
    fs::write(&file, source).unwrap();
    let uri = Url::from_file_path(&file).unwrap();
    let executable = root.join("bin/go");
    let log = root.join("go-invocations");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$LSP_GO_LOG\"\nexit 127\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let path = env::join_paths(
        iter::once(root.join("bin"))
            .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lis"))
        .arg("lsp")
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("PATH", path)
        .env("LSP_GO_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut writer = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        while let Ok(Some(message)) = read_message(&mut reader) {
            if sender.send(message).is_err() {
                break;
            }
        }
    });
    let wait = |predicate: &dyn Fn(&Value) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let message = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if predicate(&message) {
                return message;
            }
        }
    };
    let outcome = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        write_message(
            &mut writer,
            &json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{"capabilities":{}}}),
        )
        .unwrap();
        wait(&|message| message["id"] == 1);
        write_message(
            &mut writer,
            &json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}),
        )
        .unwrap();
        write_message(
            &mut writer,
            &json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{
                "textDocument":{"uri":uri, "languageId":"lisette", "version":1, "text":source}
            }}),
        )
        .unwrap();
        wait(&|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["version"] == 1
        });
        assert!(!log.exists());
        let changed = "import \"go:github.com/example/fake\"\nfn main() { fake.DoStuff() }\n";
        write_message(
            &mut writer,
            &json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{
                "textDocument":{"uri":uri, "version":2}, "contentChanges":[{"text":changed}]
            }}),
        )
        .unwrap();
        wait(&|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["version"] == 2
        });
        for (id, method) in [(2, "textDocument/hover"), (3, "textDocument/completion")] {
            write_message(
                &mut writer,
                &json!({"jsonrpc":"2.0", "id":id, "method":method, "params":{
                    "textDocument":{"uri":uri}, "position":{"line":1, "character":15}
                }}),
            )
            .unwrap();
            wait(&|message| message["id"] == id);
        }
        assert!(
            !log.exists(),
            "typing and querying must use cached typedefs only"
        );
        fs::write(&file, changed).unwrap();
        write_message(&mut writer, &json!({"jsonrpc":"2.0", "method":"textDocument/didSave", "params":{"textDocument":{"uri":uri}}})).unwrap();
        wait(&|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["version"] == 2
        });
        assert!(
            fs::read_to_string(&log).unwrap().contains("version"),
            "saving may explicitly prepare missing dependencies"
        );
        write_message(
            &mut writer,
            &json!({"jsonrpc":"2.0", "id":4, "method":"shutdown"}),
        )
        .unwrap();
        wait(&|message| message["id"] == 4);
        write_message(&mut writer, &json!({"jsonrpc":"2.0", "method":"exit"})).unwrap();
    }));
    drop(writer);
    if outcome.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    if let Err(panic) = outcome {
        panic::resume_unwind(panic);
    }
    assert!(status.success());
}
