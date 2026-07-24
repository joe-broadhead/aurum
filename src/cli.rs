//! CLI definition and orchestration.

use crate::audio;
use crate::config::{self, Config, DEFAULT_LOCAL_MODEL, DEFAULT_OPENROUTER_MODEL};
use crate::error::{Result, TranscriptionError, UserError};
use crate::output::{self, OutputFormat};
use crate::providers::{
    LocalWhisperProvider, OpenRouterProvider, TranscriptionOptions, TranscriptionProvider,
};
use clap::Parser;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Aurum — local-first transcription CLI (Latin: gold).
#[derive(Debug, Parser)]
#[command(
    name = "aurum",
    version,
    about = "Local-first, cross-platform transcription CLI (Latin: gold)",
    long_about = "Aurum converts audio files to text using local whisper.cpp models by default, \
                  with optional OpenRouter remote transcription.\n\n\
                  Aurum is Latin for gold."
)]
pub struct Cli {
    /// Audio file to transcribe.
    #[arg(value_name = "AUDIO_FILE")]
    pub audio_file: PathBuf,

    /// Transcription provider.
    #[arg(long, value_name = "local|openrouter", value_parser = ["local", "openrouter"])]
    pub provider: Option<String>,

    /// Local model name (tiny, base, small, …) or OpenRouter model id.
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// Language code (e.g. en, fr) or "auto".
    #[arg(long, value_name = "CODE")]
    pub language: Option<String>,

    /// Output format.
    #[arg(short = 'o', long = "output", value_name = "txt|srt|json", value_parser = ["txt", "srt", "json"])]
    pub output: Option<String>,

    /// Write output to this path instead of stdout (extension does not change format).
    #[arg(long = "output-file", value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Include timestamps where available (implied by srt).
    #[arg(long)]
    pub timestamps: bool,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Entry point used by `main`.
pub async fn run(cli: Cli) -> Result<()> {
    init_tracing(cli.verbose);

    let mut cfg = Config::load()?;
    cfg.apply_cli(
        cli.provider.as_deref(),
        cli.model.as_deref(),
        cli.language.as_deref(),
        cli.output.as_deref(),
        cli.output_file.as_deref(),
        cli.timestamps,
        cli.verbose,
    );

    // If the user selected openrouter but left the default local model name in place
    // (from config defaults) and did not pass --model, prefer the openrouter default.
    let model = resolve_model(&cfg, cli.model.is_some());

    let provider_name = cfg.provider.to_ascii_lowercase();
    let format = OutputFormat::parse(&cfg.output)?;

    // SRT always needs timestamps.
    let want_timestamps = cfg.timestamps || matches!(format, OutputFormat::Srt);

    tracing::info!(
        provider = %provider_name,
        model = %model,
        language = %cfg.language,
        output = %format.as_str(),
        "starting transcription"
    );

    if !cli.audio_file.exists() {
        return Err(UserError::FileNotFound {
            path: cli.audio_file.display().to_string(),
        }
        .into());
    }

    // Load / convert audio.
    if cli.verbose {
        eprintln!("aurum: loading audio {} …", cli.audio_file.display());
    }
    let audio = audio::load_audio(&cli.audio_file).await?;
    if cli.verbose {
        eprintln!(
            "aurum: loaded {:.2}s of audio ({} samples @ {} Hz)",
            audio.duration_secs,
            audio.samples.len(),
            audio.sample_rate
        );
    }

    let options = TranscriptionOptions {
        model: model.clone(),
        language: cfg.language.clone(),
        timestamps: want_timestamps,
    };

    let result = match provider_name.as_str() {
        "local" => {
            let provider = LocalWhisperProvider::new(cfg.cache_dir.clone()).with_progress(true);
            if cli.verbose {
                eprintln!("aurum: provider=local model={model}");
            }
            provider.transcribe(&audio, &options).await?
        }
        "openrouter" => {
            let provider = OpenRouterProvider::new(
                cfg.openrouter_api_key.clone(),
                Some(cfg.openrouter_base_url.clone()),
            )?;
            if cli.verbose {
                eprintln!("aurum: provider=openrouter model={model}");
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

    // Emit output.
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

    Ok(())
}

fn resolve_model(cfg: &Config, model_explicitly_set: bool) -> String {
    if model_explicitly_set {
        return cfg
            .model
            .clone()
            .unwrap_or_else(|| default_for(&cfg.provider));
    }

    // Config file may have set a model under [default]; use it for local.
    // For openrouter, if the configured model looks like a local short name
    // (no '/'), swap to the openrouter default.
    match cfg.provider.as_str() {
        "openrouter" => {
            let m = cfg
                .model
                .clone()
                .unwrap_or_else(|| DEFAULT_OPENROUTER_MODEL.to_string());
            if m.contains('/') {
                m
            } else if m == DEFAULT_LOCAL_MODEL || crate::model::lookup_model(&m).is_ok() {
                // Config still has a local whisper name — use openrouter default.
                cfg.openrouter_default_model.clone()
            } else {
                m
            }
        }
        _ => cfg
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string()),
    }
}

fn default_for(provider: &str) -> String {
    match provider {
        "openrouter" => DEFAULT_OPENROUTER_MODEL.to_string(),
        _ => DEFAULT_LOCAL_MODEL.to_string(),
    }
}

fn init_tracing(verbose: bool) {
    let filter = if verbose {
        "aurum=debug,info"
    } else {
        "aurum=warn,error"
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
    // Avoid extra dependency; crude check via is_terminal.
    use std::io::IsTerminal;
    io::stderr().is_terminal()
}

/// Map a library error to a process exit code and print it.
pub fn report_error(err: &TranscriptionError) {
    eprintln!("error: {err}");
    if let Some(path) = Config::default_config_path() {
        if matches!(err, TranscriptionError::User(UserError::MissingApiKey)) {
            eprintln!("  Config file location: {}", path.display());
            let _ = config::write_example_config(&path);
        }
    }
}

/// Compute default output path next to the input (unused helper for future --output-file auto).
#[allow(dead_code)]
pub fn default_output_path(input: &Path, format: OutputFormat) -> PathBuf {
    let mut out = input.to_path_buf();
    out.set_extension(format.default_extension());
    out
}
