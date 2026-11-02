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

/// Write a 16 kHz mono 16-bit PCM WAV of `secs` seconds (quiet tone).
#[cfg(unix)]
fn write_wav_16k(path: &std::path::Path, secs: u32) {
    let rate: u32 = 16_000;
    let n = rate * secs;
    let data_len = n * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        let s = (((i % 40) as i16) - 20) * 100;
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

// `transcribe` takes a host-owned recording; it must not be capped by the
// 30 s `--mic` live buffer. Config is isolated through HOME/XDG (Unix only,
// see above).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_transcribe_accepts_audio_longer_than_live_buffer() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "text": "forty seconds of speech" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let home = tempdir().unwrap();
    let config = format!(
        "[stt]\nprovider = \"openai\"\nmodel = \"whisper-1\"\nlanguage = \"en\"\n\n\
         [providers.openai]\nbase_url = \"{}\"\n",
        server.uri()
    );
    // directories::ProjectDirs: macOS uses ~/Library/Application Support,
    // other Unix uses $XDG_CONFIG_HOME.
    for dir in [
        home.path().join("Library/Application Support/aurum"),
        home.path().join("config/aurum"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), &config).unwrap();
    }
    let wav = home.path().join("long.wav");
    write_wav_16k(&wav, 40);

    let mut command = Command::new(env!("CARGO_BIN_EXE_aurum"));
    command
        .args(["converse", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("OPENAI_API_KEY", "sk-test");
    for key in ["OPENROUTER_API_KEY", "ELEVENLABS_API_KEY", "XAI_API_KEY"] {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("AURUM_") {
            command.env_remove(key);
        }
    }
    // Keep stdin open until the transcript arrives: EOF and `shutdown` cancel
    // in-flight work by design.
    let output = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        let mut child = command.spawn().expect("spawn aurum converse --stdio");
        let mut stdin = child.stdin.take().expect("stdin");
        let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
        let mut events: Vec<Value> = Vec::new();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({ "v": 1, "type": "transcribe", "path": wav })
        )
        .unwrap();
        stdin.flush().unwrap();
        for line in lines.by_ref() {
            let line = line.expect("read stdout");
            let ev: Value =
                serde_json::from_str(&line).unwrap_or_else(|e| panic!("json {e}: {line}"));
            let done = ev["type"] == "user_final" || ev["type"] == "error";
            events.push(ev);
            if done {
                break;
            }
        }
        writeln!(stdin, r#"{{"v":1,"type":"shutdown"}}"#).unwrap();
        stdin.flush().unwrap();
        drop(stdin);
        for line in lines {
            let line = line.expect("read stdout");
            events.push(serde_json::from_str(&line).unwrap_or_else(|e| panic!("json {e}: {line}")));
        }
        (child.wait().expect("wait"), events)
    })
    .await
    .unwrap();
    let (status, events) = output;
    assert!(status.success(), "exit {status}\nevents: {events:?}");
    let final_ev = events
        .iter()
        .find(|e| e["type"] == "user_final")
        .unwrap_or_else(|| panic!("no user_final: {events:?}"));
    assert_eq!(final_ev["text"], "forty seconds of speech");
    assert_eq!(final_ev["provider"], "openai");
    assert!(
        !events.iter().any(|e| e["type"] == "error"),
        "unexpected error: {events:?}"
    );
    assert!(events.iter().any(|e| e["type"] == "shutdown"), "{events:?}");
}

// `--speaking-rate` sets the session default; `synthesize.speaking_rate` overrides it per
// request. ElevenLabs is mocked so the applied rate is visible on the wire.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_synthesize_rate_overrides_session_speaking_rate() {
    use wiremock::matchers::{body_partial_json, method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let pcm: Vec<u8> = (0..2_400i16)
        .flat_map(|i| ((i % 30) * 15).to_le_bytes())
        .collect();
    let server = MockServer::start().await;
    for speed in [1.1, 0.9] {
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/text-to-speech/[^/]+$"))
            .and(body_partial_json(
                serde_json::json!({ "voice_settings": { "speed": speed } }),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "audio/pcm")
                    .set_body_bytes(pcm.clone()),
            )
            .expect(1)
            .mount(&server)
            .await;
    }

    let home = tempdir().unwrap();
    let config = format!(
        "[tts]\nprovider = \"elevenlabs\"\nmodel = \"eleven_flash_v2_5\"\n\
         voice = \"21m00Tcm4TlvDq8ikWAM\"\n\n[providers.elevenlabs]\nbase_url = \"{}\"\n",
        server.uri()
    );
    // directories::ProjectDirs: macOS uses ~/Library/Application Support,
    // other Unix uses $XDG_CONFIG_HOME.
    for dir in [
        home.path().join("Library/Application Support/aurum"),
        home.path().join("config/aurum"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), &config).unwrap();
    }
    let fast = home.path().join("fast.wav");
    let default_rate = home.path().join("default.wav");

    let mut command = Command::new(env!("CARGO_BIN_EXE_aurum"));
    command
        .args(["converse", "--stdio", "--speaking-rate", "0.9"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("ELEVENLABS_API_KEY", "el-test");
    for key in ["OPENROUTER_API_KEY", "OPENAI_API_KEY", "XAI_API_KEY"] {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("AURUM_") {
            command.env_remove(key);
        }
    }
    let requests = [
        serde_json::json!({ "v": 1, "type": "synthesize", "text": "Fast.", "path": fast, "speaking_rate": 1.1 }),
        serde_json::json!({ "v": 1, "type": "synthesize", "text": "Default.", "path": default_rate }),
    ];
    // One request at a time; keep stdin open until each result arrives
    // (EOF and `shutdown` cancel in-flight work by design).
    let (status, events) = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        let mut child = command.spawn().expect("spawn aurum converse --stdio");
        let mut stdin = child.stdin.take().expect("stdin");
        let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
        let mut next = || -> Value {
            let line = lines.next().expect("stdout closed").expect("read stdout");
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("json {e}: {line}"))
        };
        let mut events = vec![next()];
        for req in requests {
            writeln!(stdin, "{req}").unwrap();
            stdin.flush().unwrap();
            events.push(next());
        }
        writeln!(stdin, r#"{{"v":1,"type":"shutdown"}}"#).unwrap();
        stdin.flush().unwrap();
        events.push(next());
        drop(stdin);
        (child.wait().expect("wait"), events)
    })
    .await
    .unwrap();

    assert!(status.success(), "exit {status}\nevents: {events:?}");
    assert_eq!(events[0]["type"], "ready", "{events:?}");
    let caps = events[0]["caps"].as_array().expect("caps");
    assert!(
        caps.iter().any(|c| c == "synthesize_speaking_rate"),
        "{caps:?}"
    );
    for ev in &events[1..3] {
        assert_eq!(ev["type"], "synthesized", "{events:?}");
        assert_eq!(ev["provider"], "elevenlabs", "{events:?}");
    }
    assert_eq!(events[3]["type"], "shutdown", "{events:?}");
    assert!(fast.is_file() && default_rate.is_file());
    // MockServer verifies each `.expect(1)` on drop.
}
