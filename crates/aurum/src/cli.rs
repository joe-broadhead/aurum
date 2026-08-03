//! CLI definition and orchestration.

use aurum_core::audio;
use aurum_core::cleanup::{
    apply_cleanup_with_segments, cleanup_text, CleanupProviderKind, CleanupStyle,
    OpenRouterCleanup, RulesCleanup, SegmentCleanupPolicy, TextCleanup,
};
use aurum_core::config::Config;
use aurum_core::dto::ErrorDto;
use aurum_core::error::{Result, TranscriptionError, UserError};
use aurum_core::model;
use aurum_core::output::{self, commit_text, CommitMode, OutputFormat};
use aurum_core::providers::{OpenRouterSttMode, TranscriptionOptions};
use aurum_core::remote::RemotePolicy;
use clap::{Parser, Subcommand};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// When true, [`report_error`] prints a versioned [`ErrorDto`] JSON envelope on stderr.
static EMIT_JSON_ERRORS: AtomicBool = AtomicBool::new(false);

/// Enable structured JSON errors for machine-readable CLI modes (`-o json`, `--json`, `--emit-json`).
pub fn set_json_errors(on: bool) {
    EMIT_JSON_ERRORS.store(on, Ordering::Relaxed);
}

fn json_errors_enabled() -> bool {
    if EMIT_JSON_ERRORS.load(Ordering::Relaxed) {
        return true;
    }
    // Escape hatch for wrappers that cannot set the process flag before run().
    matches!(
        std::env::var("AURUM_JSON_ERRORS").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn wants_json_errors(cli: &Cli) -> bool {
    if let Some(out) = cli.transcribe.output.as_deref() {
        if out.eq_ignore_ascii_case("json") {
            return true;
        }
    }
    match &cli.command {
        Some(Commands::Transcribe(args)) => args
            .output
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("json")),
        Some(Commands::Cleanup(args)) => args
            .output
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("json")),
        Some(Commands::Tts(tts)) => match &tts.command {
            Some(TtsCommands::Inspect { json, .. })
            | Some(TtsCommands::Verify { json, .. })
            | Some(TtsCommands::Add { json, .. }) => *json,
            None => tts.synth.emit_json,
            _ => false,
        },
        Some(Commands::Batch(batch)) => {
            batch.json
                || batch
                    .output
                    .as_deref()
                    .is_some_and(|o| o.eq_ignore_ascii_case("json"))
        }
        Some(Commands::Cache(cache)) => cache.json,
        Some(Commands::Doctor(doc)) => doc.json,
        Some(Commands::SupportBundle(sb)) => sb.stdout,
        Some(Commands::Models {
            command: Some(ModelsCommands::Recommend { json, .. }),
        }) => *json,
        _ => false,
    }
}

/// Aurum — on-device speech CLI (STT + TTS).
#[derive(Debug, Parser)]
#[command(
    name = "aurum",
    version,
    about = "Speech both ways. On-device by default.",
    long_about = "Aurum is private speech I/O on your machine:\n\
  • STT — audio → text (whisper.cpp local by default; opt-in openrouter|openai|xai)\n\
  • cleanup — post-transcript flow styles (rules or OpenRouter)\n\
  • TTS — text → mono WAV (local ONNX by default; opt-in openrouter|openai|elevenlabs|xai)\n\
  • batch — resumable multi-file transcription\n\n\
 Quick start:\n \
 aurum meeting.m4a\n \
 aurum meeting.m4a --cleanup clean\n \
 aurum tts \"Hello from aurum\" --output-file /tmp/a.wav\n \
 aurum cleanup --style bullets < notes.txt\n \
 aurum batch ./lectures -O ./out\n \
 aurum models\n \
 aurum models recommend --profile balance\n \
 aurum tts voices\n \
 aurum support-bundle\n \
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
    Models {
        #[command(subcommand)]
        command: Option<ModelsCommands>,
    },

    /// Transcribe an audio file (default when AUDIO_FILE is given positionally).
    #[command(visible_alias = "t")]
    Transcribe(TranscribeArgs),

    /// Clean existing text (stdin or file) without re-transcribing.
    #[command(visible_alias = "flow")]
    Cleanup(CleanupArgs),

    /// Synthesize speech from text (local ONNX TTS → mono WAV).
    Tts(TtsCli),

    /// Bounded resumable multi-file transcription (JOE-1726).
    Batch(crate::batch_cmd::BatchCli),

    /// Inspect and verify local model/voice-pack cache (JOE-1592).
    Cache(CacheCli),

    /// Read-only system, config, cache, and capability diagnostics (JOE-1628).
    Doctor(DoctorCli),

    /// Privacy-safe support bundle for issue reports (JOE-1728).
    #[command(name = "support-bundle")]
    SupportBundle(crate::support_cmd::SupportBundleCli),

    /// Generate shell completions (bash/zsh/fish/powershell/elvish).
    Completions(crate::completions_cmd::CompletionsCli),

    /// Print a man page to stdout (JOE-1720).
    Man,
}

/// `aurum models` subcommands.
#[derive(Debug, Subcommand)]
pub enum ModelsCommands {
    /// Explain an intent profile and the model it resolves to (JOE-1723).
    Recommend {
        /// Profile: speed | balance | quality
        #[arg(long, value_name = "PROFILE")]
        profile: String,
        /// Language used for resolution (default: auto / config).
        #[arg(long, value_name = "CODE")]
        language: Option<String>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
}

/// `aurum doctor` — install / config / capability probe (no downloads).
#[derive(Debug, Parser)]
pub struct DoctorCli {
    /// Emit JSON report.
    #[arg(long)]
    pub json: bool,
}

/// `aurum cache` — inventory / verify / repair surface.
#[derive(Debug, Parser)]
pub struct CacheCli {
    #[command(subcommand)]
    pub command: CacheCommands,

    /// Emit JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommands {
    /// Cheap size/existence inventory (no full hashing).
    Status,
    /// Full digest verification for STT pins; quarantines bad artifacts (no network).
    Verify,
    /// Re-download a model through the verified path (requires network unless --local-only).
    Repair {
        /// Model name (e.g. tiny-q5_1).
        model: String,
        /// Fail if the model is missing rather than downloading.
        #[arg(long)]
        local_only: bool,
    },
}

/// `aurum tts` — synthesize or list TTS models/voices.
#[derive(Debug, Parser)]
#[command(args_conflicts_with_subcommands = true)]
pub struct TtsCli {
    #[command(subcommand)]
    pub command: Option<TtsCommands>,

    #[command(flatten)]
    pub synth: TtsArgs,
}

#[derive(Debug, Subcommand)]
pub enum TtsCommands {
    /// List local TTS models and cache status.
    Models,
    /// List local TTS voices and cache status.
    Voices,
    /// List supported TTS adapters (not bare ONNX loaders).
    Adapters,
    /// Read-only inspect of a model pack or manifest (no network).
    Inspect {
        /// Pack directory or `aurum-tts-manifest.json` path.
        path: PathBuf,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Full digest + adapter + conformance preflight for a pack.
    Verify {
        /// Pack directory or manifest path.
        path: PathBuf,
        /// Allow trust=local_unverified packs.
        #[arg(long)]
        allow_unverified: bool,
        /// Emit JSON conformance report.
        #[arg(long)]
        json: bool,
    },
    /// Propose adding a local pack (dry-run by default; never auto-trusts HF).
    Add {
        /// Local pack directory.
        path: PathBuf,
        /// Required known adapter id (see `aurum tts adapters`).
        #[arg(long)]
        adapter: String,
        /// Custom model id to register (cannot collide with built-ins).
        #[arg(long)]
        model_id: String,
        /// Trust mode: verified | local_unverified (never builtin).
        #[arg(long, default_value = "verified")]
        trust: String,
        /// Write the proposed manifest into the pack directory (default is dry-run).
        #[arg(long)]
        write_manifest: bool,
        /// Emit JSON proposal.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct TranscribeArgs {
    /// Audio file to transcribe.
    #[arg(value_name = "AUDIO_FILE")]
    pub audio_file: Option<PathBuf>,

    /// Transcription provider (registry id; default `local`).
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Local model name (tiny-q5_1, base, …) or reviewed remote model id.
    /// Overrides --profile when both are set.
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// Intent profile: speed | balance | quality (JOE-1723). Ignored if --model is set.
    #[arg(long, value_name = "PROFILE")]
    pub profile: Option<String>,

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

    /// Allow SRT/timestamp output when the route does not guarantee reliable timings.
    #[arg(long = "allow-unreliable-timestamps")]
    pub allow_unreliable_timestamps: bool,

    /// OpenRouter STT path: auto | chat | transcriptions (JOE-1586).
    #[arg(long = "openrouter-stt-mode", value_name = "auto|chat|transcriptions")]
    pub openrouter_stt_mode: Option<String>,

    /// Post-transcript cleanup style (default: raw = off).
    #[arg(
        long = "cleanup",
        value_name = "raw|clean|bullets|professional|summary"
    )]
    pub cleanup: Option<String>,

    /// Cleanup backend: on-device rules (default) or OpenRouter LLM.
    #[arg(long = "cleanup-provider", value_name = "rules|openrouter")]
    pub cleanup_provider: Option<String>,

    /// Model id when using --cleanup-provider openrouter.
    #[arg(long = "cleanup-model", value_name = "MODEL")]
    pub cleanup_model: Option<String>,

    /// Segment policy after cleanup: auto | keep | clear | per-segment.
    #[arg(long = "cleanup-segments", value_name = "auto|keep|clear|per-segment")]
    pub cleanup_segments: Option<String>,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Args for `aurum tts` synthesis.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct TtsArgs {
    /// Text to speak. Use `-` to read UTF-8 from stdin.
    #[arg(value_name = "TEXT")]
    pub text: Option<String>,

    /// Read UTF-8 text from this file instead of positional TEXT.
    #[arg(long = "input-file", value_name = "PATH")]
    pub input_file: Option<PathBuf>,

    /// TTS provider (registry id; default `local`).
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// TTS model id (default from config / kitten-nano-int8).
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// Voice id (default from config / Luna).
    #[arg(long, value_name = "NAME")]
    pub voice: Option<String>,

    /// Language code (default: en).
    #[arg(long, value_name = "CODE")]
    pub language: Option<String>,

    /// Output container (WAV only).
    #[arg(short = 'o', long = "output", value_name = "wav", value_parser = ["wav"])]
    pub output: Option<String>,

    /// Write WAV to this path (required for synthesis).
    #[arg(long = "output-file", short = 'O', value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Overwrite an existing non-empty output file.
    #[arg(long)]
    pub force: bool,

    /// Speaking rate multiplier (clamped 0.5..=2.0).
    #[arg(long = "speaking-rate", value_name = "RATE")]
    pub speaking_rate: Option<f32>,

    /// Optional rules-only cleanup style before synth (`clean` min).
    #[arg(long = "cleanup", value_name = "raw|clean")]
    pub cleanup: Option<String>,

    /// Wall-clock timeout in milliseconds.
    #[arg(long = "timeout", value_name = "MS")]
    pub timeout: Option<u64>,

    /// Print honesty JSON metadata on stdout (audio only in file).
    #[arg(long = "emit-json")]
    pub emit_json: bool,

    /// Fail if the voice pack is not already cached (no download).
    #[arg(long = "local-only")]
    pub local_only: bool,

    /// Local model-pack directory (manifest + artifacts). Not a bare .onnx path.
    #[arg(long = "pack-dir", value_name = "DIR")]
    pub pack_dir: Option<PathBuf>,

    /// Allow trust=local_unverified for `--pack-dir` (unsupported for production).
    #[arg(long = "allow-unverified")]
    pub allow_unverified: bool,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Args for `aurum cleanup`.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct CleanupArgs {
    /// Optional text file (default: read stdin).
    #[arg(value_name = "TEXT_FILE")]
    pub input: Option<PathBuf>,

    /// Cleanup style (default: clean for this subcommand, or config).
    #[arg(
        long = "style",
        short = 's',
        value_name = "raw|clean|bullets|professional|summary"
    )]
    pub style: Option<String>,

    /// Cleanup backend: rules (default) or openrouter.
    #[arg(long = "provider", value_name = "rules|openrouter")]
    pub provider: Option<String>,

    /// Model when provider is openrouter.
    #[arg(long = "model", value_name = "MODEL")]
    pub model: Option<String>,

    /// Output format for structured result.
    #[arg(short = 'o', long = "output", value_name = "txt|json", value_parser = ["txt", "json"])]
    pub output: Option<String>,

    #[arg(long = "output-file", value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Entry point used by `main`.
pub async fn run(cli: Cli) -> Result<()> {
    if wants_json_errors(&cli) {
        set_json_errors(true);
    }
    match cli.command {
        Some(Commands::Models { command: None }) => {
            init_tracing(false);
            let cfg = Config::load()?;
            print!("{}", model::format_model_list(&cfg.cache_dir));
            Ok(())
        }
        Some(Commands::Models {
            command:
                Some(ModelsCommands::Recommend {
                    profile,
                    language,
                    json,
                }),
        }) => {
            init_tracing(false);
            let cfg = Config::load()?;
            let lang = language.unwrap_or(cfg.language);
            let p = aurum_core::QualityProfile::parse(&profile)?;
            let res = aurum_core::resolve_profile(p, &lang)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&res).map_err(|e| UserError::Other {
                        message: format!("profile json: {e}"),
                    })?
                );
            } else {
                print!("{}", aurum_core::format_recommendation(&res));
            }
            Ok(())
        }
        Some(Commands::Transcribe(args)) => run_transcribe(args).await,
        Some(Commands::Cleanup(args)) => run_cleanup_cmd(args).await,
        Some(Commands::Tts(tts)) => run_tts_cli(tts).await,
        Some(Commands::Batch(batch)) => crate::batch_cmd::run_batch(batch).await,
        Some(Commands::Cache(cache)) => run_cache_cmd(cache).await,
        Some(Commands::Doctor(doc)) => run_doctor_cmd(doc),
        Some(Commands::SupportBundle(sb)) => crate::support_cmd::run_support_bundle(sb),
        Some(Commands::Completions(c)) => crate::completions_cmd::run_completions(c)
            .map_err(|m| UserError::Other { message: m }.into()),
        Some(Commands::Man) => {
            crate::completions_cmd::run_man().map_err(|m| UserError::Other { message: m }.into())
        }
        None => {
            if cli.transcribe.audio_file.is_none() {
                eprintln!(
                    "aurum: missing AUDIO_FILE\n\n \
 Examples:\n \
 aurum meeting.m4a\n \
 aurum meeting.m4a --cleanup clean\n \
 aurum tts \"Hello from aurum\" --output-file /tmp/a.wav\n \
 echo 'um hello' | aurum cleanup --style clean\n \
 aurum batch ./lectures -O ./out\n \
 aurum models\n\n \
 Run `aurum --help` for full options."
                );
                return Err(UserError::Other {
                    message: "AUDIO_FILE is required (or use `aurum models` / `aurum cleanup` / `aurum tts` / `aurum batch`)"
                        .into(),
                }
                .into());
            }
            run_transcribe(cli.transcribe).await
        }
    }
}

async fn run_tts_cli(cli: TtsCli) -> Result<()> {
    match cli.command {
        Some(TtsCommands::Models) => {
            init_tracing(false);
            let cfg = Config::load()?;
            print!("{}", aurum_core::format_tts_model_list(&cfg.cache_dir));
            if !cfg.tts_custom_models.is_empty() {
                let entries: Vec<_> = cfg
                    .tts_custom_models
                    .iter()
                    .map(|e| aurum_core::tts::CustomTtsModelEntry {
                        id: e.id.clone(),
                        adapter: e.adapter.clone(),
                        pack_dir: e.pack_dir.clone(),
                        trust: e.trust.clone(),
                        license: e.license.clone(),
                        notes: e.notes.clone(),
                    })
                    .collect();
                // Best-effort: list only entries whose packs validate.
                if let Ok(models) = aurum_core::tts::validate_custom_models(&entries) {
                    print!("{}", aurum_core::format_tts_custom_list(&models));
                } else {
                    println!(
                        "\n(Custom models configured but failed validation — run `aurum tts verify`.)"
                    );
                }
            }
            Ok(())
        }
        Some(TtsCommands::Voices) => {
            init_tracing(false);
            let cfg = Config::load()?;
            print!("{}", aurum_core::format_tts_voice_list(&cfg.cache_dir));
            Ok(())
        }
        Some(TtsCommands::Adapters) => {
            init_tracing(false);
            print!("{}", aurum_core::format_tts_adapters());
            Ok(())
        }
        Some(TtsCommands::Inspect { path, json }) => {
            init_tracing(false);
            let report = aurum_core::inspect_tts_pack(&path);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| {
                        UserError::Other {
                            message: format!("json: {e}"),
                        }
                    })?
                );
            } else {
                print!("{}", aurum_core::format_tts_inspect(&report));
            }
            if !report.ok {
                std::process::exit(2);
            }
            Ok(())
        }
        Some(TtsCommands::Verify {
            path,
            allow_unverified,
            json,
        }) => {
            init_tracing(false);
            let report = aurum_core::verify_tts_pack(&path, allow_unverified)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| {
                        UserError::Other {
                            message: format!("json: {e}"),
                        }
                    })?
                );
            } else {
                println!(
                    "adapter={} model={} ok={}",
                    report.adapter_id,
                    report.model_id,
                    report.ok()
                );
                if !report.passed.is_empty() {
                    println!("passed: {}", report.passed.join(", "));
                }
                if !report.failed.is_empty() {
                    println!("failed:");
                    for f in &report.failed {
                        println!("  - {}: {}", f.check, f.detail);
                    }
                }
                if !report.skipped.is_empty() {
                    println!("skipped: {}", report.skipped.join(", "));
                }
            }
            if !report.ok() {
                std::process::exit(2);
            }
            Ok(())
        }
        Some(TtsCommands::Add {
            path,
            adapter,
            model_id,
            trust,
            write_manifest,
            json,
        }) => {
            init_tracing(false);
            let trust_mode = aurum_core::TrustMode::parse(&trust)?;
            let mut proposal =
                aurum_core::propose_tts_add_local(&path, &adapter, &model_id, trust_mode)?;
            proposal.dry_run = !write_manifest;
            if write_manifest {
                let written = aurum_core::write_tts_add_manifest(&path, &proposal)?;
                proposal.dry_run = false;
                proposal
                    .next_steps
                    .insert(0, format!("Wrote manifest: {}", written.display()));
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&proposal).map_err(|e| {
                        UserError::Other {
                            message: format!("json: {e}"),
                        }
                    })?
                );
            } else {
                println!("adapter: {}", proposal.adapter_id);
                println!("model_id: {}", proposal.model_id);
                println!("trust: {}", proposal.trust);
                println!("dry_run: {}", proposal.dry_run);
                for w in &proposal.warnings {
                    println!("warning: {w}");
                }
                println!("\nProposed [[tts.custom_models]] snippet:");
                println!("[[tts.custom_models]]");
                println!("id = {:?}", proposal.model_id);
                println!("adapter = {:?}", proposal.adapter_id);
                println!("pack_dir = {:?}", path.display().to_string());
                println!("trust = {:?}", proposal.trust);
                println!("license = {:?}", proposal.manifest.license);
                for s in &proposal.next_steps {
                    println!("next: {s}");
                }
                if proposal.dry_run {
                    println!(
                        "\nNo files changed (dry-run). Pass --write-manifest to materialize the pack manifest."
                    );
                }
            }
            Ok(())
        }
        None => run_tts_synth(cli.synth).await,
    }
}

async fn run_tts_synth(cli: TtsArgs) -> Result<()> {
    init_tracing(cli.verbose);
    let cfg = Config::load()?;

    let provider_name = cli
        .provider
        .as_deref()
        .unwrap_or(cfg.tts_provider.as_str())
        .to_ascii_lowercase();
    let provider_id = aurum_core::ProviderId::parse(&provider_name)?;

    if let Some(fmt) = cli.output.as_deref() {
        if fmt != "wav" {
            return Err(UserError::Other {
                message: format!(
                    "unsupported TTS output format '{fmt}'\n  Hint: only `wav` is supported in this release."
                ),
            }
            .into());
        }
    }

    let output_file = cli.output_file.clone().ok_or_else(|| UserError::Other {
        message:
            "--output-file / -O is required for `aurum tts` synthesis\n  Hint: aurum tts \"Hello\" --output-file /tmp/a.wav"
                .into(),
    })?;

    aurum_core::tts::validate::validate_output_path(&output_file)?;
    let commit_mode = if cli.force {
        CommitMode::Replace
    } else {
        CommitMode::NoClobber
    };
    // Preflight no-clobber / symlink policy before expensive synthesis.
    aurum_core::OutputTransaction::new(&output_file, commit_mode).preflight()?;

    // Exactly one text source: positional TEXT | --input-file | (TEXT=- reads stdin).
    // Cap the read before full string load so a huge file/stdin cannot OOM us.
    let text = read_tts_text(
        cli.text.as_deref(),
        cli.input_file.as_deref(),
        cfg.tts_max_chars,
    )
    .await?;

    let mut body = text;
    if let Some(style_raw) = cli.cleanup.as_deref() {
        let style = CleanupStyle::parse(style_raw)?;
        match style {
            CleanupStyle::Raw => {}
            CleanupStyle::Clean => {
                let cleaned =
                    cleanup_text(&body, &RulesCleanup::new(), CleanupStyle::Clean).await?;
                body = cleaned.text;
            }
            other => {
                return Err(UserError::Other {
                    message: format!(
                        "TTS --cleanup only supports raw|clean (got '{}')",
                        other.as_str()
                    ),
                }
                .into());
            }
        }
    }

    // Provider-aware defaults: remote providers never inherit local kitten/Luna
    // when the operator only sets --provider openrouter|openai|… (live smoke).
    let model =
        aurum_core::resolve_tts_model(provider_id.as_str(), cli.model.as_deref(), &cfg.tts_model)?;
    let voice = {
        let requested = aurum_core::resolve_tts_voice(
            provider_id.as_str(),
            cli.voice.as_deref(),
            &cfg.tts_voice,
        )?;
        // Opt-in local Kokoro: if the selected model is Kokoro and the requested voice
        // is not model-scoped (e.g. default "Luna"), use Kokoro's default voice.
        if model.eq_ignore_ascii_case(aurum_core::KOKORO_TTS_MODEL)
            && aurum_core::resolve_voice_for_model(&model, &requested).is_err()
        {
            aurum_core::KOKORO_DEFAULT_VOICE.to_string()
        } else {
            requested
        }
    };
    let language = cli
        .language
        .clone()
        .unwrap_or_else(|| cfg.tts_language.clone());
    let speaking_rate = cli.speaking_rate.unwrap_or(1.0);
    let timeout_ms = cli.timeout.unwrap_or(cfg.tts_timeout_ms);

    if cli.verbose || atty_stderr() {
        eprintln!(
            "aurum: tts provider={} model={model} voice={voice} …",
            provider_id.as_str()
        );
    }

    // Engine-owned TTS pool + registry resolution (JOE-1795 / JOE-1938).
    let engine = aurum_core::AurumEngine::from_config(cfg.clone())?;
    aurum_core::preflight_tts_with_registry(
        engine.registry(),
        &provider_id,
        &model,
        &language,
        cli.local_only || cfg.local_only,
    )?;
    let provider = engine.tts_provider_with(
        &provider_id,
        aurum_core::ProviderResolveOptions {
            show_progress: true,
            local_only: Some(cli.local_only || cfg.local_only),
            ..Default::default()
        },
    )?;

    let pack_dir = cli.pack_dir.clone().or_else(|| cfg.tts_pack_dir.clone());
    // Custom model id → pack_dir resolution (JOE-1620). Case-insensitive match.
    let custom_entry = cfg
        .tts_custom_models
        .iter()
        .find(|m| m.id.eq_ignore_ascii_case(&model));
    let from_custom = pack_dir.is_none() && custom_entry.is_some();
    let pack_dir = if pack_dir.is_none() {
        custom_entry.and_then(|m| m.pack_dir.as_ref().map(PathBuf::from))
    } else {
        pack_dir
    };
    // Entry trust=local_unverified is itself the opt-in (still marked unsupported).
    let entry_unverified = custom_entry
        .map(|m| {
            m.trust.eq_ignore_ascii_case("local_unverified")
                || m.trust.eq_ignore_ascii_case("unverified")
                || m.trust.eq_ignore_ascii_case("local-unverified")
        })
        .unwrap_or(false);
    let allow_unverified = cli.allow_unverified || cfg.tts_allow_unverified || entry_unverified;

    let opts = aurum_core::SynthesisOptions {
        model,
        voice,
        language,
        sample_rate_hz: None,
        speaking_rate,
        timeout_ms,
        cancel: None,
        op: None,
        local_only: cli.local_only || pack_dir.is_some(),
        pack_dir,
        allow_unverified,
    };
    let custom_provenance = from_custom;

    let result = provider.synthesize(&body, &opts).await?;

    // Re-check immediately before replace so a long synth cannot silently
    // clobber a file that appeared (or was filled) after the initial check.
    aurum_core::write_wav_i16_mono_transaction(
        &output_file,
        &result.pcm_i16_mono,
        result.sample_rate_hz,
        commit_mode,
    )?;

    if cli.verbose || atty_stderr() {
        eprintln!(
            "aurum: wrote {} ({:.1}s, {} Hz mono, {} chunk{})",
            output_file.display(),
            result.duration_ms as f64 / 1000.0,
            result.sample_rate_hz,
            result.chunk_count,
            if result.chunk_count == 1 { "" } else { "s" }
        );
    }

    if cli.emit_json {
        let abs = std::fs::canonicalize(&output_file).unwrap_or(output_file.clone());
        // Freeze surface: versioned TtsMetaDto (JOE-1896) plus CLI path fields.
        let mut meta = aurum_core::dto::TtsMetaDto::from_result(&result);
        if custom_provenance {
            meta.provenance = Some("custom".to_string());
        }
        let mut payload = serde_json::to_value(&meta)
            .map_err(|e| TranscriptionError::internal(format!("json: {e}")))?;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert(
                "output_path".into(),
                serde_json::json!(abs.display().to_string()),
            );
            obj.insert("format".into(), serde_json::json!("wav"));
        }
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|e| TranscriptionError::internal(format!("json: {e}")))?
        )?;
    }

    Ok(())
}

async fn read_tts_text(
    positional: Option<&str>,
    input_file: Option<&std::path::Path>,
    max_chars: usize,
) -> Result<String> {
    let budget = aurum_core::tts::tts_input_byte_budget(max_chars);
    match (positional, input_file) {
        (Some(_), Some(_)) => Err(UserError::Other {
            message:
                "provide exactly one text source: positional TEXT or --input-file (not both)"
                    .into(),
        }
        .into()),
        (None, None) => Err(UserError::Other {
            message:
                "missing text\n  Hint: aurum tts \"Hello\" --output-file out.wav\n        aurum tts --input-file prompt.txt -O out.wav\n        echo hi | aurum tts - --output-file out.wav"
                    .into(),
        }
        .into()),
        (Some("-"), None) => {
            use std::io::Read;
            let mut limited = io::stdin().take(budget as u64 + 1);
            let mut buf = Vec::new();
            limited.read_to_end(&mut buf)?;
            enforce_tts_read_budget(buf.len(), budget, "stdin")?;
            String::from_utf8(buf).map_err(|e| {
                UserError::Other {
                    message: format!("TTS stdin is not valid UTF-8: {e}"),
                }
                .into()
            })
        }
        (Some(t), None) => {
            enforce_tts_read_budget(t.len(), budget, "positional TEXT")?;
            Ok(t.to_string())
        }
        (None, Some(path)) => {
            if !path.exists() {
                return Err(UserError::FileNotFound {
                    path: path.display().to_string(),
                }
                .into());
            }
            // Cheap metadata pre-check when available (regular files).
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.is_file() && meta.len() > budget as u64 {
                    return Err(tts_input_too_large(path.display().to_string(), budget).into());
                }
            }
            use std::io::Read;
            let file = std::fs::File::open(path)?;
            let mut limited = file.take(budget as u64 + 1);
            let mut buf = Vec::new();
            limited.read_to_end(&mut buf)?;
            enforce_tts_read_budget(buf.len(), budget, &path.display().to_string())?;
            String::from_utf8(buf).map_err(|e| {
                UserError::Other {
                    message: format!(
                        "TTS input file '{}' is not valid UTF-8: {e}",
                        path.display()
                    ),
                }
                .into()
            })
        }
    }
}

fn enforce_tts_read_budget(len: usize, budget: usize, source: &str) -> Result<()> {
    if len > budget {
        return Err(tts_input_too_large(source.to_string(), budget).into());
    }
    Ok(())
}

fn tts_input_too_large(source: String, budget: usize) -> UserError {
    UserError::Other {
        message: format!(
            "TTS input from {source} exceeds the read budget ({budget} bytes)\n  \
             Hint: shorten the text, or raise [tts].max_chars in config if truncation is acceptable."
        ),
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
    // Fail closed after CLI merge (JOE-1779 / ValidatedConfig boundary).
    let cfg = aurum_core::ValidatedConfig::try_from_config(cfg)?.into_config();
    let mut cfg = cfg;

    // Explicit --model wins; else optional --profile; else default resolution.
    let model = if cli.model.is_some() {
        cfg.resolve_model(true)?
    } else if let Some(ref profile) = cli.profile {
        let p = aurum_core::QualityProfile::parse(profile)?;
        let res = aurum_core::resolve_profile(p, &cfg.language)?;
        if cli.verbose || atty_stderr() {
            eprintln!(
                "aurum: profile {} → model {} (evidence {})",
                res.profile, res.model, res.evidence_version
            );
        }
        cfg.model = Some(res.model.clone());
        res.model
    } else {
        cfg.resolve_model(false)?
    };
    let provider_name = cfg.provider.to_ascii_lowercase();
    let provider_id = aurum_core::ProviderId::parse(&provider_name)?;
    let format = OutputFormat::parse(&cfg.output)?;

    // SRT always needs timestamps.
    let want_timestamps = cfg.timestamps || matches!(format, OutputFormat::Srt);

    let stt_mode_raw = cli
        .openrouter_stt_mode
        .as_deref()
        .unwrap_or(cfg.openrouter_stt_mode.as_str());
    let stt_mode = OpenRouterSttMode::parse(stt_mode_raw)?;

    // Chat/LLM path timestamps are unreliable; dedicated ASR may be reliable.
    // We still warn before the request; final SRT gate uses result.timestamps_reliable().

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

    // Registry resolution + preflight before decode/network (JOE-1938).
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
            show_progress: true,
            stt_mode: Some(stt_mode),
            local_only: Some(cfg.local_only),
        },
    )?;

    if cli.verbose || atty_stderr() {
        eprintln!("aurum: loading audio …");
    }
    let audio = audio::load_audio(&audio_file).await?;
    if cli.verbose {
        eprintln!(
            "aurum: loaded {:.2}s of audio ({} samples @ {} Hz)",
            audio.duration_secs(),
            audio.samples().len(),
            audio.sample_rate()
        );
    }

    if atty_stderr() {
        eprintln!(
            "aurum: transcribing with {provider_name}/{model} ({:.1}s audio) …",
            audio.duration_secs()
        );
    }

    // First-run tip when local model is not cached yet.
    if provider_id.as_str() == "local" {
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
        cancel: None,
        op: None,
    };

    if cli.verbose {
        eprintln!(
            "aurum: provider={} model={model} (engine registry)",
            provider_id.as_str()
        );
    }
    let mut result = provider.transcribe(&audio, &options).await?;
    engine.shutdown();

    // SRT requires reliable timings unless explicitly overridden.
    if matches!(format, OutputFormat::Srt)
        && (!result.timestamps_reliable() || result.has_approximate_timestamps())
        && !cli.allow_unreliable_timestamps
    {
        return Err(UserError::Other {
            message: format!(
                "Selected path did not produce reliable media timestamps \
                 (backend={:?}, timestamps_reliable=false).\n  \
                 Hint: use `-o txt` or `-o json`, pass `--allow-unreliable-timestamps`, \
                 or use `--openrouter-stt-mode transcriptions` with a dedicated ASR model.",
                result.backend_kind()
            ),
        }
        .into());
    }

    let cleanup_style = CleanupStyle::parse(&cfg.cleanup_style)?;
    let cleanup_kind = CleanupProviderKind::parse(&cfg.cleanup_provider)?;
    let segment_policy = cli
        .cleanup_segments
        .as_deref()
        .map(SegmentCleanupPolicy::parse)
        .transpose()?
        .unwrap_or(SegmentCleanupPolicy::Auto);

    apply_configured_cleanup(
        &mut result,
        &cfg,
        cleanup_style,
        cleanup_kind,
        segment_policy,
        cli.verbose,
    )
    .await?;

    if let Some(path) = &cfg.output_file {
        // STT path replaces by default (historical CLI behavior for -o files).
        output::write_result_to_path(&result, format, path, CommitMode::Replace)?;
        if cli.verbose || atty_stderr() {
            eprintln!("aurum: wrote {}", path.display());
        }
    } else {
        let mut stdout = io::stdout().lock();
        output::write_result(&result, format, &mut stdout)?;
        stdout.flush().ok();
    }

    if result.text().trim().is_empty() && atty_stderr() {
        eprintln!("aurum: note: transcript is empty (silence or no speech detected)");
    }

    Ok(())
}

async fn run_cleanup_cmd(cli: CleanupArgs) -> Result<()> {
    init_tracing(cli.verbose);
    let mut cfg = Config::load()?;

    // Style defaults to `clean` for this subcommand when config is still `raw` —
    // operators expect `aurum cleanup` to do something without extra flags.
    let style_raw = cli
        .style
        .as_deref()
        .unwrap_or(if cfg.cleanup_style == "raw" {
            "clean"
        } else {
            cfg.cleanup_style.as_str()
        });
    let provider_raw = cli
        .provider
        .as_deref()
        .unwrap_or(cfg.cleanup_provider.as_str());
    let model = cli
        .model
        .as_deref()
        .map(|s| s.to_string())
        .or_else(|| cfg.cleanup_openrouter_model.clone());

    if let Some(m) = model {
        cfg.cleanup_openrouter_model = Some(m);
    }

    let style = CleanupStyle::parse(style_raw)?;
    let kind = CleanupProviderKind::parse(provider_raw)?;

    let text = read_cleanup_input(cli.input.as_deref()).await?;
    if text.trim().is_empty() {
        return Err(UserError::Other {
            message: "no input text (empty file or stdin)".into(),
        }
        .into());
    }

    if cli.verbose || atty_stderr() {
        eprintln!(
            "aurum: cleanup-only style={} provider={}",
            style.as_str(),
            kind.as_str()
        );
    }

    let backend = build_cleanup_backend(&cfg, kind)?;
    let out = cleanup_text(&text, backend.as_ref(), style).await?;

    let format = match cli.output.as_deref().unwrap_or("txt") {
        "json" => "json",
        _ => "txt",
    };

    let body = if format == "json" {
        serde_json::to_string_pretty(&serde_json::json!({
        "text": out.text,
        "cleanup_style": out.style,
        "cleanup_provider": out.provider,
        "original_text": out.original_text,
        }))
        .map_err(|e| TranscriptionError::internal(format!("json: {e}")))?
    } else {
        out.text.clone()
    };

    if let Some(path) = &cli.output_file {
        commit_text(path, &body, CommitMode::Replace)?;
        if cli.verbose || atty_stderr() {
            eprintln!("aurum: wrote {}", path.display());
        }
    } else {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{body}")?;
    }
    Ok(())
}

async fn read_cleanup_input(path: Option<&std::path::Path>) -> Result<String> {
    match path {
        Some(p) => {
            if !p.exists() {
                return Err(UserError::FileNotFound {
                    path: p.display().to_string(),
                }
                .into());
            }
            Ok(std::fs::read_to_string(p)?)
        }
        None => {
            use std::io::Read;
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

fn build_cleanup_backend(cfg: &Config, kind: CleanupProviderKind) -> Result<Box<dyn TextCleanup>> {
    match kind {
        CleanupProviderKind::Rules => Ok(Box::new(RulesCleanup::new())),
        CleanupProviderKind::OpenRouter => {
            let policy = RemotePolicy {
                allow_custom_credentialed_endpoint: cfg.openrouter_allow_custom_endpoint,
                use_system_proxy: cfg.openrouter_use_system_proxy,
                allow_loopback_http: cfg.openrouter_base_url.contains("127.0.0.1")
                    || cfg.openrouter_base_url.contains("localhost"),
                ..Default::default()
            };
            Ok(Box::new(OpenRouterCleanup::with_policy(
                cfg.openrouter_api_key.clone(),
                Some(cfg.openrouter_base_url.clone()),
                cfg.cleanup_openrouter_model
                    .clone()
                    .or_else(|| Some(cfg.openrouter_default_model.clone())),
                policy,
            )?))
        }
    }
}

async fn apply_configured_cleanup(
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
    if verbose || atty_stderr() {
        eprintln!(
            "aurum: cleanup style={} provider={} segments={}",
            style.as_str(),
            kind.as_str(),
            segments.resolve(style).as_str()
        );
    }
    let backend = build_cleanup_backend(cfg, kind)?;
    let (_out, report) =
        apply_cleanup_with_segments(result, backend.as_ref(), style, segments).await?;
    if verbose {
        for w in &report.warnings {
            eprintln!("aurum: cleanup note: {w}");
        }
    }
    Ok(())
}

fn run_doctor_cmd(cli: DoctorCli) -> Result<()> {
    init_tracing(false);
    // Prefer owned engine path (JOE-1782); residual process caches unchanged.
    let engine = aurum_core::AurumEngine::load()?;
    let report = engine.doctor();
    if cli.json {
        println!("{}", report.to_json_pretty()?);
    } else {
        print!("{}", report.format_human());
    }
    if !report.ok {
        // Soft failures (warn) still exit 0 when overall ok is true;
        // error severity makes ok false.
        std::process::exit(2);
    }
    Ok(())
}

async fn run_cache_cmd(cli: CacheCli) -> Result<()> {
    let cfg = Config::load()?;
    match cli.command {
        CacheCommands::Status => {
            let entries = aurum_core::cache::status_stt(&cfg.cache_dir);
            if cli.json {
                println!("{}", aurum_core::cache::status_json(&entries)?);
            } else {
                print!("{}", aurum_core::cache::format_status(&entries));
            }
        }
        CacheCommands::Verify => {
            let entries = aurum_core::cache::verify_stt(&cfg.cache_dir);
            if cli.json {
                println!("{}", aurum_core::cache::status_json(&entries)?);
            } else {
                print!("{}", aurum_core::cache::format_status(&entries));
            }
        }
        CacheCommands::Repair { model, local_only } => {
            use aurum_core::model::{ensure_model_with_options, EnsureModelOptions};
            let path = ensure_model_with_options(
                &cfg.cache_dir,
                &model,
                EnsureModelOptions::new()
                    .local_only(local_only)
                    .show_progress(true),
            )
            .await?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "model": model,
                        "path": path.display().to_string(),
                        "repaired": true,
                        "local_only": local_only,
                    })
                );
            } else {
                println!("aurum: repaired model `{model}` → {}", path.display());
            }
        }
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
///
/// When JSON output was requested (`-o json`, `--json`, `--emit-json`, or
/// `AURUM_JSON_ERRORS=1`), emits a versioned [`ErrorDto`] JSON envelope on
/// stderr so machine consumers share the same schema as library embeds.
/// Otherwise prints a human line plus optional MissingApiKey hints.
pub fn report_error(err: &TranscriptionError) {
    if json_errors_enabled() {
        let dto = ErrorDto::from_error(err);
        match dto.to_json_pretty() {
            Ok(json) => eprintln!("{json}"),
            Err(_) => {
                // Last resort: never fail closed on the error path itself.
                eprintln!(
                    "{{\"schema_version\":{},\"category\":{},\"retryable\":{},\"message\":{},\"exit_code\":{}}}",
                    dto.schema_version,
                    serde_json::to_string(&dto.category).unwrap_or_else(|_| "\"error\"".into()),
                    dto.retryable,
                    serde_json::to_string(&dto.message).unwrap_or_else(|_| "\"error\"".into()),
                    dto.exit_code,
                );
            }
        }
        return;
    }
    eprintln!("error: {err}");
    if matches!(err, TranscriptionError::User(UserError::MissingApiKey)) {
        if let Some(path) = Config::default_config_path() {
            eprintln!(" Config file location: {}", path.display());
            eprintln!(
                " Create it with a [providers.openrouter] api_key, or export OPENROUTER_API_KEY."
            );
        }
    }
}
