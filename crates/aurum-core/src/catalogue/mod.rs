//! Versioned, validated model catalogue.
//!
//! This is deliberately independent of config parsing: the same strict parser is
//! used for the embedded review file and deployment-owned TOML files.

use crate::error::{Result, UserError};
use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const CATALOGUE_SCHEMA_VERSION: u32 = 1;
const BUILTIN_TOML: &str = include_str!("model-catalogue.v1.toml");

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Stt,
    Tts,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupportTier {
    Supported,
    Experimental,
    ExplicitOnly,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogueDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default, rename = "model")]
    pub records: Vec<CatalogueRecord>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default)]
    pub stt: DirectionDefaults,
    #[serde(default)]
    pub tts: DirectionDefaults,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DirectionDefaults {
    #[serde(default)]
    pub global: Option<String>,
    #[serde(default)]
    pub language: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogueRecord {
    pub id: String,
    pub direction: Direction,
    pub provider: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub tier: SupportTier,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub license: String,
    pub origin: Origin,
}

fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    DownloadableLocal {
        filename: String,
        url: String,
        size_bytes: u64,
        sha256: String,
    },
    PreparedLocal {
        filename: String,
        size_bytes: u64,
        sha256: String,
        source_url: String,
        revision: String,
        preparation: String,
    },
    TtsPack {
        adapter: String,
        files: Vec<PackFile>,
        voices: Vec<Voice>,
        max_phoneme_tokens: usize,
        sample_rate_hz: u32,
        shipped: bool,
    },
    Remote {
        wire_model: String,
        capabilities: RemoteCapabilities,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackFile {
    pub filename: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    pub id: String,
    pub internal_key: String,
    pub language: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteCapabilities {
    pub timestamps_reliable: bool,
    #[serde(default)]
    pub voices: Vec<String>,
    #[serde(default)]
    pub max_upload_bytes: Option<u64>,
    #[serde(default)]
    pub max_text_chars: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogueSource {
    Builtin,
    Deployment { path: PathBuf },
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveRecord {
    pub record: CatalogueRecord,
    pub source: CatalogueSource,
    pub digest: String,
}

#[derive(Debug, Clone)]
pub struct EffectiveCatalogue {
    records: Vec<EffectiveRecord>,
    defaults: Defaults,
}

impl CatalogueDocument {
    pub fn parse(input: &str) -> Result<Self> {
        let doc: Self = toml::from_str(input).map_err(invalid)?;
        doc.validate()?;
        Ok(doc)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|e| UserError::InvalidConfig {
            reason: format!("cannot read catalogue {}: {e}", path.display()),
        })?;
        Self::parse(&text)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CATALOGUE_SCHEMA_VERSION {
            return Err(config_error(format!(
                "catalogue schema_version must be {CATALOGUE_SCHEMA_VERSION}, got {}",
                self.schema_version
            )));
        }
        let mut ids = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        for record in &self.records {
            validate_id(&record.id, "canonical id")?;
            if !ids.insert(record.id.to_ascii_lowercase()) {
                return Err(config_error(format!(
                    "duplicate catalogue id '{}'",
                    record.id
                )));
            }
            for alias in &record.aliases {
                validate_id(alias, "alias")?;
                let key = alias.to_ascii_lowercase();
                if !aliases.insert(key.clone()) || ids.contains(&key) {
                    return Err(config_error(format!(
                        "duplicate or colliding catalogue alias '{alias}'"
                    )));
                }
            }
            for language in &record.languages {
                validate_language(language)?;
            }
            validate_origin(record)?;
        }
        for record in &self.records {
            for alias in &record.aliases {
                if ids.contains(&alias.to_ascii_lowercase()) {
                    return Err(config_error(format!(
                        "alias '{alias}' collides with canonical id"
                    )));
                }
            }
        }
        self.validate_defaults(&ids)?;
        Ok(())
    }

    fn validate_defaults(&self, ids: &BTreeSet<String>) -> Result<()> {
        for (direction, defaults) in [
            (Direction::Stt, &self.defaults.stt),
            (Direction::Tts, &self.defaults.tts),
        ] {
            if let Some(id) = &defaults.global {
                validate_default(id, direction, ids, &self.records)?;
            }
            for (language, id) in &defaults.language {
                validate_language(language)?;
                validate_default(id, direction, ids, &self.records)?;
            }
        }
        Ok(())
    }
}

impl EffectiveCatalogue {
    pub fn builtin() -> Result<Self> {
        Self::from_documents(builtin_document()?, None)
    }

    pub fn load_deployment(path: &Path) -> Result<Self> {
        Self::from_documents(
            builtin_document()?,
            Some((CatalogueDocument::load(path)?, path.to_path_buf())),
        )
    }

    pub fn from_documents(
        builtin: CatalogueDocument,
        deployment: Option<(CatalogueDocument, PathBuf)>,
    ) -> Result<Self> {
        builtin.validate()?;
        let mut records: BTreeMap<String, EffectiveRecord> = builtin
            .records
            .into_iter()
            .map(|record| {
                let digest = digest(&record);
                (
                    record.id.to_ascii_lowercase(),
                    EffectiveRecord {
                        record,
                        source: CatalogueSource::Builtin,
                        digest,
                    },
                )
            })
            .collect();
        let mut defaults = builtin.defaults;
        if let Some((deployment, path)) = deployment {
            deployment.validate()?;
            // Deployment records replace the entire matching record; nothing is inherited.
            for record in deployment.records {
                let key = record.id.to_ascii_lowercase();
                if record.enabled {
                    let digest = digest(&record);
                    records.insert(
                        key,
                        EffectiveRecord {
                            record,
                            source: CatalogueSource::Deployment { path: path.clone() },
                            digest,
                        },
                    );
                } else {
                    records.remove(&key);
                }
            }
            // A non-empty deployment defaults section is explicit; empty sides retain built-ins.
            merge_defaults(&mut defaults.stt, deployment.defaults.stt);
            merge_defaults(&mut defaults.tts, deployment.defaults.tts);
        }
        let records: Vec<_> = records.into_values().collect();
        let effective = Self { records, defaults };
        effective.validate_effective()?;
        Ok(effective)
    }

    pub fn records(&self) -> &[EffectiveRecord] {
        &self.records
    }
    /// Digest of the canonical serialized effective records for diagnostics and
    /// resumable batch fingerprints.
    pub fn digest(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(&self.records).expect("effective catalogue is serializable"),
        ))
    }
    pub fn source_for(&self, id: &str) -> Option<&CatalogueSource> {
        self.lookup(id).map(|r| &r.source)
    }
    pub fn lookup(&self, id: &str) -> Option<&EffectiveRecord> {
        let key = id.trim();
        self.records.iter().find(|entry| {
            entry.record.id.eq_ignore_ascii_case(key)
                || entry
                    .record
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(key))
        })
    }
    pub fn resolve(
        &self,
        direction: Direction,
        cli: Option<&str>,
        configured: Option<&str>,
        language: &str,
    ) -> Result<&EffectiveRecord> {
        if let Some(id) = [cli, configured].into_iter().flatten().next() {
            return self.lookup_direction(id, direction).ok_or_else(|| {
                config_error(format!(
                    "unknown or incompatible {direction:?} model '{id}'"
                ))
            });
        }
        if !language.eq_ignore_ascii_case("auto") {
            let normalized = normalize_language(language)?;
            if let Some(id) = self.defaults_for(direction).language.get(&normalized) {
                return self
                    .lookup_direction(id, direction)
                    .ok_or_else(|| config_error(format!("default '{id}' is unavailable")));
            }
            if let Some(base) = normalized.split('-').next() {
                if let Some(id) = self.defaults_for(direction).language.get(base) {
                    return self
                        .lookup_direction(id, direction)
                        .ok_or_else(|| config_error(format!("default '{id}' is unavailable")));
                }
            }
        }
        let id = self
            .defaults_for(direction)
            .global
            .as_deref()
            .ok_or_else(|| config_error(format!("no global {direction:?} default")))?;
        self.lookup_direction(id, direction)
            .ok_or_else(|| config_error(format!("default '{id}' is unavailable")))
    }

    fn lookup_direction(&self, id: &str, direction: Direction) -> Option<&EffectiveRecord> {
        self.lookup(id)
            .filter(|entry| entry.record.direction == direction)
    }
    fn defaults_for(&self, direction: Direction) -> &DirectionDefaults {
        match direction {
            Direction::Stt => &self.defaults.stt,
            Direction::Tts => &self.defaults.tts,
        }
    }
    fn validate_effective(&self) -> Result<()> {
        let doc = CatalogueDocument {
            schema_version: CATALOGUE_SCHEMA_VERSION,
            defaults: self.defaults.clone(),
            records: self
                .records
                .iter()
                .map(|entry| entry.record.clone())
                .collect(),
        };
        doc.validate()
    }
}

/// The v1 document owns reviewed metadata.  Legacy local Whisper records are
/// materialized here until their consumers finish moving off the historical
/// `model::MODELS` compatibility view; this keeps every existing ID and pin in
/// the effective catalogue during that transition.
fn builtin_document() -> Result<CatalogueDocument> {
    let mut document = CatalogueDocument::parse(BUILTIN_TOML)?;
    for model in crate::model::MODELS {
        if document
            .records
            .iter()
            .any(|record| record.id == model.name)
        {
            continue;
        }
        let sha256 = crate::model::pinned_sha256(model.filename)
            .ok_or_else(|| config_error(format!("missing built-in pin for {}", model.filename)))?;
        let size_bytes = crate::model::pinned_exact_bytes(model.filename)
            .ok_or_else(|| config_error(format!("missing built-in size for {}", model.filename)))?;
        let origin = if model.name == "large-v3-ptpt-q5_0" {
            Origin::PreparedLocal {
                filename: model.filename.into(), size_bytes, sha256: sha256.into(),
                source_url: "https://huggingface.co/inesc-id/WhisperLv3-FT/tree/77837e42b56d4be6ca15a66b5c41c9b8cf3e41b0".into(),
                revision: "77837e42b56d4be6ca15a66b5c41c9b8cf3e41b0".into(),
                preparation: "scripts/prepare_portuguese_models.sh --cache-root ${XDG_CACHE_HOME:-$HOME/.cache} --work-dir /tmp/aurum-portuguese-tools".into(),
            }
        } else {
            Origin::DownloadableLocal {
                filename: model.filename.into(),
                size_bytes,
                sha256: sha256.into(),
                url: format!(
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
                    model.filename
                ),
            }
        };
        document.records.push(CatalogueRecord {
            id: model.name.into(),
            direction: Direction::Stt,
            provider: "local".into(),
            aliases: Vec::new(),
            enabled: true,
            tier: if matches!(
                model.name,
                "large-v3-q5_0" | "medium-ptbr-q5_0" | "large-v3-ptpt-q5_0"
            ) {
                SupportTier::Experimental
            } else {
                SupportTier::Supported
            },
            languages: Vec::new(),
            notes: model.notes.into(),
            license: "MIT (whisper.cpp weights via OpenAI Whisper terms)".into(),
            origin,
        });
    }
    #[cfg(feature = "tts")]
    for model in crate::tts::catalogue::MODELS {
        if document.records.iter().any(|record| record.id == model.id) {
            continue;
        }
        let pack_file = |file: &crate::tts::catalogue::PackFile| PackFile {
            filename: file.filename.into(),
            url: file.url.map(str::to_string).unwrap_or_else(|| {
                format!(
                    "https://huggingface.co/{}/resolve/main/{}",
                    model.hf_repo, file.filename
                )
            }),
            size_bytes: file.approx_bytes,
            sha256: file.sha256.into(),
        };
        let voices = crate::tts::catalogue::VOICES
            .iter()
            .filter(|voice| voice.model == model.id)
            .map(|voice| Voice {
                id: voice.id.into(),
                internal_key: voice.internal_key.into(),
                language: voice.language.into(),
                notes: voice.notes.into(),
            })
            .collect();
        document.records.push(CatalogueRecord {
            id: model.id.into(),
            direction: Direction::Tts,
            provider: "local".into(),
            aliases: Vec::new(),
            enabled: true,
            tier: SupportTier::Supported,
            languages: model
                .languages
                .iter()
                .map(|language| (*language).into())
                .collect(),
            notes: model.notes.into(),
            license: model.license.into(),
            origin: Origin::TtsPack {
                adapter: model.adapter.into(),
                files: vec![
                    pack_file(&model.onnx),
                    pack_file(&model.voices),
                    pack_file(&model.config),
                ],
                voices,
                max_phoneme_tokens: model.max_phoneme_tokens,
                sample_rate_hz: model.sample_rate_hz,
                shipped: model.shipped,
            },
        });
    }
    for record in crate::providers::OPENAI_STT_REGISTRY {
        push_remote(
            &mut document,
            record.model,
            "openai",
            Direction::Stt,
            record.max_upload_bytes as u64,
            None,
            &[],
        );
    }
    for record in crate::providers::XAI_STT_REGISTRY {
        push_remote(
            &mut document,
            record.model,
            "xai",
            Direction::Stt,
            record.max_upload_bytes as u64,
            None,
            &[],
        );
    }
    for record in crate::capabilities::OPENROUTER_STT_REGISTRY {
        push_remote(
            &mut document,
            record.model_id,
            "openrouter",
            Direction::Stt,
            0,
            None,
            &[],
        );
    }
    for record in crate::providers::OPENROUTER_TTS_REGISTRY {
        push_remote(
            &mut document,
            record.model,
            "openrouter",
            Direction::Tts,
            0,
            Some(record.max_text_chars),
            record.voices,
        );
    }
    for record in crate::providers::OPENAI_TTS_REGISTRY {
        push_remote(
            &mut document,
            record.model,
            "openai",
            Direction::Tts,
            0,
            Some(record.max_text_chars),
            record.voices,
        );
    }
    for record in crate::providers::ELEVENLABS_TTS_REGISTRY {
        push_remote(
            &mut document,
            record.model,
            "elevenlabs",
            Direction::Tts,
            0,
            Some(record.max_text_chars),
            &[],
        );
    }
    for record in crate::providers::XAI_TTS_REGISTRY {
        push_remote(
            &mut document,
            record.model,
            "xai",
            Direction::Tts,
            0,
            Some(record.max_text_chars),
            record.voices,
        );
    }
    document.validate()?;
    Ok(document)
}

fn push_remote(
    document: &mut CatalogueDocument,
    id: &str,
    provider: &str,
    direction: Direction,
    max_upload_bytes: u64,
    max_text_chars: Option<usize>,
    voices: &[&str],
) {
    if document
        .records
        .iter()
        .any(|record| record.id.eq_ignore_ascii_case(id))
    {
        return;
    }
    document.records.push(CatalogueRecord {
        id: id.into(),
        direction,
        provider: provider.into(),
        aliases: Vec::new(),
        enabled: true,
        tier: SupportTier::Supported,
        languages: Vec::new(),
        notes: "reviewed remote provider record".into(),
        license: "provider terms".into(),
        origin: Origin::Remote {
            wire_model: id.into(),
            capabilities: RemoteCapabilities {
                timestamps_reliable: direction == Direction::Stt,
                voices: voices.iter().map(|voice| (*voice).into()).collect(),
                max_upload_bytes: (max_upload_bytes > 0).then_some(max_upload_bytes),
                max_text_chars,
            },
        },
    });
}

fn merge_defaults(target: &mut DirectionDefaults, incoming: DirectionDefaults) {
    if incoming.global.is_some() {
        target.global = incoming.global;
    }
    target.language.extend(incoming.language);
}
fn validate_default(
    id: &str,
    direction: Direction,
    _ids: &BTreeSet<String>,
    records: &[CatalogueRecord],
) -> Result<()> {
    if records.iter().any(|record| {
        record.enabled && record.direction == direction && record.id.eq_ignore_ascii_case(id)
    }) {
        Ok(())
    } else {
        Err(config_error(format!(
            "{direction:?} default '{id}' does not resolve to an enabled compatible record"
        )))
    }
}
fn validate_id(value: &str, field: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'));
    if valid {
        Ok(())
    } else {
        Err(config_error(format!("invalid {field} '{value}'")))
    }
}
fn validate_language(language: &str) -> Result<()> {
    if language.eq_ignore_ascii_case("auto") {
        return Ok(());
    }
    normalize_language(language).map(|_| ())
}
fn normalize_language(language: &str) -> Result<String> {
    language
        .parse::<LanguageTag>()
        .map(|tag| tag.to_string())
        .map_err(|_| config_error(format!("invalid BCP-47 language tag '{language}'")))
}
fn validate_origin(record: &CatalogueRecord) -> Result<()> {
    match &record.origin {
        Origin::DownloadableLocal {
            filename,
            url,
            size_bytes,
            sha256,
        } => {
            local_record(record)?;
            validate_id(filename, "artifact filename")?;
            safe_https(url)?;
            pin(*size_bytes, sha256)
        }
        Origin::PreparedLocal {
            filename,
            size_bytes,
            sha256,
            source_url,
            revision,
            preparation,
        } => {
            local_record(record)?;
            validate_id(filename, "artifact filename")?;
            safe_https(source_url)?;
            if revision.trim().is_empty() || preparation.trim().is_empty() {
                return Err(config_error(
                    "prepared-local records require immutable revision and preparation guidance",
                ));
            }
            pin(*size_bytes, sha256)
        }
        Origin::TtsPack {
            adapter,
            files,
            voices,
            max_phoneme_tokens,
            sample_rate_hz,
            ..
        } => {
            if record.direction != Direction::Tts || !record.provider.eq_ignore_ascii_case("local")
            {
                return Err(config_error(
                    "tts_pack origins require direction=tts and provider=local",
                ));
            }
            if !matches!(
                adapter.as_str(),
                "kitten-onnx-v1" | "kokoro-onnx-v0" | "placeholder-v0"
            ) || files.is_empty()
                || voices.is_empty()
                || *max_phoneme_tokens == 0
                || *sample_rate_hz == 0
            {
                return Err(config_error(
                    "invalid TTS pack adapter, files, voices, or limits",
                ));
            }
            for file in files {
                validate_id(&file.filename, "pack filename")?;
                safe_https(&file.url)?;
                pin(file.size_bytes, &file.sha256)?;
            }
            for voice in voices {
                validate_id(&voice.id, "voice id")?;
                if voice.internal_key.trim().is_empty() {
                    return Err(config_error("voice internal_key cannot be empty"));
                }
                validate_language(&voice.language)?;
            }
            Ok(())
        }
        Origin::Remote {
            wire_model,
            capabilities,
        } => {
            if record.provider.eq_ignore_ascii_case("local")
                || wire_model.trim().is_empty()
                || !is_registered_remote(&record.provider, record.direction)
            {
                return Err(config_error(
                    "remote record uses an unsupported provider/direction combination",
                ));
            }
            if record.direction == Direction::Tts && capabilities.max_text_chars.is_none() {
                return Err(config_error(
                    "remote TTS records require max_text_chars capability metadata",
                ));
            }
            Ok(())
        }
    }
}
fn local_record(record: &CatalogueRecord) -> Result<()> {
    if record.direction == Direction::Stt && record.provider.eq_ignore_ascii_case("local") {
        Ok(())
    } else {
        Err(config_error(
            "local STT artifact origins require direction=stt and provider=local",
        ))
    }
}
fn is_registered_remote(provider: &str, direction: Direction) -> bool {
    matches!(
        (provider.to_ascii_lowercase().as_str(), direction),
        ("openrouter", _) | ("openai", _) | ("xai", _) | ("elevenlabs", Direction::Tts)
    )
}
fn safe_https(url: &str) -> Result<()> {
    let url = url::Url::parse(url).map_err(|_| config_error(format!("invalid URL '{url}'")))?;
    if url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
    {
        Ok(())
    } else {
        Err(config_error(format!(
            "origin URL must be safe HTTPS: '{url}'"
        )))
    }
}
fn pin(size: u64, sha256: &str) -> Result<()> {
    if size > 0
        && sha256.len() == 64
        && sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(config_error(
            "integrity pins require nonzero exact size and lowercase 64-hex SHA-256",
        ))
    }
}
fn digest(record: &CatalogueRecord) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(record).expect("catalogue record is serializable"),
    ))
}
fn invalid(error: toml::de::Error) -> crate::error::AurumError {
    config_error(format!("invalid catalogue TOML: {error}"))
}
fn config_error(reason: impl Into<String>) -> crate::error::AurumError {
    UserError::InvalidConfig {
        reason: reason.into(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_catalogue_is_valid() {
        assert!(EffectiveCatalogue::builtin().is_ok());
    }

    #[test]
    fn parser_rejects_unknown_fields_bad_pins_and_invalid_languages() {
        let unknown = "schema_version = 1\nunexpected = true\n";
        assert!(CatalogueDocument::parse(unknown).is_err());
        let invalid = r#"schema_version = 1
[[model]]
id = "bad"
direction = "stt"
provider = "local"
languages = ["not a language"]
tier = "supported"
origin = { kind = "downloadable_local", filename = "bad.bin", url = "https://example.invalid/bad.bin", size_bytes = 1, sha256 = "UPPERCASE" }
"#;
        assert!(CatalogueDocument::parse(invalid).is_err());
    }

    #[test]
    fn effective_builtin_catalogue_covers_legacy_stt_ids_and_pins() {
        let catalogue = EffectiveCatalogue::builtin().unwrap();
        for model in crate::model::MODELS {
            assert!(
                catalogue.lookup(model.name).is_some(),
                "missing {}",
                model.name
            );
        }
    }

    #[cfg(feature = "tts")]
    #[test]
    fn effective_builtin_catalogue_covers_legacy_tts_models() {
        let catalogue = EffectiveCatalogue::builtin().unwrap();
        for model in crate::tts::catalogue::MODELS {
            assert!(catalogue.lookup(model.id).is_some(), "missing {}", model.id);
        }
    }
    #[test]
    fn resolver_prefers_exact_then_base_then_global() {
        let catalogue = EffectiveCatalogue::builtin().unwrap();
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, None, None, "pt-BR")
                .unwrap()
                .record
                .id,
            "medium-ptbr-q5_0"
        );
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, None, None, "pt")
                .unwrap()
                .record
                .id,
            "base"
        );
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, Some("tiny"), Some("base"), "pt-BR")
                .unwrap()
                .record
                .id,
            "tiny"
        );
    }
    #[test]
    fn deployment_replaces_and_disable_removes_aliases() {
        let base = builtin_document().unwrap();
        let deploy = CatalogueDocument::parse(r#"schema_version = 1
[[model]]
id = "tiny"
direction = "stt"
provider = "local"
enabled = false
tier = "supported"
origin = { kind = "downloadable_local", filename = "tiny.bin", url = "https://example.com/tiny.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#).unwrap();
        let effective = EffectiveCatalogue::from_documents(
            base,
            Some((deploy, PathBuf::from("/tmp/deploy.toml"))),
        )
        .unwrap();
        assert!(effective.lookup("tiny").is_none());
    }

    #[test]
    fn deployment_replacement_has_no_inherited_aliases_or_metadata() {
        let deployment_path = PathBuf::from("/tmp/deployment-catalogue.toml");
        let deployment = CatalogueDocument::parse(r#"schema_version = 1
[[model]]
id = "base"
aliases = ["replacement-base"]
direction = "stt"
provider = "local"
tier = "experimental"
notes = "deployment-owned record"
license = "Apache-2.0"
origin = { kind = "downloadable_local", filename = "replacement-base.bin", url = "https://example.invalid/replacement-base.bin", size_bytes = 42, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#).unwrap();
        let effective = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, deployment_path.clone())),
        )
        .unwrap();
        let base = effective.lookup("base").unwrap();
        assert_eq!(base.record.notes, "deployment-owned record");
        assert_eq!(base.record.aliases, ["replacement-base"]);
        assert!(effective.lookup("base-default").is_none());
        assert!(
            matches!(base.source, CatalogueSource::Deployment { ref path } if path == &deployment_path)
        );
    }

    #[test]
    fn effective_digest_changes_when_a_record_changes() {
        let builtin = EffectiveCatalogue::builtin().unwrap();
        let deployment = CatalogueDocument::parse(r#"schema_version = 1
[[model]]
id = "base"
direction = "stt"
provider = "local"
tier = "supported"
notes = "changed effective record"
origin = { kind = "downloadable_local", filename = "changed-base.bin", url = "https://example.invalid/changed-base.bin", size_bytes = 42, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#).unwrap();
        let changed = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, PathBuf::from("/tmp/changed.toml"))),
        )
        .unwrap();
        assert_ne!(builtin.digest(), changed.digest());
    }
}
