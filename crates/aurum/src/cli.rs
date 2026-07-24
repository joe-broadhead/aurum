//! CLI definition and orchestration.

use aurum_core::audio;
use aurum_core::cleanup::{
    apply_cleanup, CleanupProviderKind, CleanupStyle, OpenRouterCleanup, RulesCleanup, TextCleanup,
};
use aurum_core::config::Config;
use aurum_core::error::{Result, TranscriptionError, UserError};
use aurum_core::model;
use aurum_core::output::{self, OutputFormat};
use aurum_core::providers::{
    BackendKind, LocalWhisperProvider, OpenRouterProvider, TranscriptionOptions,
    TranscriptionProvider,
};
use clap::{Parser, Subcommand};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

/// Aurum — local-first transcription CLI (Latin: gold).
#[derive(Debug, Parser)]
#[command(
    name = "aurum",
    version,
    about = "Local-first, cross-platform transcription CLI (Latin: gold)",
    long_about = "Aurum converts audio files to text using local whisper.cpp models by default, \
                  with optional OpenRouter remote transcription.\n\n\
                  Aurum is Latin for gold.\n\n\
                  Quick start:\n  \
                    aurum meeting.m4a\n  \
                    aurum meeting.m4a --model tiny-q5_1\n  \
                    aurum models\n  \
                    aurum --help"
)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub transcribe: TranscribeArgs,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// List local whisper models and cache status.
    #[command(visible_alias = "list-models")]
    Models,

    /// Transcribe an audio file (default when AUDIO_FILE is given positionally).
    #[command(visible_alias = "t")]
    Transcribe(TranscribeArgs),
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct TranscribeArgs {
    /// Audio file to transcribe.
    #[arg(value_name = "AUDIO_FILE")]
    pub audio_file: Option<PathBuf>,

    /// Transcription provider.
    #[arg(long, value_name = "local|openrouter", value_parser = ["local", "openrouter"])]
    pub provider: Option<String>,

    /// Local model name (tiny-q5_1, base, …) or OpenRouter model id.
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// Language code (e.g. en, fr) or "auto".
    #[arg(long, value_name = "CODE")]
    pub language: Option<String>,

    /// Output format.
    #[arg(short = 'o', long = "output", value_name = "txt|srt|json", value_parser = ["txt", "srt", "json"])]
    pub output: Option<String>,

    /// Write output to this path instead of stdout.
    #[arg(long = "output-file", value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Include timestamps where available (implied by srt).
    #[arg(long)]
    pub timestamps: bool,

    /// Allow SRT/timestamp output from OpenRouter despite unreliable timings.
    #[arg(long = "allow-unreliable-timestamps")]
    pub allow_unreliable_timestamps: bool,

    /// Post-transcript cleanup style (default: raw = off).
    #[arg(long = "cleanup", value_name = "raw|clean|bullets|professional|summary")]
    pub cleanup: Option<String>,

    /// Cleanup backend: on-device rules (default) or OpenRouter LLM.
    #[arg(long = "cleanup-provider", value_name = "rules|openrouter")]
    pub cleanup_provider: Option<String>,

    /// Model id when using --cleanup-provider openrouter.
    #[arg(long = "cleanup-model", value_name = "MODEL")]
    pub cleanup_model: Option<String>,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Entry point used by `main`.
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Commands::Models) => {
            init_tracing(false);
            let cfg = Config::load()?;
            print!("{}", model::format_model_list(&cfg.cache_dir));
            Ok(())
        }
        Some(Commands::Transcribe(args)) => run_transcribe(args).await,
        None => {
            if cli.transcribe.audio_file.is_none() {
                // No args at all — show a short nudge rather than clap's missing-arg only.
                eprintln!(
                    "aurum: missing AUDIO_FILE\n\n  \
                     Examples:\n    \
                     aurum meeting.m4a\n    \
                     aurum meeting.m4a --model tiny-q5_1 -o srt\n    \
                     aurum models\n\n  \
                     Run `aurum --help` for full options."
                );
                return Err(UserError::Other {
                    message: "AUDIO_FILE is required (or use `aurum models`)".into(),
                }
                .into());
            }
            run_transcribe(cli.transcribe).await
        }
    }
}

async fn run_transcribe(cli: TranscribeArgs) -> Result<()> {
    init_tracing(cli.verbose);

    let audio_file = cli.audio_file.clone().ok_or_else(|| UserError::Other {
        message: "AUDIO_FILE is required".into(),
    })?;

    let mut cfg = Config::load()?;
    cfg.apply_cli(
        cli.provider.as_deref(),
        cli.model.as_deref(),
        cli.language.as_deref(),
        cli.output.as_deref(),
        cli.output_file.as_deref(),
        cli.timestamps,
        cli.verbose,
        cli.cleanup.as_deref(),
        cli.cleanup_provider.as_deref(),
        cli.cleanup_model.as_deref(),
    );

    let model = cfg.resolve_model(cli.model.is_some());
    let provider_name = cfg.provider.to_ascii_lowercase();
    let format = OutputFormat::parse(&cfg.output)?;

    // SRT always needs timestamps.
    let want_timestamps = cfg.timestamps || matches!(format, OutputFormat::Srt);

    // OpenRouter timestamps are unreliable — refuse SRT unless explicitly overridden.
    if provider_name == "openrouter"
        && matches!(format, OutputFormat::Srt)
        && !cli.allow_unreliable_timestamps
    {
        return Err(UserError::Other {
            message:
                "OpenRouter is LLM-assisted and does not produce reliable media timestamps.\n  \
                      Use `-o txt` or `-o json` (json sets timestamps_reliable=false), or pass \
                      `--allow-unreliable-timestamps` to force SRT."
                    .into(),
        }
        .into());
    }

    if provider_name == "openrouter"
        && want_timestamps
        && !cli.allow_unreliable_timestamps
        && atty_stderr()
    {
        eprintln!(
            "aurum: warning: OpenRouter timestamps are best-effort only \
             (timestamps_reliable=false in JSON)"
        );
    }

    tracing::info!(
        provider = %provider_name,
        model = %model,
        language = %cfg.language,
        output = %format.as_str(),
        "starting transcription"
    );

    if !audio_file.exists() {
        return Err(UserError::FileNotFound {
            path: audio_file.display().to_string(),
        }
        .into());
    }
    if !audio_file.is_file() {
        return Err(UserError::InvalidAudio {
            reason: format!("{} is not a regular file", audio_file.display()),
        }
        .into());
    }

    if cli.verbose || atty_stderr() {
        eprintln!("aurum: loading audio …");
    }
    let audio = audio::load_audio(&audio_file).await?;
    if cli.verbose {
        eprintln!(
            "aurum: loaded {:.2}s of audio ({} samples @ {} Hz)",
            audio.duration_secs,
            audio.samples.len(),
            audio.sample_rate
        );
    }

    if atty_stderr() {
        eprintln!(
            "aurum: transcribing with {provider_name}/{model} ({:.1}s audio) …",
            audio.duration_secs
        );
    }

    // First-run tip when local model is not cached yet.
    if provider_name == "local" {
        if let Ok(info) = model::lookup_model(&model) {
            let path = model::model_path(&cfg.cache_dir, info);
            if !path.exists() && atty_stderr() {
                eprintln!(
                    "aurum: note: model `{model}` is not cached yet (~{}). \
                     For a quicker trial next time, try `--model tiny-q5_1` (~32 MB). \
                     Run `aurum models` to list options.",
                    format_approx(info.approx_bytes)
                );
            }
        }
    }

    let options = TranscriptionOptions {
        model: model.clone(),
        language: cfg.language.clone(),
        timestamps: want_timestamps,
    };

    let mut result = match provider_name.as_str() {
        "local" => {
            let provider = LocalWhisperProvider::new(cfg.cache_dir.clone()).with_progress(true);
            if cli.verbose {
                eprintln!(
                    "aurum: provider=local model={model} backend={:?}",
                    BackendKind::Asr
                );
            }
            provider.transcribe(&audio, &options).await?
        }
        "openrouter" => {
            let provider = OpenRouterProvider::new(
                cfg.openrouter_api_key.clone(),
                Some(cfg.openrouter_base_url.clone()),
            )?;
            if cli.verbose {
                eprintln!(
                    "aurum: provider=openrouter model={model} backend={:?}",
                    BackendKind::LlmAssisted
                );
            }
            provider.transcribe(&audio, &options).await?
        }
        other => {
            return Err(UserError::InvalidProvider {
                provider: other.to_string(),
            }
            .into());
        }
    };

    // Config file defaults, then CLI overrides (already merged into cfg).
    let cleanup_style = CleanupStyle::parse(&cfg.cleanup_style)?;
    let cleanup_kind = CleanupProviderKind::parse(&cfg.cleanup_provider)?;

    // Always stamp cleanup metadata (raw when off) for JSON consumers.
    if matches!(cleanup_style, CleanupStyle::Raw) {
        result.cleanup_style = CleanupStyle::Raw;
        result.cleanup_provider = None;
        result.original_text = None;
    } else {
        if cli.verbose || atty_stderr() {
            eprintln!(
                "aurum: cleanup style={} provider={}",
                cleanup_style.as_str(),
                cleanup_kind.as_str()
            );
        }
        match cleanup_kind {
            CleanupProviderKind::Rules => {
                let c = RulesCleanup::new();
                apply_cleanup(&mut result, &c as &dyn TextCleanup, cleanup_style).await?;
            }
            CleanupProviderKind::OpenRouter => {
                let c = OpenRouterCleanup::new(
                    cfg.openrouter_api_key.clone(),
                    Some(cfg.openrouter_base_url.clone()),
                    cfg.cleanup_openrouter_model
                        .clone()
                        .or_else(|| Some(cfg.openrouter_default_model.clone())),
                )?;
                apply_cleanup(&mut result, &c as &dyn TextCleanup, cleanup_style).await?;
            }
        }
    }

    if let Some(path) = &cfg.output_file {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = std::fs::File::create(path)?;
        output::write_result(&result, format, &mut file)?;
        if cli.verbose || atty_stderr() {
            eprintln!("aurum: wrote {}", path.display());
        }
    } else {
        let mut stdout = io::stdout().lock();
        output::write_result(&result, format, &mut stdout)?;
        stdout.flush().ok();
    }

    if result.text.trim().is_empty() && atty_stderr() {
        eprintln!("aurum: note: transcript is empty (silence or no speech detected)");
    }

    Ok(())
}

fn format_approx(n: u64) -> String {
    let mb = n as f64 / (1024.0 * 1024.0);
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

fn init_tracing(verbose: bool) {
    let filter = if verbose {
        "aurum=debug,aurum_core=debug,info"
    } else {
        "aurum=warn,aurum_core=warn,error"
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

fn atty_stderr() -> bool {
    io::stderr().is_terminal()
}

/// Print a library error.
pub fn report_error(err: &TranscriptionError) {
    eprintln!("error: {err}");
    if matches!(err, TranscriptionError::User(UserError::MissingApiKey)) {
        if let Some(path) = Config::default_config_path() {
            eprintln!("  Config file location: {}", path.display());
            eprintln!("  Create it with a [openrouter] api_key, or export OPENROUTER_API_KEY.");
        }
    }
}
