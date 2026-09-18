//! Process-level `--stdio` contract (no microphone, no model download).

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

#[test]
fn stdio_ready_rejects_relative_transcribe_then_shutdown() {
    let home = tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_aurum"))
        .args(["converse", "--stdio", "--local-only"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .spawn()
        .expect("spawn aurum converse --stdio");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        writeln!(
            stdin,
            r#"{{"v":1,"type":"transcribe","path":"relative.wav"}}"#
        )
        .unwrap();
        writeln!(stdin, r#"{{"v":1,"type":"speak","text":"Hello."}}"#).unwrap();
        writeln!(stdin, r#"{{"v":1,"type":"shutdown"}}"#).unwrap();
        stdin.flush().unwrap();
    }

    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let events: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("json {e}: {l}")))
        .collect();
    assert!(
        !events.is_empty(),
        "no JSON events\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(events[0]["type"], "ready", "{events:?}");
    assert_eq!(events[0]["v"], 1);
    let caps = events[0]["caps"].as_array().expect("caps");
    assert!(caps.iter().any(|c| c == "transcribe"));
    assert!(caps.iter().any(|c| c == "synthesize"));

    let err = events
        .iter()
        .find(|e| e["type"] == "error" && e["message"].as_str().unwrap_or("").contains("absolute"))
        .expect("absolute-path error");
    assert_eq!(err["category"], "user");

    let speak_err = events
        .iter()
        .find(|e| {
            e["type"] == "error" && e["message"].as_str().unwrap_or("").contains("synthesize")
        })
        .expect("speak-without-mic error");
    assert_eq!(speak_err["category"], "user");

    assert!(
        events.iter().any(|e| e["type"] == "shutdown"),
        "missing shutdown: {events:?}"
    );
}
