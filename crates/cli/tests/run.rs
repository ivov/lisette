//! Exercise `lis run` as a real process, which signals reach by PID.
#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

const SIGTERM: i32 = 15;

#[test]
fn stopping_lis_run_stops_the_program() {
    if Command::new("go").arg("version").output().is_err() {
        eprintln!("skipping stopping_lis_run_stops_the_program: `go` not found");
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("spin.lis");
    fs::write(
        &script,
        r#"import "go:fmt"
import "go:os"

fn main() {
  fmt.Println(os.Getpid())
  let mut n = 0
  while true {
    n += 1
  }
}
"#,
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_lis"))
        .arg("run")
        .arg(&script)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let program_pid: u32 = line.trim().parse().unwrap_or_else(|_| {
        let _ = child.kill();
        panic!("expected the program to print its PID, got {line:?}");
    });

    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = child.wait().unwrap();

    let program_alive = Command::new("kill")
        .args(["-0", &program_pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();
    if program_alive {
        let _ = Command::new("kill")
            .args(["-KILL", &program_pid.to_string()])
            .status();
    }

    assert_eq!(
        program_pid,
        child.id(),
        "the program must run in the `lis` process, not in a child"
    );
    assert!(!program_alive, "the program must stop with `lis`");
    assert_eq!(status.signal(), Some(SIGTERM));
}
