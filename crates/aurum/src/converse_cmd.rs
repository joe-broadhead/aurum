//! `aurum converse` — turn-based half-duplex replay (GitHub #140).
//!
//! File in → STT → canned reply or CLI LLM → TTS → WAV. No microphone.
//! Speech providers come from the same flags/config as `aurum` / `aurum tts`.
//! `--llm-provider` is never inferred from API keys.

use crate::llm::{complete_chat, ChatRequest, LlmProvider};
use aurum_core::config::Config;
use aurum_core::error::{Result, UserError};
use aurum_core::live::{LiveSession, LiveSessionConfig};
use aurum_core::output::CommitMode;
use aurum_core::provider_platform::ProviderId;
use clap::Parser;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

/// `aurum converse` — one user turn from a file, one agent turn to a WAV.
#[derive(Debug, Parser)]
pub struct ConverseCli {
    /// Audio file for the user turn (decoded to 16 kHz mono).
    #[arg(value_name = "AUDIO_FILE")]
    pub audio_file: PathBuf,

    /// STT provider (registry id; default `local`).
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// STT model id (local ggml name or reviewed remote id).
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// STT language (default from config / auto).
    #[arg(long, value_name = "CODE")]
    pub language: Option<String>,

    /// TTS provider (registry id; default `local`).
    #[arg(long = "tts-provider", value_name = "PROVIDER")]
    pub tts_provider: Option<String>,

    /// TTS model id.
    #[arg(long = "tts-model", value_name = "NAME")]
    pub tts_model: Option<String>,

    /// TTS voice id (required for ElevenLabs; never remapped from Luna).
    #[arg(long, value_name = "NAME")]
    pub voice: Option<String>,

    /// Agent reply text (mutually exclusive with `--reply-file` and `--llm-provider`).
    #[arg(long = "reply-text", value_name = "TEXT")]
    pub reply_text: Option<String>,

    /// Read agent reply UTF-8 from this file.
    #[arg(long = "reply-file", value_name = "PATH")]
    pub reply_file: Option<PathBuf>,

    /// Chat backend for the agent turn: `openai` | `openrouter` | `xai`.
    /// Never selected just because a key is set.
    #[arg(long = "llm-provider", value_name = "PROVIDER")]
    pub llm_provider: Option<String>,

    /// Chat model id (defaults: openai=`gpt-4o-mini`, openrouter=`google/gemini-2.5-flash-lite`;
    /// xAI requires this flag).
    #[arg(long = "llm-model", value_name = "NAME")]
    pub llm_model: Option<String>,

    /// Optional system prompt override for `--llm-provider`.
    #[arg(long = "llm-system", value_name = "TEXT")]
    pub llm_system: Option<String>,

    /// Write agent WAV here.
    #[arg(long = "output-file", short = 'O', value_name = "PATH")]
    pub output_file: PathBuf,

    /// Overwrite an existing non-empty output file.
    #[arg(long)]
    pub force: bool,

    /// Reject remote STT/TTS before encode/upload.
    #[arg(long)]
    pub local_only: bool,

    /// Honesty JSON on stdout (no PCM).
    #[arg(long = "emit-json")]
    pub emit_json: bool,

    /// Verbose diagnostics.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

pub async fn run_converse(cli: ConverseCli) -> Result<()> {
    crate::cli::init_tracing(cli.verbose);

    let agent = resolve_agent_source(&cli)?;
    if cli.local_only && matches!(agent, AgentSource::Llm { .. }) {
        return Err(UserError::Other {
            message: "--local-only rejects --llm-provider (chat leaves the machine)".into(),
        }
        .into());
    }
    aurum_core::tts::validate::validate_output_path(&cli.output_file)?;
    let commit_mode = if cli.force {
        CommitMode::Replace
    } else {
        CommitMode::NoClobber
    };
    aurum_core::OutputTransaction::new(&cli.output_file, commit_mode).preflight()?;

    let mut cfg = Config::load()?;
    if let Some(p) = cli.provider.as_deref() {
        cfg.provider = p.to_ascii_lowercase();
    }
    if let Some(m) = cli.model.as_deref() {
        cfg.model = Some(m.to_string());
    }
    if let Some(l) = cli.language.as_deref() {
        cfg.language = l.to_string();
    }
    if cli.local_only {
        cfg.local_only = true;
    }
    if let Some(p) = cli.tts_provider.as_deref() {
        cfg.tts_provider = p.to_ascii_lowercase();
    }
    let tts_id = aurum_core::ProviderId::parse(&cfg.tts_provider)?;
    cfg.tts_model =
        resolve_converse_tts_model(tts_id.as_str(), cli.tts_model.as_deref(), &cfg.tts_model)?;
    if let Some(v) = cli.voice.as_deref() {
        cfg.tts_voice = v.to_string();
    }

    let engine = aurum_core::AurumEngine::from_config(cfg)?;
    let audio = aurum_core::load_audio(&cli.audio_file).await?;
    let live_cfg = LiveSessionConfig {
        max_utterance_secs: audio.duration_secs().max(1.0),
        ..Default::default()
    };

    let mut live = LiveSession::new(engine, live_cfg)?;
    live.push_pcm(audio.samples().as_ref())?;
    live.end_user_turn()?;
    let stt = live.commit_user_turn().await?;
    let (reply, llm_meta) = match agent {
        AgentSource::Canned(text) => (text, None),
        AgentSource::Llm {
            provider,
            model,
            system,
        } => {
            let cfg = live.engine().config();
            let key = cfg
                .provider_secret(&ProviderId::parse(provider.as_str())?)
                .ok_or_else(|| UserError::MissingProviderCredential {
                    provider: provider.as_str().into(),
                })?;
            let base = llm_base_url(cfg, provider);
            let text = complete_chat(ChatRequest {
                provider,
                model: &model,
                api_key: &key,
                base_url: base.as_deref(),
                user_text: stt.text(),
                system: system.as_deref().unwrap_or(""),
            })
            .await?;
            (
                text.clone(),
                Some((provider.as_str().to_string(), model, text)),
            )
        }
    };
    aurum_core::tts::validate::validate_text(&reply)?;
    let tts = live.speak_text(&reply).await?;
    live.end_agent_turn()?;

    aurum_core::write_wav_i16_mono_transaction(
        &cli.output_file,
        &tts.pcm_i16_mono,
        tts.sample_rate_hz,
        commit_mode,
    )?;

    if cli.verbose || io::stderr().is_terminal() {
        eprintln!(
            "aurum converse: stt={} {:?} → tts={} {:?} ({:.1}s)",
            stt.provider(),
            stt.text(),
            tts.provider,
            cli.output_file.display(),
            tts.duration_ms as f64 / 1000.0
        );
        if let Some((p, m, t)) = &llm_meta {
            eprintln!("aurum converse: llm={p}/{m} {t:?}");
        }
    }

    if cli.emit_json {
        let abs = std::fs::canonicalize(&cli.output_file).unwrap_or(cli.output_file.clone());
        let stt_dto = aurum_core::dto::SttResultDto::from_result(&stt);
        let mut tts_dto = serde_json::to_value(aurum_core::dto::TtsMetaDto::from_result(&tts))
            .map_err(|e| UserError::Other {
                message: format!("tts json: {e}"),
            })?;
        if let Some(obj) = tts_dto.as_object_mut() {
            obj.insert(
                "output_path".into(),
                serde_json::json!(abs.display().to_string()),
            );
            obj.insert("format".into(), serde_json::json!("wav"));
        }
        let mut payload = serde_json::json!({
            "stt": stt_dto,
            "tts": tts_dto,
        });
        if let Some((provider, model, text)) = llm_meta {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert(
                    "llm".into(),
                    serde_json::json!({
                        "provider": provider,
                        "model": model,
                        "text": text,
                    }),
                );
            }
        }
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| UserError::Other {
                message: format!("json: {e}"),
            })?
        )?;
    }

    live.shutdown();
    Ok(())
}

#[derive(Debug)]
enum AgentSource {
    Canned(String),
    Llm {
        provider: LlmProvider,
        model: String,
        system: Option<String>,
    },
}

fn resolve_agent_source(cli: &ConverseCli) -> Result<AgentSource> {
    let has_reply = cli.reply_text.is_some() || cli.reply_file.is_some();
    match (cli.llm_provider.as_deref(), has_reply) {
        (Some(_), true) => Err(UserError::Other {
            message: "pass --llm-provider or --reply-text/--reply-file, not both".into(),
        }
        .into()),
        (Some(p), false) => {
            let provider = LlmProvider::parse(p)?;
            let model = match cli
                .llm_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(m) => m.to_string(),
                None => provider.default_model()?.to_string(),
            };
            Ok(AgentSource::Llm {
                provider,
                model,
                system: cli.llm_system.clone(),
            })
        }
        (None, _) => Ok(AgentSource::Canned(read_reply(
            cli.reply_text.as_deref(),
            cli.reply_file.as_deref(),
        )?)),
    }
}

fn llm_base_url(cfg: &Config, provider: LlmProvider) -> Option<String> {
    match provider {
        LlmProvider::OpenAi => cfg.providers.openai.base_url.clone(),
        LlmProvider::Xai => cfg.providers.xai.base_url.clone(),
        LlmProvider::OpenRouter => {
            let u = cfg.openrouter_base_url.trim();
            if u.is_empty() {
                None
            } else {
                Some(u.to_string())
            }
        }
    }
}

fn read_reply(text: Option<&str>, file: Option<&std::path::Path>) -> Result<String> {
    match (text, file) {
        (Some(_), Some(_)) => Err(UserError::Other {
            message: "pass exactly one of --reply-text or --reply-file".into(),
        }
        .into()),
        (None, None) => Err(UserError::Other {
            message: "agent reply required: --reply-text or --reply-file".into(),
        }
        .into()),
        (Some(t), None) => Ok(t.to_string()),
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|e| {
            UserError::Other {
                message: format!("failed to read --reply-file {}: {e}", path.display()),
            }
            .into()
        }),
    }
}

/// Live-only: when ElevenLabs is selected and config still has a local model id,
/// prefer flash over the catalogue default (`eleven_multilingual_v2`).
fn resolve_converse_tts_model(
    provider: &str,
    cli_model: Option<&str>,
    config_model: &str,
) -> Result<String> {
    if cli_model.map(str::trim).filter(|s| !s.is_empty()).is_some() {
        return aurum_core::resolve_tts_model(provider, cli_model, config_model);
    }
    if provider == "elevenlabs"
        && !aurum_core::tts_model_known_for_provider("elevenlabs", config_model)
    {
        return Ok("eleven_flash_v2_5".into());
    }
    aurum_core::resolve_tts_model(provider, None, config_model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn reply_xor() {
        let err = read_reply(None, None).unwrap_err();
        assert!(err.to_string().contains("reply"));
        let err = read_reply(Some("a"), Some(std::path::Path::new("x"))).unwrap_err();
        assert!(err.to_string().contains("exactly one"));
        assert_eq!(read_reply(Some("hi"), None).unwrap(), "hi");
    }

    #[test]
    fn elevenlabs_live_default_is_flash_when_config_is_kitten() {
        let m = resolve_converse_tts_model("elevenlabs", None, "kitten-nano-int8").unwrap();
        assert_eq!(m, "eleven_flash_v2_5");
        let explicit = resolve_converse_tts_model(
            "elevenlabs",
            Some("eleven_multilingual_v2"),
            "kitten-nano-int8",
        )
        .unwrap();
        assert_eq!(explicit, "eleven_multilingual_v2");
    }

    #[test]
    fn clap_parses_converse() {
        let cli = crate::cli::Cli::try_parse_from([
            "aurum",
            "converse",
            "talk.wav",
            "--reply-text",
            "hello",
            "-O",
            "/tmp/out.wav",
            "--provider",
            "openai",
            "--tts-provider",
            "elevenlabs",
            "--voice",
            "21m00Tcm4TlvDq8ikWAM",
        ])
        .unwrap();
        match cli.command {
            Some(crate::cli::Commands::Converse(c)) => {
                assert_eq!(c.provider.as_deref(), Some("openai"));
                assert_eq!(c.tts_provider.as_deref(), Some("elevenlabs"));
                assert_eq!(c.reply_text.as_deref(), Some("hello"));
            }
            other => panic!("expected Converse, got {other:?}"),
        }
    }

    #[test]
    fn llm_xor_reply() {
        let cli = crate::cli::Cli::try_parse_from([
            "aurum",
            "converse",
            "talk.wav",
            "--reply-text",
            "hello",
            "--llm-provider",
            "openai",
            "-O",
            "/tmp/out.wav",
        ])
        .unwrap();
        let Some(crate::cli::Commands::Converse(c)) = cli.command else {
            panic!("expected Converse");
        };
        let err = resolve_agent_source(&c).unwrap_err();
        assert!(err.to_string().contains("not both"), "{err}");
    }

    #[test]
    fn llm_openai_default_model() {
        let cli = crate::cli::Cli::try_parse_from([
            "aurum",
            "converse",
            "talk.wav",
            "--llm-provider",
            "openai",
            "-O",
            "/tmp/out.wav",
        ])
        .unwrap();
        let Some(crate::cli::Commands::Converse(c)) = cli.command else {
            panic!("expected Converse");
        };
        match resolve_agent_source(&c).unwrap() {
            AgentSource::Llm {
                provider, model, ..
            } => {
                assert_eq!(provider, LlmProvider::OpenAi);
                assert_eq!(model, "gpt-4o-mini");
            }
            AgentSource::Canned(_) => panic!("expected llm"),
        }
    }
}
