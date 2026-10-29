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

// ProjectDirs honors HOME/XDG on Unix. Windows uses OS known-folder paths,
// which cannot be isolated by changing a child process's environment.
#[cfg(unix)]
#[test]
fn stdio_missing_tts_credentials_names_selected_provider() {
    use std::io::{BufRead, BufReader};
    use std::process::Child;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    struct KillOnDrop(Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let timeout = Duration::from_secs(15);
    for (provider, env_key, voice) in [
        ("elevenlabs", "ELEVENLABS_API_KEY", "21m00Tcm4TlvDq8ikWAM"),
        ("openai", "OPENAI_API_KEY", "alloy"),
        ("xai", "XAI_API_KEY", "eve"),
    ] {
        let home = tempdir().unwrap();
        let output_path = home.path().join("speech.wav");
        let mut command = Command::new(env!("CARGO_BIN_EXE_aurum"));
        command
            .args([
                "converse",
                "--stdio",
                "--provider",
                "local",
                "--tts-provider",
                provider,
                "--voice",
                voice,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"));
        for key in [
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
            "ELEVENLABS_API_KEY",
            "XAI_API_KEY",
        ] {
            command.env_remove(key);
        }
        // Model/voice/provider overrides must not escape this isolated setup.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("AURUM_") {
                command.env_remove(key);
            }
        }
        let mut child = KillOnDrop(command.spawn().expect("spawn credential-error sidecar"));
        let stdout = child.0.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let next_event = || -> Value {
            let line = rx
                .recv_timeout(timeout)
                .unwrap_or_else(|e| panic!("{provider}: timed out or lost stdout: {e}"))
                .expect("read stdout");
            serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("{provider}: invalid event {line:?}: {e}"))
        };

        let ready = next_event();
        assert_eq!(ready["type"], "ready", "{provider}: {ready}");
        assert_eq!(ready["tts_provider"], provider);
        let stdin = child.0.stdin.as_mut().expect("stdin");
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "v": 1,
                "type": "synthesize",
                "text": "Hello.",
                "path": output_path,
            })
        )
        .unwrap();
        stdin.flush().unwrap();

        let error = next_event();
        assert_eq!(error["type"], "error", "{provider}: {error}");
        assert_eq!(error["category"], "user", "{provider}: {error}");
        let message = error["message"].as_str().expect("error message");
        assert!(message.contains(provider), "{provider}: {message}");
        assert!(message.contains(env_key), "{provider}: {message}");
        assert!(
            message.contains(&format!("[providers.{provider}]")),
            "{provider}: {message}"
        );
        assert!(
            !message.to_ascii_lowercase().contains("openrouter"),
            "{provider}: {message}"
        );
        assert!(!output_path.exists(), "{provider}: wrote unexpected audio");

        // Wait for the error before shutdown: the stdin reader eagerly cancels
        // operations on shutdown, which would otherwise race this assertion.
        let stdin = child.0.stdin.as_mut().expect("stdin");
        writeln!(stdin, r#"{{"v":1,"type":"shutdown"}}"#).unwrap();
        stdin.flush().unwrap();
        let shutdown = next_event();
        assert_eq!(shutdown["type"], "shutdown", "{provider}: {shutdown}");
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.0.try_wait().expect("poll sidecar exit") {
                assert!(status.success(), "{provider}: sidecar exit {status}");
                break;
            }
            assert!(Instant::now() < deadline, "{provider}: shutdown timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
