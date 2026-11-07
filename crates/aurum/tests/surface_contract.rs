//! Process-level contract for the machine surface (A4, A5, A7, A10/#10).
//!
//! These run the real binary with an isolated HOME and a private cache root, so
//! they never read the operator's config, models or cache. No model download and
//! no network: every command here is static.

use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

fn run(args: &[&str], home: &std::path::Path, envs: &[(&str, &str)]) -> (bool, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aurum"));
    cmd.args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("AURUM_CACHE");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run aurum");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn parse(stdout: &str) -> Value {
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("not JSON ({e}):\n{stdout}"))
}

#[test]
fn capabilities_covers_the_host_contract() {
    let home = tempdir().unwrap();
    let cache = home.path().join("cache");
    let cache_s = cache.display().to_string();
    let (ok, stdout, stderr) = run(
        &["capabilities", "--json"],
        home.path(),
        &[("AURUM_CACHE", &cache_s)],
    );
    assert!(ok, "capabilities failed:\n{stderr}");
    let doc = parse(&stdout);

    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["protocol_version"], 1);
    assert_eq!(doc["cache_root"], cache_s);
    assert_eq!(doc["caps"]["stdio_sidecar"], true);
    assert_eq!(doc["caps"]["long_audio"], true);
    assert_eq!(doc["caps"]["synthesize_speaking_rate"], true);
    assert_eq!(doc["caps"]["synthesize_abort"], true);
    assert_eq!(doc["caps"]["record"], true);
    assert_eq!(doc["caps"]["play"], true);
    assert_eq!(doc["models"]["partials"], Value::Null);
    // Without --mic the sidecar does not advertise `speak`.
    let sidecar_caps = doc["sidecar"]["caps"].as_array().expect("sidecar caps");
    assert!(
        !sidecar_caps.iter().any(|c| c == "speak"),
        "{sidecar_caps:?}"
    );
    assert!(sidecar_caps.iter().any(|c| c == "long_audio"));
    // Silence must not carry a provider key.
    assert!(!stdout.contains("api_key"));
}

#[test]
fn cache_dir_flag_and_env_agree() {
    let home = tempdir().unwrap();
    let flag_cache = home.path().join("by-flag");
    let env_cache = home.path().join("by-env");

    let (ok, flag_out, err) = run(
        &[
            "models",
            "--json",
            "--cache-dir",
            &flag_cache.display().to_string(),
        ],
        home.path(),
        &[("AURUM_CACHE", &env_cache.display().to_string())],
    );
    assert!(ok, "models --cache-dir failed:\n{err}");
    assert_eq!(
        parse(&flag_out)["cache_root"],
        flag_cache.display().to_string()
    );

    let (ok, env_out, err) = run(
        &["models", "--json"],
        home.path(),
        &[("AURUM_CACHE", &env_cache.display().to_string())],
    );
    assert!(ok, "models with AURUM_CACHE failed:\n{err}");
    assert_eq!(
        parse(&env_out)["cache_root"],
        env_cache.display().to_string()
    );
}

#[test]
fn models_json_names_licence_pins_and_cache_state() {
    let home = tempdir().unwrap();
    let cache = home.path().join("cache");
    let (ok, stdout, err) = run(
        &[
            "models",
            "--json",
            "--cache-dir",
            &cache.display().to_string(),
        ],
        home.path(),
        &[],
    );
    assert!(ok, "models --json failed:\n{err}");
    let doc = parse(&stdout);
    let models = doc["models"].as_array().expect("models");
    assert!(models.len() >= 10);
    let tiny = models
        .iter()
        .find(|m| m["id"] == "tiny-q5_1")
        .expect("tiny-q5_1");
    assert_eq!(tiny["cached"], false, "a fresh cache must be empty");
    assert!(tiny["bytes"].as_u64().unwrap() > 0);
    assert_eq!(tiny["sha256"].as_str().unwrap().len(), 64);
    assert!(tiny["licence"].as_str().unwrap().contains("MIT"));
    assert_eq!(tiny["revision"].as_str().unwrap().len(), 40);
    assert!(tiny["files"][0]["path"]
        .as_str()
        .unwrap()
        .contains("ggml-tiny-q5_1.bin"));
}

#[test]
fn tts_models_json_does_not_load_the_runtime() {
    let home = tempdir().unwrap();
    let cache = home.path().join("cache");
    let (ok, stdout, err) = run(
        &[
            "tts",
            "models",
            "--json",
            "--cache-dir",
            &cache.display().to_string(),
        ],
        home.path(),
        &[],
    );
    assert!(ok, "tts models --json failed:\n{err}");
    let doc = parse(&stdout);
    assert_eq!(doc["direction"], "tts");
    let models = doc["models"].as_array().expect("tts models");
    assert!(models
        .iter()
        .any(|m| m["id"] == "kitten-nano-int8" && m["cached"] == false));
    assert!(models[0]["licence"].as_str().is_some_and(|l| !l.is_empty()));
}

#[test]
fn doctor_json_publishes_the_stable_surface() {
    let home = tempdir().unwrap();
    let cache = home.path().join("cache");
    let (ok, stdout, err) = run(
        &[
            "doctor",
            "--json",
            "--cache-dir",
            &cache.display().to_string(),
        ],
        home.path(),
        &[],
    );
    // A machine with no audio device warns but stays healthy.
    assert!(ok, "doctor --json failed:\n{err}\n{stdout}");
    let doc = parse(&stdout);
    assert_eq!(doc["schema_version"], 1);
    for key in [
        "engine",
        "platform",
        "models",
        "audio",
        "microphone",
        "checks",
    ] {
        assert!(doc.get(key).is_some(), "doctor JSON missing {key}");
    }
    assert_eq!(doc["engine"]["protocol_version"], 1);
    assert_eq!(doc["audio"]["stt_sample_rate_hz"], 16000);
    assert_eq!(doc["models"]["cache_dir"], cache.display().to_string());
    assert!(doc["platform"]["os"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert!(doc["microphone"]["probed"].as_bool().unwrap());
}
