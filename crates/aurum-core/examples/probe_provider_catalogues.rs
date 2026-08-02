//! Provider catalogue probe (JOE-2213).
//!
//! Dumps reviewed static registries and optionally compares defaults against
//! live vendor catalogues. **Never prints API keys or full secret-bearing URLs.**
//!
//! ```bash
//! # Offline (always safe in CI):
//! cargo run -p aurum-core --example probe_provider_catalogues -- --offline
//!
//! # Live catalogue probe (requires keys; list endpoints only — no audio/text synth):
//! OPENROUTER_API_KEY=… cargo run -p aurum-core --example probe_provider_catalogues -- --live
//! OPENAI_API_KEY=…    cargo run -p aurum-core --example probe_provider_catalogues -- --live
//!
//! # Write markdown matrix:
//! cargo run -p aurum-core --example probe_provider_catalogues -- \
//!   --offline --out dist/provider-catalogue/PROBE_REPORT.md
//! ```

use aurum_core::capabilities::OPENROUTER_STT_REGISTRY;
use aurum_core::providers::{
    lookup_elevenlabs_tts, lookup_openai_stt, lookup_openai_tts, lookup_openrouter_tts,
    lookup_xai_stt, lookup_xai_tts, DEFAULT_ELEVENLABS_TTS_MODEL, DEFAULT_OPENAI_STT_MODEL,
    DEFAULT_OPENAI_TTS_MODEL, DEFAULT_OPENROUTER_TTS_MODEL, DEFAULT_XAI_STT_MODEL,
    DEFAULT_XAI_TTS_MODEL, ELEVENLABS_TTS_REGISTRY, OPENAI_STT_REGISTRY, OPENAI_TTS_REGISTRY,
    OPENROUTER_TTS_REGISTRY, XAI_STT_REGISTRY, XAI_TTS_REGISTRY,
};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
struct Row {
    surface: String,
    id: String,
    role: String, // default | reviewed
    static_ok: bool,
    live: String, // pass | fail | skip | n/a
    note: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let live = args.iter().any(|a| a == "--live");
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let mut rows: Vec<Row> = Vec::new();
    collect_static(&mut rows);

    let mut live_failures = 0usize;
    if live {
        match probe_openrouter_live(&mut rows).await {
            Ok(n) => live_failures += n,
            Err(e) => {
                eprintln!("aurum: openrouter live probe error: {e}");
                live_failures += 1;
            }
        }
        match probe_openai_live(&mut rows).await {
            Ok(n) => live_failures += n,
            Err(e) => {
                eprintln!("aurum: openai live probe error: {e}");
                live_failures += 1;
            }
        }
    } else {
        for r in rows.iter_mut() {
            if r.live == "n/a" {
                r.live = "skip".into();
                r.note = if r.note.is_empty() {
                    "offline mode (pass --live with keys)".into()
                } else {
                    format!("{}; offline (pass --live with keys)", r.note)
                };
            }
        }
    }

    let default_static_ok = rows
        .iter()
        .filter(|r| r.role == "default")
        .all(|r| r.static_ok);
    if !default_static_ok {
        eprintln!("aurum: FAIL — a product default is missing from its reviewed registry");
    }

    let md = render_markdown(&rows, live);
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&path, &md) {
            eprintln!("aurum: write {}: {e}", path.display());
            return ExitCode::from(2);
        }
        eprintln!("aurum: wrote {}", path.display());
    }
    print!("{md}");

    if !default_static_ok {
        return ExitCode::from(1);
    }
    if live && live_failures > 0 {
        eprintln!("aurum: FAIL — {live_failures} live default/catalogue check(s) failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn collect_static(rows: &mut Vec<Row>) {
    push_default(
        rows,
        "openrouter.tts",
        DEFAULT_OPENROUTER_TTS_MODEL,
        lookup_openrouter_tts(DEFAULT_OPENROUTER_TTS_MODEL).is_some(),
    );
    push_default(
        rows,
        "openai.stt",
        DEFAULT_OPENAI_STT_MODEL,
        lookup_openai_stt(DEFAULT_OPENAI_STT_MODEL).is_some(),
    );
    push_default(
        rows,
        "openai.tts",
        DEFAULT_OPENAI_TTS_MODEL,
        lookup_openai_tts(DEFAULT_OPENAI_TTS_MODEL).is_some(),
    );
    push_default(
        rows,
        "elevenlabs.tts",
        DEFAULT_ELEVENLABS_TTS_MODEL,
        lookup_elevenlabs_tts(DEFAULT_ELEVENLABS_TTS_MODEL).is_some(),
    );
    push_default(
        rows,
        "xai.stt",
        DEFAULT_XAI_STT_MODEL,
        lookup_xai_stt(DEFAULT_XAI_STT_MODEL).is_some(),
    );
    push_default(
        rows,
        "xai.tts",
        DEFAULT_XAI_TTS_MODEL,
        lookup_xai_tts(DEFAULT_XAI_TTS_MODEL).is_some(),
    );

    for r in OPENROUTER_TTS_REGISTRY {
        if r.model == DEFAULT_OPENROUTER_TTS_MODEL {
            continue;
        }
        rows.push(Row {
            surface: "openrouter.tts".into(),
            id: r.model.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: format!("tier={:?}", r.tier),
        });
    }
    for r in OPENROUTER_STT_REGISTRY {
        rows.push(Row {
            surface: "openrouter.stt".into(),
            id: r.model_id.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: format!("path={:?}", r.path),
        });
    }
    for r in OPENAI_STT_REGISTRY {
        if r.model == DEFAULT_OPENAI_STT_MODEL {
            continue;
        }
        rows.push(Row {
            surface: "openai.stt".into(),
            id: r.model.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: String::new(),
        });
    }
    for r in OPENAI_TTS_REGISTRY {
        if r.model == DEFAULT_OPENAI_TTS_MODEL {
            continue;
        }
        rows.push(Row {
            surface: "openai.tts".into(),
            id: r.model.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: String::new(),
        });
    }
    for r in ELEVENLABS_TTS_REGISTRY {
        if r.model == DEFAULT_ELEVENLABS_TTS_MODEL {
            continue;
        }
        rows.push(Row {
            surface: "elevenlabs.tts".into(),
            id: r.model.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: String::new(),
        });
    }
    for r in XAI_STT_REGISTRY {
        if r.model == DEFAULT_XAI_STT_MODEL {
            continue;
        }
        rows.push(Row {
            surface: "xai.stt".into(),
            id: r.model.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: String::new(),
        });
    }
    for r in XAI_TTS_REGISTRY {
        if r.model == DEFAULT_XAI_TTS_MODEL {
            continue;
        }
        rows.push(Row {
            surface: "xai.tts".into(),
            id: r.model.into(),
            role: "reviewed".into(),
            static_ok: true,
            live: "n/a".into(),
            note: String::new(),
        });
    }
}

fn push_default(rows: &mut Vec<Row>, surface: &str, id: &str, static_ok: bool) {
    rows.push(Row {
        surface: surface.into(),
        id: id.into(),
        role: "default".into(),
        static_ok,
        live: "n/a".into(),
        note: if static_ok {
            "product default".into()
        } else {
            "MISSING FROM REGISTRY — demote/replace before ship".into()
        },
    });
}

async fn probe_openrouter_live(rows: &mut [Row]) -> Result<usize, String> {
    let key = env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "OPENROUTER_API_KEY not set".to_string())?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let speech_ids = fetch_openrouter_model_ids(
        &client,
        &key,
        "https://openrouter.ai/api/v1/models?output_modalities=speech",
    )
    .await?;
    let mut fails = 0usize;
    fails += mark_live(
        rows,
        "openrouter.tts",
        DEFAULT_OPENROUTER_TTS_MODEL,
        &speech_ids,
        "speech catalogue",
    );

    let all_ids =
        fetch_openrouter_model_ids(&client, &key, "https://openrouter.ai/api/v1/models").await?;
    for r in rows.iter_mut().filter(|r| r.surface == "openrouter.stt") {
        if all_ids.iter().any(|id| id.eq_ignore_ascii_case(&r.id)) {
            r.live = "pass".into();
            r.note = "present in OpenRouter models list".into();
        } else {
            r.live = "fail".into();
            r.note = "not listed in OpenRouter models API".into();
        }
    }

    if speech_ids.is_empty() {
        eprintln!(
            "aurum: OpenRouter speech catalogue empty — check https://openrouter.ai/settings/privacy"
        );
        fails += 1;
    }
    Ok(fails)
}

async fn fetch_openrouter_model_ids(
    client: &reqwest::Client,
    key: &str,
    url: &str,
) -> Result<Vec<String>, String> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://github.com/joe-broadhead/aurum")
        .header("X-OpenRouter-Title", "Aurum")
        .send()
        .await
        .map_err(|e| redact_net(&e))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let hint = if body.contains("guardrail") || body.contains("privacy") {
            " (privacy/guardrail — https://openrouter.ai/settings/privacy)"
        } else {
            ""
        };
        return Err(format!("HTTP {status}{hint}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("json parse: {e}"))?;
    let mut ids = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

async fn probe_openai_live(rows: &mut [Row]) -> Result<usize, String> {
    let key = env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let Some(key) = key else {
        for r in rows.iter_mut().filter(|r| r.surface.starts_with("openai.")) {
            r.live = "skip".into();
            r.note = "OPENAI_API_KEY not set".into();
        }
        return Ok(0);
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(&key)
        .send()
        .await
        .map_err(|e| redact_net(&e))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("json parse: {e}"))?;
    let mut ids = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                ids.push(id.to_string());
            }
        }
    }

    let mut fails = 0usize;
    fails += mark_live(
        rows,
        "openai.stt",
        DEFAULT_OPENAI_STT_MODEL,
        &ids,
        "openai models list",
    );
    fails += mark_live(
        rows,
        "openai.tts",
        DEFAULT_OPENAI_TTS_MODEL,
        &ids,
        "openai models list",
    );
    for r in rows
        .iter_mut()
        .filter(|r| r.surface.starts_with("openai.") && r.role == "reviewed")
    {
        if ids.iter().any(|id| id.eq_ignore_ascii_case(&r.id)) {
            r.live = "pass".into();
        } else {
            r.live = "fail".into();
            r.note = "not in OpenAI models list (may still work; investigate)".into();
        }
    }
    Ok(fails)
}

fn mark_live(
    rows: &mut [Row],
    surface: &str,
    default_id: &str,
    live_ids: &[String],
    source: &str,
) -> usize {
    let mut fails = 0usize;
    for r in rows.iter_mut().filter(|r| r.surface == surface) {
        let present = live_ids.iter().any(|id| id.eq_ignore_ascii_case(&r.id));
        if r.id == default_id || r.role == "default" {
            if present {
                r.live = "pass".into();
                r.note = format!("default present in {source}");
            } else {
                r.live = "fail".into();
                r.note = format!("DEFAULT missing from {source} — demote/replace before ship");
                fails += 1;
            }
        } else if surface == "openrouter.tts" {
            if present {
                r.live = "pass".into();
                r.note = format!("present in {source}");
            } else {
                r.live = "fail".into();
                r.note = format!("not in {source}");
            }
        }
    }
    fails
}

fn redact_net(err: &reqwest::Error) -> String {
    let _ = err;
    "network error (details redacted)".into()
}

fn render_markdown(rows: &[Row], live: bool) -> String {
    let mut out = String::new();
    out.push_str("# Provider catalogue probe (JOE-2213)\n\n");
    out.push_str(&format!(
        "- **Mode:** {}\n",
        if live { "live" } else { "offline" }
    ));
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    out.push_str(&format!("- **UTC unix_ts:** {secs}\n"));
    out.push_str(
        "- **Privacy:** OpenRouter live probes require account privacy/guardrails that allow listed models — https://openrouter.ai/settings/privacy\n",
    );
    out.push_str("- **Secrets:** keys never written; network errors redacted.\n\n");
    out.push_str("| Surface | Id | Role | Static | Live | Note |\n");
    out.push_str("|---------|----|------|--------|------|------|\n");
    for r in rows {
        out.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} |\n",
            r.surface,
            r.id,
            r.role,
            if r.static_ok { "ok" } else { "FAIL" },
            r.live,
            r.note.replace('|', "/")
        ));
    }
    out.push_str(
        "\n## Operator actions\n\n\
         1. If a **default** row is static FAIL → remove/replace constant in code before release.\n\
         2. If a **default** live FAIL → open demotion PR (do not ship dead default).\n\
         3. Reviewed live FAIL → investigate; demote from registry if upstream removed.\n\
         4. OpenRouter empty speech list → fix privacy settings, then re-probe.\n\
         5. OpenRouter TTS remains **experimental** until protected smoke (JOE-1978).\n",
    );
    out
}
