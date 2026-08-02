//! `aurum batch` — bounded resumable multi-file transcription (JOE-1726).

use aurum_core::audio;
use aurum_core::batch::{
    build_items, discover_inputs, fingerprint_file, manifest_path, merge_for_resume, work_indices,
    BatchItemStatus, BatchManifest, BATCH_MANIFEST_NAME,
};
use aurum_core::cleanup::{
    apply_cleanup_with_segments, CleanupProviderKind, CleanupStyle, OpenRouterCleanup,
    RulesCleanup, SegmentCleanupPolicy, TextCleanup,
};
use aurum_core::config::Config;
use aurum_core::error::{Result, UserError};
use aurum_core::output::{self, CommitMode, OutputFormat};
use aurum_core::profile::{resolve_profile, QualityProfile};
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

    /// Resume from an existing `aurum-batch-manifest.json` in --output-dir.
    #[arg(long)]
    pub resume: bool,

    /// Retry items marked failed when resuming.
    #[arg(long = "retry-failed")]
    pub retry_failed: bool,

    /// Dry-run: write/update the manifest only (no transcription).
    #[arg(long)]
    pub dry_run: bool,

    /// Transcription provider.
    #[arg(long, value_name = "local|openrouter", value_parser = ["local", "openrouter"])]
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
    let model = if let Some(ref m) = cli.model {
        m.clone()
    } else if let Some(ref p) = cli.profile {
        let profile = QualityProfile::parse(p)?;
        let res = resolve_profile(profile, &cfg.language)?;
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

    let mut manifest = if cli.resume && man_path.exists() {
        let mut m = BatchManifest::load(&man_path)?;
        if m.model != model || m.provider != provider_name || m.output_format != format.as_str() {
            return Err(UserError::Other {
                message: format!(
                    "resume manifest at {} was created with provider={}/model={}/format={} \
                     but this run uses {}/{}/{}\n  \
                     Hint: use a new --output-dir or match the original options",
                    man_path.display(),
                    m.provider,
                    m.model,
                    m.output_format,
                    provider_name,
                    model,
                    format.as_str()
                ),
            }
            .into());
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
        );
        m.items = build_items(&sources, format);
        m
    };

    manifest.save(&man_path)?;
    if cli.verbose || atty_stderr() {
        eprintln!(
            "aurum batch: {} item(s) → {} ({BATCH_MANIFEST_NAME})",
            manifest.items.len(),
            cli.output_dir.display()
        );
    }

    if cli.dry_run {
        let summary = manifest.summary();
        print_summary(&manifest, &summary, cli.json)?;
        return Ok(());
    }

    let provider_id = aurum_core::ProviderId::parse(&provider_name)?;
    let stt_mode_raw = cli
        .openrouter_stt_mode
        .as_deref()
        .unwrap_or(cfg.openrouter_stt_mode.as_str());
    let stt_mode = OpenRouterSttMode::parse(stt_mode_raw)?;
    // Engine-owned registry path for the whole batch (JOE-1795 / JOE-1938).
    let engine = aurum_core::AurumEngine::from_config(cfg.clone())?;
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

    let cleanup_style = CleanupStyle::parse(&cfg.cleanup_style)?;
    let cleanup_kind = CleanupProviderKind::parse(&cfg.cleanup_provider)?;
    let segment_policy = cli
        .cleanup_segments
        .as_deref()
        .map(SegmentCleanupPolicy::parse)
        .transpose()?
        .unwrap_or(SegmentCleanupPolicy::Auto);

    let indices = work_indices(&manifest, cli.retry_failed);
    for idx in indices {
        let source = PathBuf::from(&manifest.items[idx].source);
        let out_rel = manifest.items[idx].output.clone();
        let out_path = cli.output_dir.join(&out_rel);

        manifest.items[idx].status = BatchItemStatus::Running;
        manifest.items[idx].attempts = manifest.items[idx].attempts.saturating_add(1);
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

        let item_result = async {
            if !source.is_file() {
                return Err(UserError::FileNotFound {
                    path: source.display().to_string(),
                }
                .into());
            }
            let fp = fingerprint_file(&source).ok();
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
            Ok::<_, aurum_core::error::TranscriptionError>(fp)
        }
        .await;

        match item_result {
            Ok(fp) => {
                manifest.items[idx].status = BatchItemStatus::Succeeded;
                manifest.items[idx].error = None;
                manifest.items[idx].source_sha256 = fp;
            }
            Err(e) => {
                manifest.items[idx].status = BatchItemStatus::Failed;
                manifest.items[idx].error = Some(e.to_string());
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
                cfg.openrouter_api_key_exposed(),
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
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| UserError::Other {
                message: format!("summary json: {e}"),
            })?
        );
    } else {
        eprintln!(
            "aurum batch summary: total={} succeeded={} failed={} pending={} skipped={}",
            summary.total, summary.succeeded, summary.failed, summary.pending, summary.skipped
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
