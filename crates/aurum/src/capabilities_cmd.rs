//! `aurum capabilities --json` — the one document a host reads to know what
//! this build can do (#10).
//!
//! oc-aurum had to infer Aurum's state from prose and probe for invented
//! capability names. Everything a host needs is here: version, protocol,
//! platform, commands, sidecar capabilities, cache root and the default models.
//! It is static and cheap: no model load, no network, no device access.

use aurum_core::catalogue::Direction;
use aurum_core::config::Config;
use serde_json::json;

/// Build the capabilities document for a resolved config.
pub fn capabilities_json(cfg: &Config) -> serde_json::Value {
    let stt_models: Vec<_> = cfg
        .catalogue
        .records()
        .iter()
        .filter(|r| r.record.direction == Direction::Stt && r.record.enabled)
        .map(|r| r.record.id.clone())
        .collect();
    // The CLI always links aurum-core with `tts`; no cfg gate is needed here.
    let tts_models: Vec<_> = aurum_core::tts::catalogue::MODELS
        .iter()
        .filter(|m| m.shipped)
        .map(|m| m.id.to_string())
        .collect();

    json!({
        "schema_version": aurum_core::SURFACE_SCHEMA_VERSION,
        "name": "aurum",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol_version": aurum_core::PROTOCOL_VERSION,
        "platform": aurum_core::PlatformInfo::current(),
        "cache_root": cfg.cache_dir.display().to_string(),
        "config_path": cfg.config_path.as_ref().map(|p| p.display().to_string()),
        "defaults": {
            "stt_model": cfg.default_local_stt_model(),
            "stt_provider": cfg.provider,
            "tts_model": cfg.tts_model,
            "tts_voice": cfg.tts_voice,
            "language": cfg.language,
        },
        "commands": [
            "transcribe",
            "cleanup",
            "tts",
            "batch",
            "converse",
            "record",
            "play",
            "models",
            "capabilities",
            "cache",
            "doctor",
            "support-bundle",
            "completions",
            "man",
        ],
        "caps": {
            "stdio_sidecar": true,
            "partials": false,
            "streaming_asr": false,
            "long_audio": true,
            "max_audio_secs": aurum_core::SIDECAR_MAX_AUDIO_SECS,
            "synthesize_speaking_rate": true,
            "synthesize_abort": true,
            "cache_dir_flag": true,
            "cache_env": Config::CACHE_ENV,
            "record": true,
            "play": true,
            "models_json": true,
            "doctor_json": true,
            "capabilities_json": true,
        },
        "sidecar": {
            "protocol_version": aurum_core::PROTOCOL_VERSION,
            "caps": aurum_core::sidecar_caps(false),
            "max_audio_secs": aurum_core::SIDECAR_MAX_AUDIO_SECS,
            "commands": ["transcribe", "synthesize", "speak", "preload", "end_turn", "abort", "shutdown"],
        },
        "outputs": ["txt", "srt", "json", "wav"],
        "models": {
            "stt": stt_models,
            "tts": tts_models,
            "disabled": cfg.catalogue.disabled(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn capabilities_cover_every_command_and_no_secrets() {
        let cfg = Config::load().expect("default config");
        let doc = capabilities_json(&cfg);
        let s = doc.to_string();
        assert!(!s.contains("api_key"));
        assert!(!s.contains("sk-"));

        let declared = doc["commands"].as_array().expect("commands");
        let declared: Vec<&str> = declared.iter().filter_map(|v| v.as_str()).collect();
        let cli = crate::cli::Cli::command();
        for sub in cli.get_subcommands() {
            let name = sub.get_name();
            // `list-models` is a visible alias of `models`; `t`/`flow` likewise.
            let canonical = match name {
                "t" => "transcribe",
                "flow" => "cleanup",
                other => other,
            };
            assert!(
                declared.contains(&canonical),
                "`{name}` is missing from aurum capabilities"
            );
        }
    }
}
