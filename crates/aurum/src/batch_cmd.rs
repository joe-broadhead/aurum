//! `aurum batch` — content-addressed resumable multi-file transcription (JOE-1726 / JOE-2220).

use aurum_core::audio;
use aurum_core::batch::{
    acquire_batch_lock, build_items, discover_inputs, manifest_path, merge_for_resume,
    operation_fingerprint, prepare_resume, sha256_file_full, truncate_error,
    validate_batch_stt_provider, BatchItemStatus, BatchManifest, OperationFingerprintInput,
    BATCH_MANIFEST_NAME,
};
use aurum_core::cleanup::{
    apply_cleanup_with_segments, CleanupProviderKind, CleanupStyle, OpenRouterCleanup,
    RulesCleanup, SegmentCleanupPolicy, TextCleanup,
};
use aurum_core::config::Config;
use aurum_core::error::{Result, UserError};
use aurum_core::output::{self, CommitMode, OutputFormat};
use aurum_core::profile::{resolve_profile, QualityProfile, PROFILE_EVIDENCE_VERSION};
use aurum_core::providers::{OpenRouterSttMode, TranscriptionOptions};
use aurum_core::remote::RemotePolicy;
use clap::Parser;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

/// `aurum batch` — process a file or folder with a versioned resume manifest.
#[derive(Debug, Parser)]
pub struct BatchCli {
    /// Audio file or directory of audio files.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Directory for transcripts and the batch manifest.
    #[arg(long = "output-dir", short = 'O', value_name = "DIR")]
    pub output_dir: PathBuf,

    /// Recurse into subdirectories when INPUT is a directory.
    #[arg(long)]
    pub recursive: bool,

    /// Exact-match resume only (full source/output SHA-256 + operation fingerprint).
    #[arg(long)]
    pub resume: bool,

    /// Retry items marked failed or interrupted when resuming.
    #[arg(long = "retry-failed")]
    pub retry_failed: bool,

    /// Opt in to reprocessing stale source/config/output items.
    #[arg(long = "reprocess-changed")]
    pub reprocess_changed: bool,

    /// Report resume decisions without transcription.
    #[arg(long = "verify-only")]
    pub verify_only: bool,

    /// Dry-run: write/update the manifest only (no transcription).
    #[arg(long)]
    pub dry_run: bool,

    /// Transcription provider (validated against the provider registry).
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Explicit model id (overrides --profile).
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// Intent profile: speed | balance | quality (ignored when --model is set).
    #[arg(long, value_name = "PROFILE")]
    pub profile: Option<String>,

    /// Language code or auto.
    #[arg(long, value_name = "CODE")]
    pub language: Option<String>,

    /// Output format for each item.
    #[arg(short = 'o', long = "output", value_name = "txt|srt|json", value_parser = ["txt", "srt", "json"])]
    pub output: Option<String>,

    /// Include timestamps (implied by srt).
    #[arg(long)]
    pub timestamps: bool,

    #[arg(long = "allow-unreliable-timestamps")]
    pub allow_unreliable_timestamps: bool,

    #[arg(long = "openrouter-stt-mode", value_name = "auto|chat|transcriptions")]
    pub openrouter_stt_mode: Option<String>,

    #[arg(
        long = "cleanup",
        value_name = "raw|clean|bullets|professional|summary"
    )]
    pub cleanup: Option<String>,

    #[arg(long = "cleanup-provider", value_name = "rules|openrouter")]
    pub cleanup_provider: Option<String>,

    #[arg(long = "cleanup-model", value_name = "MODEL")]
    pub cleanup_model: Option<String>,

    #[arg(long = "cleanup-segments", value_name = "auto|keep|clear|per-segment")]
    pub cleanup_segments: Option<String>,

    /// Emit final summary as JSON on stdout.
    #[arg(long)]
    pub json: bool,

    #[arg(short = 'v', long)]
    pub verbose: bool,
}

pub async fn run_batch(cli: BatchCli) -> Result<()> {
    let mut cfg = Config::load()?;
    cfg.apply_cli(
        cli.provider.as_deref(),
        None,
        cli.language.as_deref(),
        cli.output.as_deref(),
        None,
        cli.timestamps,
        cli.verbose,
        cli.cleanup.as_deref(),
        cli.cleanup_provider.as_deref(),
        cli.cleanup_model.as_deref(),
    );
    let cfg = aurum_core::ValidatedConfig::try_from_config(cfg)?.into_config();
    let mut cfg = cfg;

    let profile_name = cli.profile.clone();
    let mut profile_evidence: Option<String> = None;
    let model = if let Some(ref m) = cli.model {
        m.clone()
    } else if let Some(ref p) = cli.profile {
        let profile = QualityProfile::parse(p)?;
        let res = resolve_profile(profile, &cfg.language)?;
        profile_evidence = Some(res.evidence_version.clone());
        if cli.verbose || atty_stderr() {
            eprintln!(
                "aurum batch: profile {} → model {} (evidence {})",
                res.profile, res.model, res.evidence_version
            );
        }
        res.model
    } else {
        cfg.resolve_model(false)?
    };
    cfg.model = Some(model.clone());

    let format = OutputFormat::parse(&cfg.output)?;
    let want_timestamps = cfg.timestamps || matches!(format, OutputFormat::Srt);
    let provider_name = cfg.provider.to_ascii_lowercase();

    std::fs::create_dir_all(&cli.output_dir).map_err(|e| UserError::Other {
        message: format!("create output dir {}: {e}", cli.output_dir.display()),
    })?;

    let man_path = manifest_path(&cli.output_dir);
    let sources = discover_inputs(&cli.input, cli.recursive)?;

    let cleanup_style = CleanupStyle::parse(&cfg.cleanup_style)?;
    let cleanup_kind = CleanupProviderKind::parse(&cfg.cleanup_provider)?;
    let segment_policy = cli
        .cleanup_segments
        .as_deref()
        .map(SegmentCleanupPolicy::parse)
        .transpose()?
        .unwrap_or(SegmentCleanupPolicy::Auto);

    let stt_mode_raw = cli
        .openrouter_stt_mode
        .as_deref()
        .unwrap_or(cfg.openrouter_stt_mode.as_str());
    let stt_mode = OpenRouterSttMode::parse(stt_mode_raw)?;

    let op_fp_input = OperationFingerprintInput {
        provider_id: provider_name.clone(),
        backend_route: if provider_name == "local" {
            "whisper_cpp".into()
        } else {
            format!("remote/{stt_mode_raw}")
        },
        model_id: model.clone(),
        support_evidence: profile_evidence.clone(),
        language: cfg.language.clone(),
        timestamps: want_timestamps,
        allow_unreliable_timestamps: cli.allow_unreliable_timestamps,
        output_format: format.as_str().into(),
        cleanup_style: cleanup_style.as_str().into(),
        cleanup_provider: cleanup_kind.as_str().into(),
        cleanup_model: cfg.cleanup_openrouter_model.clone(),
        cleanup_segments: segment_policy.as_str().into(),
        long_form_policy: None,
        dto_schema_version: aurum_core::STT_RESULT_SCHEMA_VERSION.to_string(),
        profile: profile_name.clone(),
        profile_evidence_version: profile_evidence
            .clone()
            .or_else(|| Some(PROFILE_EVIDENCE_VERSION.into())),
        local_only: cfg.local_only,
        trust_mode: None,
        aurum_behavior_version: env!("CARGO_PKG_VERSION").into(),
    };
    let op_fp = operation_fingerprint(&op_fp_input);

    // Temporary run_id for lock before manifest exists.
    let provisional_run_id = format!("batch-{}", std::process::id());
    let _lock = acquire_batch_lock(&cli.output_dir, &provisional_run_id)?;

    let mut manifest = if cli.resume && man_path.exists() {
        let mut m = BatchManifest::load(&man_path)?;
        if m.operation_fingerprint != op_fp && !cli.reprocess_changed {
            return Err(UserError::Other {
                message: format!(
                    "resume manifest at {} has a different operation fingerprint.\n  \
                     Hint: pass --reprocess-changed to reprocess under the new options, \
                     or match the original provider/model/language/format/cleanup/profile",
                    man_path.display()
                ),
            }
            .into());
        }
        // Refresh fingerprint when reprocessing under new options.
        if m.operation_fingerprint != op_fp && cli.reprocess_changed {
            m.operation_fingerprint = op_fp.clone();
            m.provider = provider_name.clone();
            m.model = model.clone();
            m.language = cfg.language.clone();
            m.output_format = format.as_str().into();
            m.profile = profile_name.clone();
        }
        merge_for_resume(&mut m, &sources, format);
        m
    } else {
        if man_path.exists() && !cli.resume {
            return Err(UserError::Other {
                message: format!(
                    "batch manifest already exists at {}\n  \
                     Hint: pass --resume to continue, or choose a fresh --output-dir",
                    man_path.display()
                ),
            }
            .into());
        }
        let mut m = BatchManifest::new(
            &provider_name,
            &model,
            &cfg.language,
            format,
            &cli.output_dir,
            profile_name.as_deref(),
            &op_fp,
        );
        m.items = build_items(&sources, format);
        m
    };

    // Align lock run_id documentation: persist manifest early.
    manifest.save(&man_path)?;
    if cli.verbose || atty_stderr() {
        eprintln!(
            "aurum batch: {} item(s) → {} ({BATCH_MANIFEST_NAME}, schema v{})",
            manifest.items.len(),
            cli.output_dir.display(),
            manifest.schema_version
        );
    }

    let work = prepare_resume(
        &mut manifest,
        &op_fp,
        cli.retry_failed,
        cli.reprocess_changed,
    )?;
    manifest.save(&man_path)?;

    if cli.verify_only || cli.dry_run {
        let summary = manifest.summary();
        if cli.verify_only && (cli.verbose || atty_stderr()) {
            eprintln!(
                "aurum batch verify-only: {} item(s) would be processed",
                work.len()
            );
        }
        print_summary(&manifest, &summary, cli.json)?;
        return Ok(());
    }

    let engine = aurum_core::AurumEngine::from_config(cfg.clone())?;
    let provider_id = validate_batch_stt_provider(engine.registry(), &provider_name)?;
    aurum_core::preflight_stt_with_registry(
        engine.registry(),
        &provider_id,
        &model,
        matches!(format, OutputFormat::Srt),
        cfg.local_only,
        stt_mode,
    )?;
    let provider = engine.stt_provider_with(
        &provider_id,
        aurum_core::ProviderResolveOptions {
            show_progress: cli.verbose,
            stt_mode: Some(stt_mode),
            local_only: Some(cfg.local_only),
        },
    )?;

    for idx in work {
        let source = PathBuf::from(&manifest.items[idx].source);
        let out_rel = manifest.items[idx].output.clone();
        let out_path = cli.output_dir.join(&out_rel);

        // 1. compute/verify source identity
        let (src_digest, src_size) = match sha256_file_full(&source) {
            Ok(v) => v,
            Err(e) => {
                manifest.items[idx].status = BatchItemStatus::Failed;
                manifest.items[idx].error = Some(truncate_error(&e.to_string()));
                manifest.items[idx].attempts = manifest.items[idx].attempts.saturating_add(1);
                manifest.touch();
                manifest.save(&man_path)?;
                continue;
            }
        };

        // 2. mark running and persist
        manifest.items[idx].status = BatchItemStatus::Running;
        manifest.items[idx].attempts = manifest.items[idx].attempts.saturating_add(1);
        manifest.items[idx].source_sha256 = Some(src_digest.clone());
        manifest.items[idx].source_size = Some(src_size);
        manifest.items[idx].operation_fingerprint = Some(op_fp.clone());
        manifest.items[idx].started_at_unix = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        manifest.items[idx].error = None;
        manifest.touch();
        manifest.save(&man_path)?;

        if atty_stderr() || cli.verbose {
            eprintln!(
                "aurum batch: [{}/{}] {}",
                idx + 1,
                manifest.items.len(),
                source.display()
            );
        }

        // 3. execute and transactionally publish transcript
        let item_result = async {
            let audio = audio::load_audio(&source).await?;
            let options = TranscriptionOptions {
                model: model.clone(),
                language: cfg.language.clone(),
                timestamps: want_timestamps,
                cancel: None,
            };
            let mut result = provider.transcribe(&audio, &options).await?;

            if matches!(format, OutputFormat::Srt)
                && !result.timestamps_reliable()
                && !cli.allow_unreliable_timestamps
            {
                return Err(UserError::Other {
                    message: "unreliable timestamps for SRT; pass --allow-unreliable-timestamps"
                        .into(),
                }
                .into());
            }

            apply_cleanup_local(
                &mut result,
                &cfg,
                cleanup_style,
                cleanup_kind,
                segment_policy,
                cli.verbose,
            )
            .await?;

            output::write_result_to_path(&result, format, &out_path, CommitMode::Replace)?;
            // 4. hash the committed output
            let (out_digest, out_size) = sha256_file_full(&out_path)?;
            Ok::<_, aurum_core::error::TranscriptionError>((out_digest, out_size))
        }
        .await;

        match item_result {
            Ok((out_digest, out_size)) => {
                // 5. mark succeeded with output identity and persist
                manifest.items[idx].status = BatchItemStatus::Succeeded;
                manifest.items[idx].error = None;
                manifest.items[idx].output_sha256 = Some(out_digest);
                manifest.items[idx].output_size = Some(out_size);
                manifest.items[idx].finished_at_unix = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                );
            }
            Err(e) => {
                manifest.items[idx].status = BatchItemStatus::Failed;
                manifest.items[idx].error = Some(truncate_error(&e.to_string()));
                manifest.items[idx].finished_at_unix = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                );
                if cli.verbose {
                    eprintln!("aurum batch: failed {}: {e}", source.display());
                }
            }
        }
        manifest.touch();
        manifest.save(&man_path)?;
    }

    let summary = manifest.summary();
    print_summary(&manifest, &summary, cli.json)?;
    engine.shutdown();

    if summary.failed > 0 {
        return Err(UserError::Other {
            message: format!(
                "batch completed with {} failure(s) of {} (manifest: {})",
                summary.failed,
                summary.total,
                man_path.display()
            ),
        }
        .into());
    }
    Ok(())
}

async fn apply_cleanup_local(
    result: &mut aurum_core::TranscriptionResult,
    cfg: &Config,
    style: CleanupStyle,
    kind: CleanupProviderKind,
    segments: SegmentCleanupPolicy,
    verbose: bool,
) -> Result<()> {
    if matches!(style, CleanupStyle::Raw) {
        result.set_cleanup_style(CleanupStyle::Raw);
        result.set_cleanup_provider(None);
        result.set_original_text(None);
        result.set_original_segments(None);
        result.set_cleanup_segment_policy(None);
        return Ok(());
    }
    let backend: Box<dyn TextCleanup> = match kind {
        CleanupProviderKind::Rules => Box::new(RulesCleanup::new()),
        CleanupProviderKind::OpenRouter => {
            let policy = RemotePolicy {
                allow_custom_credentialed_endpoint: cfg.openrouter_allow_custom_endpoint,
                use_system_proxy: cfg.openrouter_use_system_proxy,
                allow_loopback_http: cfg.openrouter_base_url.contains("127.0.0.1")
                    || cfg.openrouter_base_url.contains("localhost"),
                ..Default::default()
            };
            Box::new(OpenRouterCleanup::with_policy(
                cfg.openrouter_api_key.clone(),
                Some(cfg.openrouter_base_url.clone()),
                cfg.cleanup_openrouter_model
                    .clone()
                    .or_else(|| Some(cfg.openrouter_default_model.clone())),
                policy,
            )?)
        }
    };
    let (_out, report) =
        apply_cleanup_with_segments(result, backend.as_ref(), style, segments).await?;
    if verbose {
        for w in &report.warnings {
            eprintln!("aurum: cleanup note: {w}");
        }
    }
    Ok(())
}

fn print_summary(
    manifest: &BatchManifest,
    summary: &aurum_core::batch::BatchSummary,
    json: bool,
) -> Result<()> {
    if json {
        let mut v = serde_json::to_value(summary).map_err(|e| UserError::Other {
            message: format!("summary json: {e}"),
        })?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "manifest".into(),
                serde_json::Value::String(
                    manifest_path(std::path::Path::new(&manifest.output_dir))
                        .display()
                        .to_string(),
                ),
            );
            obj.insert(
                "model".into(),
                serde_json::Value::String(manifest.model.clone()),
            );
            obj.insert(
                "provider".into(),
                serde_json::Value::String(manifest.provider.clone()),
            );
            obj.insert(
                "operation_fingerprint".into(),
                serde_json::Value::String(manifest.operation_fingerprint.clone()),
            );
            obj.insert(
                "run_id".into(),
                serde_json::Value::String(manifest.run_id.clone()),
            );
            obj.insert(
                "schema_version".into(),
                serde_json::json!(manifest.schema_version),
            );
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| UserError::Other {
                message: format!("summary json: {e}"),
            })?
        );
    } else {
        eprintln!(
            "aurum batch summary: total={} succeeded={} failed={} pending={} interrupted={} stale_source={} stale_config={} stale_output={}",
            summary.total,
            summary.succeeded,
            summary.failed,
            summary.pending,
            summary.interrupted,
            summary.stale_source,
            summary.stale_configuration,
            summary.stale_output
        );
        eprintln!(
            "aurum batch: manifest {}",
            manifest_path(std::path::Path::new(&manifest.output_dir)).display()
        );
    }
    Ok(())
}

fn atty_stderr() -> bool {
    io::stderr().is_terminal()
}
