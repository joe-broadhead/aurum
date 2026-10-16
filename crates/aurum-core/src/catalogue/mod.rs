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
use std::io::Read;
use std::path::{Path, PathBuf};

pub const CATALOGUE_SCHEMA_VERSION: u32 = 1;
/// Upper bound for a deployment catalogue file. The embedded catalogue is
/// ~30 KiB; anything near this limit is not a reviewed model list.
pub const MAX_CATALOGUE_BYTES: u64 = 1024 * 1024;
const BUILTIN_TOML: &str = include_str!("model-catalogue.v1.toml");
/// Tracks honouring deployment add/replace and TTS records.
const DEPLOYMENT_RECORDS_ISSUE: &str = "https://github.com/joe-broadhead/aurum/issues/146";

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
    /// The only implicit selection. Language never selects a model: experimental
    /// specialists are reachable only through an explicit model id.
    #[serde(default)]
    pub global: Option<String>,
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
    /// Canonical ids of built-in records a deployment disabled, sorted. Until
    /// #146 a deployment can only narrow, so this (with the digest) is where
    /// its effect is visible: every remaining record's source is `Builtin`.
    disabled: Vec<String>,
    /// Computed once at construction so reading it can never fail or panic.
    digest: String,
}

impl CatalogueDocument {
    pub fn parse(input: &str) -> Result<Self> {
        let doc: Self = toml::from_str(input).map_err(invalid)?;
        doc.validate()?;
        Ok(doc)
    }

    /// Load a deployment catalogue: a trusted, operator-owned input. The path
    /// must be absolute and name a regular file (a final symlink is refused,
    /// not followed) no larger than [`MAX_CATALOGUE_BYTES`].
    pub fn load(path: &Path) -> Result<Self> {
        let read_error = |e: std::io::Error| {
            config_error(format!("cannot read catalogue {}: {e}", path.display()))
        };
        if !path.is_absolute() {
            return Err(config_error(format!(
                "catalogue path must be absolute, got '{}'",
                path.display()
            )));
        }
        let checked = fs::symlink_metadata(path).map_err(read_error)?;
        if checked.file_type().is_symlink() {
            return Err(config_error(format!(
                "refusing catalogue {} because it is a symlink; point [catalogue].path at the regular file",
                path.display()
            )));
        }
        if !checked.is_file() {
            return Err(config_error(format!(
                "catalogue {} is not a regular file",
                path.display()
            )));
        }
        let file = fs::File::open(path).map_err(read_error)?;
        let opened = file.metadata().map_err(read_error)?;
        // The handle must be the file that was checked, not a swapped-in path.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (checked.dev(), checked.ino()) != (opened.dev(), opened.ino()) {
                return Err(config_error(format!(
                    "catalogue {} changed while it was being opened",
                    path.display()
                )));
            }
        }
        if !opened.is_file() || opened.len() > MAX_CATALOGUE_BYTES {
            return Err(config_error(format!(
                "catalogue {} must be a regular file of at most {MAX_CATALOGUE_BYTES} bytes",
                path.display()
            )));
        }
        let mut text = String::new();
        file.take(MAX_CATALOGUE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(read_error)?;
        if text.len() as u64 > MAX_CATALOGUE_BYTES {
            return Err(config_error(format!(
                "catalogue {} exceeds {MAX_CATALOGUE_BYTES} bytes",
                path.display()
            )));
        }
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
            // This is a local model catalogue. Remote providers keep their own
            // reviewed registries and `ProviderCapabilities`; duplicating them
            // here would create a second, drifting source of capability truth.
            if record.provider != "local" {
                return Err(config_error(format!(
                    "catalogue record '{}' must use provider = \"local\"; remote models are \
                     selected with --provider and are not catalogue records",
                    record.id
                )));
            }
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
        // Defaults are deliberately not resolved here. A deployment document may
        // point at a built-in record which only exists after the effective view is
        // assembled. Validate the keys now and resolve them after merging.
        self.validate_default_keys()?;
        Ok(())
    }

    fn validate_default_keys(&self) -> Result<()> {
        for defaults in [&self.defaults.stt, &self.defaults.tts] {
            if let Some(id) = &defaults.global {
                validate_id(id, "default model id")?;
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
            .filter(|record| record.enabled)
            .map(|record| {
                let digest = digest(&record)?;
                Ok((
                    record.id.to_ascii_lowercase(),
                    EffectiveRecord {
                        record,
                        source: CatalogueSource::Builtin,
                        digest,
                    },
                ))
            })
            .collect::<Result<_>>()?;
        let mut defaults = builtin.defaults;
        let mut disabled_ids: Vec<String> = Vec::new();
        if let Some((deployment, _path)) = deployment {
            deployment.validate()?;
            reject_widening_deployment(&deployment)?;
            // A deployment can only narrow the built-in catalogue: every record
            // here is a disable entry for an STT built-in.
            let mut disabled: Vec<EffectiveRecord> = Vec::new();
            for record in deployment.records {
                // Aliases are compatibility names; disabling one must disable
                // its canonical record rather than silently doing nothing.
                let key = records.iter().find_map(|(canonical, entry)| {
                    names_record(&entry.record, &record.id).then(|| canonical.clone())
                });
                let Some(key) = key else {
                    // Disabling `large-v3` and its alias `large` together is
                    // one intent, not an error.
                    if disabled
                        .iter()
                        .any(|entry| names_record(&entry.record, &record.id))
                    {
                        continue;
                    }
                    return Err(config_error(format!(
                        "disabled model '{}' does not match an effective canonical id or alias",
                        record.id
                    )));
                };
                // The disable entry declares `direction = "stt"`, but the name it
                // matches is what gets removed: it must not reach a TTS record.
                if records[&key].record.direction != Direction::Stt {
                    return Err(config_error(format!(
                        "deployment catalogue disables '{}', which is not an STT model; a \
                         deployment catalogue can currently only disable built-in STT models \
                         (see {DEPLOYMENT_RECORDS_ISSUE})",
                        record.id
                    )));
                }
                disabled.extend(records.remove(&key));
            }
            // An explicit deployment global replaces the built-in one; an absent
            // global retains the built-in default.
            merge_defaults(&mut defaults.stt, deployment.defaults.stt);
            merge_defaults(&mut defaults.tts, deployment.defaults.tts);
            disabled_ids = disabled.into_iter().map(|entry| entry.record.id).collect();
            disabled_ids.sort();
        }
        let records: Vec<_> = records.into_values().collect();
        reject_effective_name_collisions(&records)?;
        // The digest covers what selection depends on: the records and the
        // defaults. Source paths are diagnostic metadata, not model identity,
        // so moving an identical deployment file does not change it.
        #[derive(Serialize)]
        struct DigestInput<'a> {
            records: Vec<&'a CatalogueRecord>,
            defaults: &'a Defaults,
        }
        let digest = digest(&DigestInput {
            records: records.iter().map(|entry| &entry.record).collect(),
            defaults: &defaults,
        })?;
        let effective = Self {
            records,
            defaults,
            disabled: disabled_ids,
            digest,
        };
        effective.validate_effective()?;
        Ok(effective)
    }

    pub fn records(&self) -> &[EffectiveRecord] {
        &self.records
    }
    /// Canonical ids of built-in records the deployment catalogue disabled.
    pub fn disabled(&self) -> &[String] {
        &self.disabled
    }
    /// Digest of the canonical serialized effective records for diagnostics and
    /// resumable batch fingerprints.
    pub fn digest(&self) -> &str {
        &self.digest
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
    /// Resolve a model: CLI id, then configured id, then the global default.
    /// Language is deliberately not an input: it is a decoding hint only and
    /// must never change which weights are selected or downloaded.
    pub fn resolve(
        &self,
        direction: Direction,
        cli: Option<&str>,
        configured: Option<&str>,
    ) -> Result<&EffectiveRecord> {
        if let Some(id) = [cli, configured].into_iter().flatten().next() {
            return self.lookup_direction(id, direction).ok_or_else(|| {
                config_error(format!(
                    "unknown or incompatible {direction:?} model '{id}'"
                ))
            });
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
        for (direction, defaults) in [
            (Direction::Stt, &self.defaults.stt),
            (Direction::Tts, &self.defaults.tts),
        ] {
            if let Some(id) = &defaults.global {
                self.validate_effective_default(id, direction)?;
            }
        }
        Ok(())
    }
    /// Validate a default through the same alias-aware lookup `resolve()` uses,
    /// so the record that is checked is always the record that is selected.
    fn validate_effective_default(&self, id: &str, direction: Direction) -> Result<()> {
        let Some(entry) = self
            .lookup_direction(id, direction)
            .filter(|entry| entry.record.provider == "local")
        else {
            return Err(config_error(format!(
                "{direction:?} default '{id}' does not resolve to an enabled compatible record"
            )));
        };
        // Experimental and explicit-only records are reachable by explicit id only;
        // a catalogue (built-in or deployment) can never make one implicit.
        if entry.record.tier != SupportTier::Supported {
            return Err(config_error(format!(
                "{direction:?} default '{id}' must be a supported record, not {:?}",
                entry.record.tier
            )));
        }
        Ok(())
    }
}

/// Model downloads, cache pins and TTS selection still read the legacy Rust
/// tables (#125), so a deployment record that adds or replaces a model would
/// change diagnostics without changing what is fetched. Until that routing
/// lands, a deployment may only disable STT built-ins and choose the STT default.
fn reject_widening_deployment(deployment: &CatalogueDocument) -> Result<()> {
    let unsupported = |what: String| {
        config_error(format!(
            "deployment catalogue {what}; a deployment catalogue can currently only \
             disable built-in STT models (`enabled = false`) and set [defaults.stt].global \
             (see {DEPLOYMENT_RECORDS_ISSUE})"
        ))
    };
    if deployment.defaults.tts.global.is_some() {
        return Err(unsupported("sets [defaults.tts].global".into()));
    }
    for record in &deployment.records {
        if record.direction == Direction::Tts {
            return Err(unsupported(format!("has TTS record '{}'", record.id)));
        }
        if record.enabled {
            return Err(unsupported(format!(
                "adds or replaces model '{}'",
                record.id
            )));
        }
    }
    Ok(())
}

/// Whether `name` is the canonical id or an alias of `record` (ASCII
/// case-insensitive, like [`EffectiveCatalogue::lookup`]).
fn names_record(record: &CatalogueRecord, name: &str) -> bool {
    std::iter::once(&record.id)
        .chain(&record.aliases)
        .any(|candidate| candidate.eq_ignore_ascii_case(name.trim()))
}

/// Every effective id and alias must name exactly one record. Each document
/// checks itself; this catches a deployment name that shadows a built-in one,
/// which would otherwise let lookup order decide which weights are selected.
///
/// While [`reject_widening_deployment`] limits a deployment to disabling
/// records, the effective set is a subset of the validated built-ins and this
/// cannot fire. It is kept armed for #146, when deployment records may add or
/// replace models; `narrowing_deployment_never_produces_deployment_sources`
/// pins the current invariant.
fn reject_effective_name_collisions(records: &[EffectiveRecord]) -> Result<()> {
    let mut owners: BTreeMap<String, &str> = BTreeMap::new();
    for entry in records {
        let record = &entry.record;
        for name in std::iter::once(&record.id).chain(&record.aliases) {
            if let Some(owner) = owners.insert(name.to_ascii_lowercase(), &record.id) {
                return Err(config_error(format!(
                    "catalogue name '{name}' is used by both '{owner}' and '{}'",
                    record.id
                )));
            }
        }
    }
    Ok(())
}

/// The embedded v1 document is the complete built-in catalogue. Nothing is
/// synthesized from Rust tables: parity tests instead assert that the legacy
/// `model::MODELS` / `tts::catalogue::MODELS` views match this file exactly.
fn builtin_document() -> Result<CatalogueDocument> {
    let document = CatalogueDocument::parse(BUILTIN_TOML)?;
    #[cfg(not(feature = "tts"))]
    let document = {
        let mut document = document;
        document
            .records
            .retain(|record| record.direction != Direction::Tts);
        document.defaults.tts = DirectionDefaults::default();
        document
    };
    for record in &document.records {
        for (url, kind) in record_urls(record) {
            reviewed_builtin_url(url, kind)?;
        }
    }
    Ok(document)
}

fn merge_defaults(target: &mut DirectionDefaults, incoming: DirectionDefaults) {
    if incoming.global.is_some() {
        target.global = incoming.global;
    }
}
fn validate_id(value: &str, field: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(config_error(format!("invalid {field} '{value}'")))
    }
}
/// Artifact filenames are joined onto cache directories, so each one must be a
/// single, visible path component: no separators, no `.`/`..`, no dotfiles.
fn validate_filename(value: &str, field: &str) -> Result<()> {
    validate_id(value, field)?;
    if value.starts_with('.') {
        return Err(config_error(format!(
            "invalid {field} '{value}': must be a single file name, not a dotfile or relative path"
        )));
    }
    Ok(())
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
            validate_filename(filename, "artifact filename")?;
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
            validate_filename(filename, "artifact filename")?;
            safe_https(source_url)?;
            // The revision is provenance for a locally prepared artifact: it must
            // be an immutable revision and must be the one named by source_url,
            // so the guidance and the checkpoint cannot drift apart.
            if !is_immutable_revision(revision) || !url_revision(source_url, revision) {
                return Err(config_error(format!(
                    "prepared-local record '{}' requires an immutable 40-hex revision that \
                     appears in source_url",
                    record.id
                )));
            }
            if preparation.trim().is_empty() {
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
                validate_filename(&file.filename, "pack filename")?;
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
    }
}
/// Whether a URL is fetched by Aurum or only names a preparation source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlKind {
    Download,
    PreparationSource,
}
fn record_urls(record: &CatalogueRecord) -> Vec<(&str, UrlKind)> {
    match &record.origin {
        Origin::DownloadableLocal { url, .. } => vec![(url.as_str(), UrlKind::Download)],
        Origin::PreparedLocal { source_url, .. } => {
            vec![(source_url.as_str(), UrlKind::PreparationSource)]
        }
        Origin::TtsPack { files, .. } => files
            .iter()
            .map(|file| (file.url.as_str(), UrlKind::Download))
            .collect(),
    }
}
/// GitHub release origins a built-in record may use, as `(owner, repo, tag)`.
/// Release tags are not content-addressed (an asset can be replaced), so each
/// entry is a reviewed exception: the exact size and SHA-256 pins are the
/// identity, and both are enforced on download and on cache verification, so a
/// replaced asset fails closed. Prefer a pinned Hugging Face revision; adding
/// an entry here needs review.
const REVIEWED_GITHUB_RELEASES: &[(&str, &str, &str)] =
    &[("thewh1teagle", "kokoro-onnx", "model-files-v1.0")];

/// Built-in records may only point at the reviewed hosts already used by
/// `model::` / `tts::catalogue`, and Hugging Face URLs must name an immutable
/// 40-hex revision rather than a moving branch such as `main`. GitHub URLs
/// must be a release asset of a [`REVIEWED_GITHUB_RELEASES`] entry. Downloads
/// must use `resolve/` (a file); `tree/` (a repository view) only names the
/// source of a prepared-local artifact.
fn reviewed_builtin_url(url: &str, kind: UrlKind) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(|_| config_error(format!("invalid URL '{url}'")))?;
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(Iterator::collect)
        .unwrap_or_default();
    let reviewed = parsed.scheme() == "https"
        && default_https_port(&parsed)
        && match parsed.host_str() {
            // huggingface.co/{org}/{repo}/(resolve|tree)/{revision}/...
            Some("huggingface.co") => {
                segments.len() >= 4
                    && (segments[2] == "resolve"
                        || (segments[2] == "tree" && kind == UrlKind::PreparationSource))
                    && is_immutable_revision(segments[3])
            }
            // github.com/{org}/{repo}/releases/download/{tag}/{asset}
            Some("github.com") => {
                segments.len() == 6
                    && segments[2..4] == ["releases", "download"]
                    && REVIEWED_GITHUB_RELEASES.iter().any(|(owner, repo, tag)| {
                        segments[0] == *owner && segments[1] == *repo && segments[4] == *tag
                    })
                    && parsed.query().is_none()
            }
            _ => false,
        };
    if reviewed {
        Ok(())
    } else {
        Err(config_error(format!(
            "built-in catalogue URL must use a reviewed host and an immutable revision: '{url}'"
        )))
    }
}
/// A content-addressed source revision: exactly 40 hex digits.
fn is_immutable_revision(revision: &str) -> bool {
    revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit())
}
/// Whether `revision` names a path segment of `source_url`. Guards against a
/// prepared-local record whose guidance and pinned checkpoint disagree.
fn url_revision(source_url: &str, revision: &str) -> bool {
    url::Url::parse(source_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .map(|segments| segments.into_iter().any(|segment| segment == revision))
        })
        .unwrap_or(false)
}
/// Only the default HTTPS port is accepted. The host check alone would let
/// `https://huggingface.co:8443/...` through, because `host_str()` drops the port.
fn default_https_port(url: &url::Url) -> bool {
    matches!(url.port(), None | Some(443))
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
fn safe_https(url: &str) -> Result<()> {
    let url = url::Url::parse(url).map_err(|_| config_error(format!("invalid URL '{url}'")))?;
    if url.scheme() == "https"
        && url.host_str().is_some()
        && default_https_port(&url)
        && url.username().is_empty()
        && url.password().is_none()
    {
        Ok(())
    } else {
        Err(config_error(format!(
            "origin URL must be safe HTTPS (default port, no credentials): '{url}'"
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
/// SHA-256 of the canonical JSON encoding. Serialization failure is reported
/// as a configuration error rather than aborting config load or fingerprinting.
fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| config_error(format!("cannot encode catalogue for digest: {e}")))?;
    Ok(hex::encode(Sha256::digest(bytes)))
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
    fn toml_stt_records_match_the_legacy_table_exactly() {
        let document = builtin_document().unwrap();
        let stt: Vec<_> = document
            .records
            .iter()
            .filter(|record| record.direction == Direction::Stt)
            .collect();
        // Every legacy name (canonical or alias) is a TOML id or alias with identical pins.
        for model in crate::model::MODELS {
            let record = stt
                .iter()
                .find(|record| {
                    record.id == model.name || record.aliases.iter().any(|a| a == model.name)
                })
                .unwrap_or_else(|| panic!("{} missing from model-catalogue.v1.toml", model.name));
            let (filename, size_bytes, sha256) = match &record.origin {
                Origin::DownloadableLocal {
                    filename,
                    size_bytes,
                    sha256,
                    ..
                }
                | Origin::PreparedLocal {
                    filename,
                    size_bytes,
                    sha256,
                    ..
                } => (filename, *size_bytes, sha256),
                other => panic!("{} has non-local origin {other:?}", model.name),
            };
            assert_eq!(filename, model.filename, "{} filename drift", model.name);
            assert_eq!(
                Some(size_bytes),
                crate::model::pinned_exact_bytes(model.filename),
                "{} exact size drift",
                model.name
            );
            assert_eq!(
                Some(sha256.as_str()),
                crate::model::pinned_sha256(model.filename),
                "{} sha256 drift",
                model.name
            );
            let tier = match crate::model::model_support_tier(model.name) {
                crate::model::ModelSupportTier::Supported => SupportTier::Supported,
                crate::model::ModelSupportTier::Experimental => SupportTier::Experimental,
            };
            assert_eq!(record.tier, tier, "{} tier drift", model.name);
            // The URL a reviewer reads is the URL the downloader fetches.
            let manifest = crate::model::artifact_manifest_json(model);
            match &record.origin {
                Origin::DownloadableLocal { url, .. } => assert_eq!(
                    manifest["download_url_template"],
                    url.as_str(),
                    "{} url drift",
                    model.name
                ),
                Origin::PreparedLocal {
                    revision,
                    source_url,
                    ..
                } => {
                    assert!(manifest["download_url_template"].is_null());
                    assert_eq!(manifest["source_revision"], revision.as_str());
                    assert_eq!(manifest["source_url"], source_url.as_str());
                }
                _ => unreachable!(),
            }
        }
        // And nothing in the TOML is unknown to the legacy download path.
        for record in &stt {
            for name in std::iter::once(&record.id).chain(&record.aliases) {
                assert!(
                    crate::model::lookup_model(name).is_ok(),
                    "{name} is in the TOML but not in model::MODELS"
                );
            }
        }
    }

    #[cfg(feature = "tts")]
    #[test]
    fn toml_tts_records_match_the_legacy_table_exactly() {
        use crate::tts::catalogue as tts;
        let document = builtin_document().unwrap();
        let shipped: Vec<_> = tts::MODELS.iter().filter(|model| model.shipped).collect();
        for model in &shipped {
            let record = document
                .records
                .iter()
                .find(|record| record.id == model.id)
                .unwrap_or_else(|| panic!("{} missing from model-catalogue.v1.toml", model.id));
            assert_eq!(record.direction, Direction::Tts);
            assert_eq!(record.license, model.license);
            assert_eq!(record.languages, model.languages);
            let Origin::TtsPack {
                adapter,
                files,
                voices,
                max_phoneme_tokens,
                sample_rate_hz,
                shipped,
            } = &record.origin
            else {
                panic!("{} is not a tts_pack", model.id);
            };
            assert_eq!(adapter, model.adapter);
            assert_eq!(*max_phoneme_tokens, model.max_phoneme_tokens);
            assert_eq!(*sample_rate_hz, model.sample_rate_hz);
            assert!(*shipped);
            let expected: Vec<_> = [model.onnx, model.voices, model.config]
                .iter()
                .map(|file| {
                    (
                        file.filename.to_string(),
                        tts::pack_file_url(model, file),
                        file.size_bytes,
                        file.sha256.to_string(),
                    )
                })
                .collect();
            let actual: Vec<_> = files
                .iter()
                .map(|file| {
                    (
                        file.filename.clone(),
                        file.url.clone(),
                        file.size_bytes,
                        file.sha256.clone(),
                    )
                })
                .collect();
            assert_eq!(actual, expected, "{} pack file drift", model.id);
            let expected_voices: Vec<_> = tts::VOICES
                .iter()
                .filter(|voice| voice.model == model.id)
                .map(|voice| (voice.id, voice.internal_key, voice.language))
                .collect();
            let actual_voices: Vec<_> = voices
                .iter()
                .map(|voice| {
                    (
                        voice.id.as_str(),
                        voice.internal_key.as_str(),
                        voice.language.as_str(),
                    )
                })
                .collect();
            assert_eq!(actual_voices, expected_voices, "{} voice drift", model.id);
        }
        let tts_records = document
            .records
            .iter()
            .filter(|record| record.direction == Direction::Tts)
            .count();
        assert_eq!(tts_records, shipped.len(), "TOML has unshipped TTS records");
    }

    #[test]
    fn builtin_urls_use_reviewed_hosts_and_immutable_revisions() {
        // builtin_document() enforces this; assert the policy itself too.
        assert!(builtin_document().is_ok());
        for bad in [
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
            "https://example.com/ggml-base.bin",
            "http://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin",
            "https://github.com/org/repo/raw/main/model.onnx",
            // A release asset outside the reviewed (owner, repo, tag) list.
            "https://github.com/someone/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx",
            "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.1/kokoro-v1.0.int8.onnx",
            "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx?x=1",
        ] {
            assert!(
                reviewed_builtin_url(bad, UrlKind::Download).is_err(),
                "{bad}"
            );
        }
        let resolve = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin";
        assert!(reviewed_builtin_url(resolve, UrlKind::Download).is_ok());
        let release = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin";
        assert!(reviewed_builtin_url(release, UrlKind::Download).is_ok());
        // A `tree/` URL is a repository view: it may name a preparation
        // source, but it is never something Aurum downloads.
        let tree = "https://huggingface.co/inesc-id/WhisperLv3-FT/tree/77837e42b56d4be6ca15a66b5c41c9b8cf3e41b0";
        assert!(reviewed_builtin_url(tree, UrlKind::Download).is_err());
        assert!(reviewed_builtin_url(tree, UrlKind::PreparationSource).is_ok());
    }

    #[test]
    fn effective_digest_covers_defaults() {
        let builtin = EffectiveCatalogue::builtin().unwrap();
        let deployment =
            CatalogueDocument::parse("schema_version = 1\n[defaults.stt]\nglobal = \"tiny\"\n")
                .unwrap();
        let moved = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, PathBuf::from("/tmp/deploy.toml"))),
        )
        .unwrap();
        // Same records, different default: a different effective catalogue.
        assert_eq!(builtin.records().len(), moved.records().len());
        assert_ne!(builtin.digest(), moved.digest());
    }

    #[cfg(not(feature = "tts"))]
    #[test]
    fn no_tts_build_has_a_valid_stt_only_catalogue() {
        let catalogue = EffectiveCatalogue::builtin().unwrap();
        assert!(catalogue
            .records()
            .iter()
            .all(|record| { record.record.direction == Direction::Stt }));
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, None, None)
                .unwrap()
                .record
                .id,
            "base"
        );
    }
    #[test]
    fn resolver_prefers_cli_then_configured_then_global() {
        let catalogue = EffectiveCatalogue::builtin().unwrap();
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, Some("tiny"), Some("small"))
                .unwrap()
                .record
                .id,
            "tiny"
        );
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, None, Some("small"))
                .unwrap()
                .record
                .id,
            "small"
        );
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, None, None)
                .unwrap()
                .record
                .id,
            "base"
        );
        // Experimental specialists stay reachable by explicit id.
        assert_eq!(
            catalogue
                .resolve(Direction::Stt, None, Some("medium-ptbr-q5_0"))
                .unwrap()
                .record
                .id,
            "medium-ptbr-q5_0"
        );
    }

    #[test]
    fn builtin_defaults_are_supported_local_records() {
        let catalogue = EffectiveCatalogue::builtin().unwrap();
        let stt = catalogue.resolve(Direction::Stt, None, None).unwrap();
        assert_eq!(stt.record.id, "base");
        assert_eq!(stt.record.tier, SupportTier::Supported);
    }

    #[test]
    fn language_defaults_are_rejected_by_the_schema() {
        let deployment = "schema_version = 1\n[defaults.stt.language]\npt-BR = \"tiny\"\n";
        assert!(CatalogueDocument::parse(deployment).is_err());
    }

    #[test]
    fn experimental_records_cannot_be_defaults() {
        for id in ["medium-ptbr-q5_0", "large-v3-ptpt-q5_0", "large-v3-q5_0"] {
            let deployment = CatalogueDocument::parse(&format!(
                "schema_version = 1\n[defaults.stt]\nglobal = \"{id}\"\n"
            ))
            .unwrap();
            let err = EffectiveCatalogue::from_documents(
                builtin_document().unwrap(),
                Some((deployment, PathBuf::from("/tmp/deploy.toml"))),
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("must be a supported record"), "{id}: {err}");
        }
        let explicit_only = CatalogueDocument::parse(r#"schema_version = 1
[defaults.stt]
global = "pinned-only"
[[model]]
id = "pinned-only"
direction = "stt"
provider = "local"
tier = "explicit_only"
origin = { kind = "downloadable_local", filename = "pinned-only.bin", url = "https://example.invalid/pinned-only.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#).unwrap();
        assert!(EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((explicit_only, PathBuf::from("/tmp/deploy.toml"))),
        )
        .is_err());
    }
    const DISABLE_TINY: &str = r#"schema_version = 1
[[model]]
id = "tiny"
direction = "stt"
provider = "local"
enabled = false
tier = "supported"
origin = { kind = "downloadable_local", filename = "tiny.bin", url = "https://example.com/tiny.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#;

    #[test]
    fn deployment_disable_removes_the_record() {
        let effective = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((
                CatalogueDocument::parse(DISABLE_TINY).unwrap(),
                PathBuf::from("/tmp/deploy.toml"),
            )),
        )
        .unwrap();
        assert!(effective.lookup("tiny").is_none());
        assert!(effective.lookup("base").is_some());
        assert!(effective
            .records()
            .iter()
            .all(|entry| entry.source == CatalogueSource::Builtin));
    }

    #[test]
    fn deployment_cannot_add_replace_or_touch_tts_records() {
        let sha = "a".repeat(64);
        let stt = |id: &str| {
            format!(
                r#"schema_version = 1
[[model]]
id = "{id}"
direction = "stt"
provider = "local"
tier = "supported"
origin = {{ kind = "downloadable_local", filename = "x.bin", url = "https://example.invalid/x.bin", size_bytes = 42, sha256 = "{sha}" }}
"#
            )
        };
        let tts = format!(
            r#"schema_version = 1
[[model]]
id = "kitten-nano-int8"
direction = "tts"
provider = "local"
enabled = false
tier = "supported"
origin = {{ kind = "tts_pack", adapter = "kitten-onnx-v1", files = [{{ filename = "m.onnx", url = "https://example.invalid/m.onnx", size_bytes = 1, sha256 = "{sha}" }}], voices = [{{ id = "v", internal_key = "v", language = "en" }}], max_phoneme_tokens = 1, sample_rate_hz = 1, shipped = true }}
"#
        );
        for (case, document) in [
            ("new id", stt("deployment-only")),
            ("replacement", stt("base")),
            ("tts record", tts),
            (
                "tts default",
                "schema_version = 1\n[defaults.tts]\nglobal = \"kitten-nano-int8\"\n".into(),
            ),
        ] {
            let err = EffectiveCatalogue::from_documents(
                builtin_document().unwrap(),
                Some((
                    CatalogueDocument::parse(&document).unwrap(),
                    PathBuf::from("/tmp/deploy.toml"),
                )),
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("can currently only"), "{case}: {err}");
            assert!(err.contains(DEPLOYMENT_RECORDS_ISSUE), "{case}: {err}");
        }
    }

    #[test]
    fn effective_digest_changes_when_a_record_is_disabled() {
        let builtin = EffectiveCatalogue::builtin().unwrap();
        let changed = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((
                CatalogueDocument::parse(DISABLE_TINY).unwrap(),
                PathBuf::from("/tmp/changed.toml"),
            )),
        )
        .unwrap();
        assert_ne!(builtin.digest(), changed.digest());
    }

    #[test]
    fn deployment_default_can_target_builtin_record() {
        let deployment =
            CatalogueDocument::parse("schema_version = 1\n[defaults.stt]\nglobal = \"tiny\"\n")
                .unwrap();
        let effective = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, PathBuf::from("/tmp/deploy.toml"))),
        )
        .unwrap();
        assert_eq!(
            effective
                .resolve(Direction::Stt, None, None)
                .unwrap()
                .record
                .id,
            "tiny"
        );
    }

    #[test]
    fn remote_records_are_not_catalogue_records() {
        // The old remote origin kind no longer exists in the schema.
        let remote_origin = r#"schema_version = 1
[[model]]
id = "remote"
direction = "stt"
provider = "openai"
tier = "supported"
origin = { kind = "remote", wire_model = "whisper-1", capabilities = { timestamps_reliable = true } }
"#;
        assert!(CatalogueDocument::parse(remote_origin).is_err());
        // A non-local provider is rejected even with a local-looking origin, so
        // a deployment can never make a remote provider implicit.
        let remote_provider = r#"schema_version = 1
[defaults.stt]
global = "remote"
[[model]]
id = "remote"
direction = "stt"
provider = "openai"
tier = "supported"
origin = { kind = "downloadable_local", filename = "remote.bin", url = "https://example.invalid/remote.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#;
        let err = CatalogueDocument::parse(remote_provider)
            .unwrap_err()
            .to_string();
        assert!(err.contains("provider = \"local\""), "{err}");
        // And the built-in catalogue carries no remote rows at all.
        assert!(builtin_document()
            .unwrap()
            .records
            .iter()
            .all(|record| record.provider == "local"));
    }

    #[test]
    fn alias_disable_removes_the_canonical_record() {
        let deployment = CatalogueDocument::parse(r#"schema_version = 1
[[model]]
id = "turbo"
direction = "stt"
provider = "local"
enabled = false
tier = "supported"
origin = { kind = "downloadable_local", filename = "unused.bin", url = "https://example.invalid/unused.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
"#).unwrap();
        let effective = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, PathBuf::from("/tmp/deploy.toml"))),
        )
        .unwrap();
        assert!(effective.lookup("large-v3-turbo").is_none());
        assert!(effective.lookup("turbo").is_none());
    }

    /// One STT disable entry per name, in a deployment document.
    fn disable_entries(names: &[&str]) -> CatalogueDocument {
        let mut text = String::from("schema_version = 1\n");
        for name in names {
            text.push_str(&format!(
                r#"[[model]]
id = "{name}"
direction = "stt"
provider = "local"
enabled = false
tier = "supported"
origin = {{ kind = "downloadable_local", filename = "unused.bin", url = "https://example.invalid/unused.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }}
"#
            ));
        }
        CatalogueDocument::parse(&text).unwrap()
    }

    /// Until #146 a deployment only narrows: whatever it disables, every
    /// effective record stays `Builtin` and the effect is reported through
    /// `disabled()` by canonical id (an alias entry reports its canonical id).
    #[test]
    fn narrowing_deployment_never_produces_deployment_sources() {
        let effective = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((
                disable_entries(&["large", "tiny", "turbo-q5_0"]),
                PathBuf::from("/tmp/deploy.toml"),
            )),
        )
        .unwrap();
        assert!(effective
            .records()
            .iter()
            .all(|entry| entry.source == CatalogueSource::Builtin));
        let disabled = effective.disabled();
        assert_eq!(disabled.len(), 3, "{disabled:?}");
        assert!(disabled.contains(&"tiny".to_string()));
        assert!(disabled.contains(&"large-v3".to_string()), "{disabled:?}");
        assert!(disabled.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(EffectiveCatalogue::builtin().unwrap().disabled().is_empty());
    }

    #[test]
    fn disabling_a_model_and_its_alias_together_is_accepted() {
        for names in [["large-v3", "large"], ["large", "large-v3"]] {
            let effective = EffectiveCatalogue::from_documents(
                builtin_document().unwrap(),
                Some((disable_entries(&names), PathBuf::from("/tmp/deploy.toml"))),
            )
            .unwrap();
            assert!(effective.lookup("large-v3").is_none(), "{names:?}");
            assert!(effective.lookup("large").is_none(), "{names:?}");
        }
        // A name that never existed still fails closed.
        assert!(EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((
                disable_entries(&["large-v3", "no-such-model"]),
                PathBuf::from("/tmp/deploy.toml"),
            )),
        )
        .is_err());
    }

    #[cfg(feature = "tts")]
    #[test]
    fn stt_disable_entry_cannot_remove_a_tts_record() {
        // The entry declares `direction = "stt"`, so only the matched record's
        // direction can stop it from silently dropping a TTS pack.
        for name in ["kokoro-82m-int8", "kitten-nano-int8"] {
            let err = EffectiveCatalogue::from_documents(
                builtin_document().unwrap(),
                Some((disable_entries(&[name]), PathBuf::from("/tmp/deploy.toml"))),
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("not an STT model"), "{name}: {err}");
        }
    }

    #[test]
    fn digest_does_not_depend_on_deployment_path() {
        let deployment = CatalogueDocument::parse(DISABLE_TINY).unwrap();
        let a = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment.clone(), PathBuf::from("/tmp/a.toml"))),
        )
        .unwrap();
        let b = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, PathBuf::from("/elsewhere/b.toml"))),
        )
        .unwrap();
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn effective_names_cannot_shadow_another_record() {
        // An experimental record aliased as `base` would sort before the real
        // `base` and become the implicit default; it must be rejected instead.
        let mut records = EffectiveCatalogue::builtin().unwrap().records;
        let mut shadow = records
            .iter()
            .find(|entry| entry.record.id == "medium-ptbr-q5_0")
            .unwrap()
            .clone();
        shadow.record.id = "aaa-shadow".into();
        shadow.record.aliases = vec!["BASE".into()];
        records.push(shadow);
        let err = reject_effective_name_collisions(&records)
            .unwrap_err()
            .to_string();
        assert!(err.contains("used by both"), "{err}");
        assert!(
            reject_effective_name_collisions(EffectiveCatalogue::builtin().unwrap().records())
                .is_ok()
        );
    }

    #[test]
    fn artifact_filenames_and_ids_are_single_path_components() {
        let record = |id: &str, filename: &str| {
            format!(
                r#"schema_version = 1
[[model]]
id = "{id}"
direction = "stt"
provider = "local"
tier = "supported"
origin = {{ kind = "downloadable_local", filename = "{filename}", url = "https://example.invalid/x.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }}
"#
            )
        };
        for filename in [
            "../x.bin",
            "../../../.config/x",
            "a/b.bin",
            "a\\\\b.bin",
            ".hidden",
            ".",
            "..",
        ] {
            assert!(
                CatalogueDocument::parse(&record("ok", filename)).is_err(),
                "{filename}"
            );
        }
        assert!(CatalogueDocument::parse(&record("org/model", "x.bin")).is_err());
        assert!(CatalogueDocument::parse(&record("ok", "ggml-x_q5.0.bin")).is_ok());
    }

    #[test]
    fn urls_require_the_default_https_port() {
        let record = |url: &str| {
            format!(
                r#"schema_version = 1
[[model]]
id = "porty"
direction = "stt"
provider = "local"
tier = "supported"
origin = {{ kind = "downloadable_local", filename = "x.bin", url = "{url}", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }}
"#
            )
        };
        assert!(CatalogueDocument::parse(&record("https://example.invalid:8443/x.bin")).is_err());
        assert!(CatalogueDocument::parse(&record("https://example.invalid:443/x.bin")).is_ok());
        // A non-default port cannot smuggle a reviewed host past the built-in policy.
        assert!(reviewed_builtin_url(
            "https://huggingface.co:8443/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin",
            UrlKind::Download
        )
        .is_err());
    }

    #[test]
    fn prepared_local_revision_must_be_immutable_and_match_source_url() {
        let record = |source_url: &str, revision: &str| {
            format!(
                r#"schema_version = 1
[[model]]
id = "prepared"
direction = "stt"
provider = "local"
tier = "explicit_only"
origin = {{ kind = "prepared_local", filename = "p.bin", size_bytes = 1, sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", source_url = "{source_url}", revision = "{revision}", preparation = "scripts/prepare.sh" }}
"#
            )
        };
        let rev = "77837e42b56d4be6ca15a66b5c41c9b8cf3e41b0";
        let tree = format!("https://huggingface.co/inesc-id/WhisperLv3-FT/tree/{rev}");
        assert!(CatalogueDocument::parse(&record(&tree, rev)).is_ok());
        // A branch-like revision is refused.
        assert!(CatalogueDocument::parse(&record(&tree, "main")).is_err());
        // A revision that does not name the source path is refused.
        assert!(CatalogueDocument::parse(&record(
            &tree,
            "0000000000000000000000000000000000000000"
        ))
        .is_err());
    }

    #[test]
    fn default_named_by_alias_is_validated_as_the_resolved_record() {
        // `large` is an alias of the supported `large-v3`.
        let deployment =
            CatalogueDocument::parse("schema_version = 1\n[defaults.stt]\nglobal = \"large\"\n")
                .unwrap();
        let effective = EffectiveCatalogue::from_documents(
            builtin_document().unwrap(),
            Some((deployment, PathBuf::from("/tmp/deploy.toml"))),
        )
        .unwrap();
        let resolved = effective.resolve(Direction::Stt, None, None).unwrap();
        assert_eq!(resolved.record.id, "large-v3");
        assert_eq!(resolved.record.tier, SupportTier::Supported);
    }
}
