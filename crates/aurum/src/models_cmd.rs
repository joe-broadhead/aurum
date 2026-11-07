//! Machine-readable model inventory and explicit preparation (A5 + #3).
//!
//! `aurum models --json` and `aurum tts models --json` answer "what exists,
//! what is cached, what does it cost, what licence is it" without loading the
//! TTS runtime or touching the network. `aurum models prepare` is the explicit
//! warm step: it downloads if needed, loads the weights and runs one short
//! decode so the first real transcription is not the one paying for graph and
//! shader compilation.

use aurum_core::catalogue::{CatalogueRecord, Direction, Origin};
use aurum_core::config::Config;
use aurum_core::error::{Result, UserError};
use serde_json::json;
use std::path::PathBuf;

/// Schema version for both model inventories.
pub const MODELS_SCHEMA_VERSION: u32 = 1;

const WHISPER_CPP_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

fn stt_record_json(cfg: &Config, record: &CatalogueRecord) -> serde_json::Value {
    let mut files = Vec::new();
    let mut bytes = 0u64;
    let mut sha256 = None;
    let mut url = None;
    let mut revision = None;
    let mut prepared_by = None;

    match &record.origin {
        Some(Origin::DownloadableLocal {
            filename,
            url: source,
            size_bytes,
            sha256: digest,
        }) => {
            bytes = *size_bytes;
            sha256 = Some(digest.clone());
            url = Some(source.clone());
            revision = Some(WHISPER_CPP_REVISION.to_string());
            files.push(json!({
                "filename": filename,
                "path": aurum_core::model::models_dir(&cfg.cache_dir).join(filename).display().to_string(),
                "size_bytes": size_bytes,
                "sha256": digest,
                "cached": aurum_core::model::models_dir(&cfg.cache_dir)
                    .join(filename)
                    .metadata()
                    .map(|m| m.len() == *size_bytes)
                    .unwrap_or(false),
            }));
        }
        Some(Origin::PreparedLocal {
            filename,
            size_bytes,
            sha256: digest,
            source_url,
            revision: rev,
            preparation,
        }) => {
            bytes = *size_bytes;
            sha256 = Some(digest.clone());
            url = Some(source_url.clone());
            revision = Some(rev.clone());
            prepared_by = Some(preparation.clone());
            files.push(json!({
                "filename": filename,
                "path": aurum_core::model::models_dir(&cfg.cache_dir).join(filename).display().to_string(),
                "size_bytes": size_bytes,
                "sha256": digest,
                "cached": aurum_core::model::models_dir(&cfg.cache_dir)
                    .join(filename)
                    .metadata()
                    .map(|m| m.len() == *size_bytes)
                    .unwrap_or(false),
            }));
        }
        Some(Origin::TtsPack { .. }) | None => {}
    }

    let cached = !files.is_empty()
        && files
            .iter()
            .all(|f| f.get("cached").and_then(|c| c.as_bool()).unwrap_or(false));

    json!({
        "id": record.id,
        "aliases": record.aliases,
        "tier": record.tier,
        "languages": record.languages,
        "notes": record.notes,
        "licence": record.license,
        "enabled": cfg.local_model_allowed(&record.id),
        "cached": cached,
        "bytes": bytes,
        "sha256": sha256,
        "url": url,
        "revision": revision,
        "prepared_by": prepared_by,
        "files": files,
    })
}

/// `aurum models --json`.
pub fn stt_models_json(cfg: &Config) -> serde_json::Value {
    let default = cfg.default_local_stt_model();
    let models: Vec<_> = cfg
        .catalogue
        .records()
        .iter()
        .filter(|r| r.record.direction == Direction::Stt && r.record.enabled)
        .map(|r| stt_record_json(cfg, &r.record))
        .collect();
    json!({
        "schema_version": MODELS_SCHEMA_VERSION,
        "direction": "stt",
        "cache_root": cfg.cache_dir.display().to_string(),
        "models_dir": aurum_core::model::models_dir(&cfg.cache_dir).display().to_string(),
        "default_model": default,
        "models": models,
        "disabled": cfg.catalogue.disabled(),
    })
}

/// `aurum tts models --json` — cache state without loading the ONNX runtime.
pub fn tts_models_json(cfg: &Config) -> serde_json::Value {
    let rows = aurum_core::tts::catalogue::list_models(&cfg.cache_dir);
    let models: Vec<_> = rows
        .iter()
        .map(|row| {
            let info = row.info;
            json!({
                "id": info.id,
                "adapter": info.adapter,
                "notes": info.notes,
                "licence": info.license,
                "languages": info.languages,
                "sample_rate_hz": info.sample_rate_hz,
                "bytes": info.onnx.size_bytes + info.voices.size_bytes + info.config.size_bytes,
                "cached": row.cached,
                "path": row.path.display().to_string(),
                "files": [
                    {"role": "onnx", "filename": info.onnx.filename, "bytes": info.onnx.size_bytes, "sha256": info.onnx.sha256},
                    {"role": "voices", "filename": info.voices.filename, "bytes": info.voices.size_bytes, "sha256": info.voices.sha256},
                    {"role": "config", "filename": info.config.filename, "bytes": info.config.size_bytes, "sha256": info.config.sha256},
                ],
                "hf_repo": info.hf_repo,
                "hf_revision": info.hf_revision,
                "shipped": info.shipped,
            })
        })
        .collect();
    let custom = cfg
        .tts_custom_models
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "adapter": m.adapter,
                "pack_dir": m.pack_dir,
                "trust": m.trust,
                "licence": m.license,
                "notes": m.notes,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema_version": MODELS_SCHEMA_VERSION,
        "direction": "tts",
        "cache_root": cfg.cache_dir.display().to_string(),
        "models_dir": aurum_core::tts::catalogue::tts_cache_dir(&cfg.cache_dir).display().to_string(),
        "default_model": cfg.tts_model,
        "default_voice": cfg.tts_voice,
        "models": models,
        "custom_models": custom,
    })
}

/// Resolve the model name `prepare` should warm.
pub fn resolve_prepare_model(cfg: &Config, requested: Option<&str>) -> String {
    requested
        .map(str::to_string)
        .or_else(|| cfg.model.clone())
        .unwrap_or_else(|| cfg.default_local_stt_model().to_string())
}

/// `aurum models prepare [MODEL]`: download, load and warm, then report.
pub async fn prepare_model(
    cfg: &Config,
    requested: Option<&str>,
    json_out: bool,
) -> Result<aurum_core::WarmReport> {
    let model = resolve_prepare_model(cfg, requested);
    let mut owned = cfg.clone();
    owned.model = Some(model.clone());
    let engine = aurum_core::AurumEngine::from_config(owned)?;
    let report = engine.warm_stt(&model).await?;
    engine.shutdown();
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema_version": MODELS_SCHEMA_VERSION,
                "prepared": true,
                "model": report.model,
                "path": report.path,
                "load_ms": report.load_ms,
                "warm_ms": report.warm_ms,
                "total_ms": report.total_ms,
            }))
            .map_err(|e| UserError::Other {
                message: format!("prepare json: {e}"),
            })?
        );
    } else {
        println!(
            "aurum: model `{}` ready ({:.1}s total; {} ms load, {} ms warm decode)",
            report.model,
            report.total_ms as f64 / 1000.0,
            report.load_ms,
            report.warm_ms
        );
        println!("  {}", report.path);
    }
    Ok(report)
}

/// Print a model list as JSON, or the existing human table.
pub fn emit_stt_models(cfg: &Config, json_out: bool) -> Result<()> {
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&stt_models_json(cfg)).map_err(|e| UserError::Other {
                message: format!("models json: {e}"),
            })?
        );
    } else {
        print!(
            "{}",
            aurum_core::model::format_model_list_with(
                &cfg.cache_dir,
                |m| cfg.local_model_allowed(m),
                cfg.default_local_stt_model(),
            )
        );
    }
    Ok(())
}

/// Cache root for a host that asks without a config.
pub fn cache_root_from_env() -> Result<PathBuf> {
    Config::default_cache_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stt_json_names_licence_and_cache_state() {
        let cfg = Config::load().expect("default config");
        let doc = stt_models_json(&cfg);
        assert_eq!(doc["schema_version"], MODELS_SCHEMA_VERSION);
        let models = doc["models"].as_array().expect("models array");
        assert!(!models.is_empty());
        let first = &models[0];
        for key in ["id", "licence", "cached", "bytes", "sha256", "tier"] {
            assert!(first.get(key).is_some(), "missing {key}: {first}");
        }
        assert!(!first["licence"].as_str().unwrap().is_empty());
        assert!(doc["default_model"].as_str().is_some_and(|m| !m.is_empty()));
        // No secrets or absolute home paths leak beyond the declared cache root.
        assert!(!doc.to_string().contains("api_key"));
    }

    #[test]
    fn tts_json_does_not_need_the_runtime_to_be_warm() {
        let cfg = Config::load().expect("default config");
        let doc = tts_models_json(&cfg);
        assert_eq!(doc["direction"], "tts");
        let models = doc["models"].as_array().expect("tts models");
        assert!(!models.is_empty());
        assert!(models[0].get("licence").is_some());
        assert!(models[0].get("cached").is_some());
    }

    #[test]
    fn prepare_defaults_to_the_configured_model() {
        let cfg = Config::load().expect("default config");
        assert_eq!(
            resolve_prepare_model(&cfg, None),
            cfg.model
                .clone()
                .unwrap_or_else(|| cfg.default_local_stt_model().to_string())
        );
        assert_eq!(resolve_prepare_model(&cfg, Some("tiny-q5_1")), "tiny-q5_1");
    }
}
