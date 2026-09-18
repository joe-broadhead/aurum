//! Process-level `--stdio` contract (no microphone, no model download).

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn spawn_stdio() -> (tempfile::TempDir, std::process::Child) {
    let home = tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_aurum"))
        .args(["converse", "--stdio", "--local-only"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .spawn()
        .expect("spawn aurum converse --stdio");
    (home, child)
}

fn read_json(stdout: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read stdout line");
    serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("json {e}: {line}"))
}

#[test]
fn stdio_ready_rejects_relative_transcribe_then_shutdown() {
    let (_home, mut child) = spawn_stdio();
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let ready = read_json(&mut stdout);
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["v"], 1);
    let caps = ready["caps"].as_array().expect("caps");
    assert!(caps.iter().any(|c| c == "transcribe"));
    assert!(caps.iter().any(|c| c == "synthesize"));

    writeln!(
        stdin,
        r#"{{"v":1,"type":"transcribe","path":"relative.wav"}}"#
    )
    .unwrap();
    let err = read_json(&mut stdout);
    assert_eq!(err["type"], "error");
    assert_eq!(err["category"], "user");
    assert!(
        err["message"].as_str().unwrap_or("").contains("absolute"),
        "{}",
        err
    );

    writeln!(stdin, r#"{{"v":1,"type":"speak","text":"Hello."}}"#).unwrap();
    let speak_err = read_json(&mut stdout);
    assert_eq!(speak_err["type"], "error");
    assert_eq!(speak_err["category"], "user");
    assert!(speak_err["message"]
        .as_str()
        .unwrap_or("")
        .contains("synthesize"));

    writeln!(stdin, r#"{{"v":1,"type":"shutdown"}}"#).unwrap();
    let shut = read_json(&mut stdout);
    assert_eq!(shut["type"], "shutdown");
    drop(stdin);
    let status = child.wait().expect("wait");
    assert!(status.success(), "exit {status}");
}
