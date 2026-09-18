//! `aurum converse` — turn-based half-duplex replay (GitHub #140).
//!
//! File in → STT → canned reply or CLI LLM → TTS → WAV. No microphone.
//! Speech providers come from the same flags/config as `aurum` / `aurum tts`.
//! `--llm-provider` is never inferred from API keys.

use crate::audio_io::{play_i16_mono, resample_mono, MicCapture};
use crate::llm::{complete_chat, stream_chat, take_speakable, ChatRequest, LlmProvider};
use crate::stdio_proto::{self, InCmd, OutEvent, MAX_LINE_BYTES};
use aurum_core::audio::WHISPER_SAMPLE_RATE;
use aurum_core::config::Config;
use aurum_core::error::{Result, UserError};
use aurum_core::live::LiveEvent;
use aurum_core::live::{LivePhase, LiveSession, LiveSessionConfig};
use aurum_core::output::CommitMode;
use aurum_core::provider_platform::ProviderId;
use clap::Parser;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// `aurum converse` — one user turn from a file, or `--mic` for a live loop.
#[derive(Debug, Parser)]
pub struct ConverseCli {
    /// Audio file for a single user turn (omit when using `--mic`).
    #[arg(value_name = "AUDIO_FILE")]
    pub audio_file: Option<PathBuf>,

    /// Use the default microphone and speakers (half-duplex). Experimental.
    #[arg(long)]
    pub mic: bool,

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

    /// Write agent WAV here (required for file mode; optional with `--mic`).
    #[arg(long = "output-file", short = 'O', value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Overwrite an existing non-empty output file.
    #[arg(long)]
    pub force: bool,

    /// Reject remote STT/TTS before encode/upload.
    #[arg(long)]
    pub local_only: bool,

    /// Honesty JSON on stdout (no PCM). Incompatible with `--stdio`.
    #[arg(long = "emit-json")]
    pub emit_json: bool,

    /// JSONL sidecar for harnesses (pi, OpenCode, …). Stdout = events, stdin =
    /// commands. No in-process LLM. Logs stay on stderr. Requires `--mic` or a file.
    #[arg(long)]
    pub stdio: bool,

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
    match (cli.mic, cli.audio_file.is_some()) {
        (true, true) => {
            return Err(UserError::Other {
                message: "pass a file or --mic, not both".into(),
            }
            .into());
        }
        (false, false) => {
            return Err(UserError::Other {
                message: "pass AUDIO_FILE or --mic".into(),
            }
            .into());
        }
        _ => {}
    }

    let mut cfg = Config::load()?;
    if let Some(p) = cli.provider.as_deref() {
        cfg.provider = p.to_ascii_lowercase();
    }
    if let Some(m) = cli.model.as_deref() {
        cfg.model = Some(m.to_string());
    } else if cli.mic && cfg.provider == "openai" {
        // Faster than whisper-1 for turn-taking (reviewed OpenAI STT id).
        cfg.model = Some("gpt-4o-mini-transcribe".into());
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
    if cli.stdio {
        return run_stdio_loop(cli, engine).await;
    }
    if cli.mic {
        return run_mic_loop(cli, engine, agent).await;
    }
    let audio_file = cli.audio_file.as_ref().ok_or_else(|| UserError::Other {
        message: "pass AUDIO_FILE or --mic".into(),
    })?;
    let output_file = cli.output_file.as_ref().ok_or_else(|| UserError::Other {
        message: "-O/--output-file is required unless --mic".into(),
    })?;
    aurum_core::tts::validate::validate_output_path(output_file)?;
    let commit_mode = if cli.force {
        CommitMode::Replace
    } else {
        CommitMode::NoClobber
    };
    aurum_core::OutputTransaction::new(output_file, commit_mode).preflight()?;
    let audio = aurum_core::load_audio(audio_file).await?;
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
                prior: &[],
            })
            .await?;
            (
                text.clone(),
                Some((provider.as_str().to_string(), model, text)),
            )
        }
        AgentSource::Harness => {
            return Err(UserError::Other {
                message: "--stdio does not use file+LLM path".into(),
            }
            .into());
        }
    };
    aurum_core::tts::validate::validate_text(&reply)?;
    let tts = live.speak_text(&reply).await?;
    live.end_agent_turn()?;

    aurum_core::write_wav_i16_mono_transaction(
        output_file,
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
            output_file.display(),
            tts.duration_ms as f64 / 1000.0
        );
        if let Some((p, m, t)) = &llm_meta {
            eprintln!("aurum converse: llm={p}/{m} {t:?}");
        }
    }

    if cli.emit_json {
        let abs = std::fs::canonicalize(output_file).unwrap_or_else(|_| output_file.clone());
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

async fn run_mic_loop(
    cli: ConverseCli,
    engine: aurum_core::AurumEngine,
    agent: AgentSource,
) -> Result<()> {
    eprintln!("aurum converse --mic: headphones recommended (half-duplex, no echo cancel)");
    eprintln!("Speak, pause briefly, reply starts on the first sentence. Ctrl+C to stop.");
    let mic = MicCapture::start()?;
    let live_cfg = LiveSessionConfig {
        max_utterance_secs: 30.0,
        min_speech_secs: 0.25,
        trailing_silence_secs: 0.45,
        min_rms: 0.02,
        max_speak_ahead: 8,
    };
    let mut live = LiveSession::new(engine, live_cfg)?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            stop.store(true, Ordering::SeqCst);
        });
    }
    let mut prior: Vec<(String, String)> = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        let chunk = tokio::task::block_in_place(|| mic.recv_timeout(Duration::from_millis(150)));
        let Some(chunk) = chunk else {
            continue;
        };
        if live.phase() == LivePhase::Speaking || live.phase() == LivePhase::Thinking {
            continue;
        }
        let pcm16 = resample_mono(&chunk, mic.sample_rate, WHISPER_SAMPLE_RATE);
        if pcm16.is_empty() {
            continue;
        }
        live.push_pcm(&pcm16)?;
        if live.phase() != LivePhase::Ending {
            continue;
        }
        let stt = match live.commit_user_turn().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("aurum: STT failed ({e}); listening");
                live.discard_turn();
                mic.drain();
                continue;
            }
        };
        if stt.text().trim().is_empty() {
            live.discard_turn();
            continue;
        }
        eprintln!("you: {}", stt.text());
        mic.drain();
        let reply = match speak_agent_turn(
            &mut live,
            &agent,
            stt.text(),
            &prior,
            cli.output_file.as_deref(),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("aurum: agent failed ({e}); listening");
                live.discard_turn();
                mic.drain();
                continue;
            }
        };
        let _ = live.end_agent_turn();
        mic.drain();
        if matches!(agent, AgentSource::Llm { .. }) {
            prior.push(("user".into(), stt.text().to_string()));
            prior.push(("assistant".into(), reply));
            if prior.len() > 16 {
                prior.drain(0..2);
            }
        }
    }
    live.shutdown();
    eprintln!("aurum converse: stopped");
    Ok(())
}

async fn run_stdio_loop(cli: ConverseCli, engine: aurum_core::AurumEngine) -> Result<()> {
    let stt_p = engine.stt_provider_id()?.as_str().to_string();
    let tts_p = engine.tts_provider_id()?.as_str().to_string();
    let live_cfg = LiveSessionConfig {
        max_utterance_secs: 30.0,
        min_speech_secs: 0.25,
        trailing_silence_secs: 0.45,
        min_rms: 0.02,
        max_speak_ahead: 8,
    };
    let mut live = LiveSession::new(engine, live_cfg)?;
    stdio_proto::write_event(&OutEvent::Ready {
        stt_provider: stt_p,
        tts_provider: tts_p,
    })
    .map_err(|e| UserError::Other {
        message: format!("stdio write: {e}"),
    })?;

    let mic = if cli.mic {
        Some(MicCapture::start()?)
    } else {
        None
    };

    if let Some(path) = cli.audio_file.as_ref() {
        let audio = aurum_core::load_audio(path).await?;
        live.push_pcm(audio.samples().as_ref())?;
        live.end_user_turn()?;
        match live.commit_user_turn().await {
            Ok(stt) if !stt.text().trim().is_empty() => {
                let _ = stdio_proto::write_event(&OutEvent::UserFinal {
                    text: stt.text().to_string(),
                    provider: stt.provider().to_string(),
                    model: stt.model().to_string(),
                });
            }
            Ok(_) => {
                live.discard_turn();
                let _ = stdio_proto::write_event(&OutEvent::Error {
                    message: "empty transcript".into(),
                });
            }
            Err(e) => {
                live.discard_turn();
                let _ = stdio_proto::write_event(&OutEvent::Error {
                    message: e.to_string(),
                });
            }
        }
    }

    let (cmd_tx, cmd_rx) = std::sync::mpsc::sync_channel::<std::result::Result<InCmd, String>>(64);
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        loop {
            let mut line = String::new();
            match stdin.read_line(&mut line) {
                Ok(0) => {
                    let _ = cmd_tx.send(Ok(InCmd::Shutdown));
                    break;
                }
                Ok(_) => {
                    if line.len() > MAX_LINE_BYTES {
                        let _ = cmd_tx.send(Err("stdin line too long".into()));
                        continue;
                    }
                    match stdio_proto::parse_line(&line) {
                        Ok(c) => {
                            if cmd_tx.send(Ok(c)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = cmd_tx.send(Err(e));
                        }
                    }
                }
                Err(e) => {
                    let _ = cmd_tx.send(Err(format!("stdin: {e}")));
                    break;
                }
            }
        }
    });

    enum PlayMsg {
        Clip(PlayClip),
        Flush(std::sync::mpsc::Sender<()>),
    }
    let (play_tx, play_rx) = std::sync::mpsc::sync_channel::<PlayMsg>(8);
    let play_thread = std::thread::spawn(move || {
        while let Ok(msg) = play_rx.recv() {
            match msg {
                PlayMsg::Clip(clip) => {
                    let _ = play_i16_mono(&clip.pcm, clip.rate);
                }
                PlayMsg::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    });

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            stop.store(true, Ordering::SeqCst);
        });
    }

    let mut shutting = false;
    while !stop.load(Ordering::SeqCst) && !shutting {
        while let Ok(msg) = cmd_rx.try_recv() {
            match msg {
                Ok(InCmd::Speak { text }) => match live.speak_text(&text).await {
                    Ok(tts) => {
                        drain_live(&mut live);
                        let _ = play_tx.send(PlayMsg::Clip(PlayClip {
                            pcm: tts.pcm_i16_mono,
                            rate: tts.sample_rate_hz,
                        }));
                    }
                    Err(e) => {
                        let _ = stdio_proto::write_event(&OutEvent::Error {
                            message: e.to_string(),
                        });
                    }
                },
                Ok(InCmd::EndTurn) => {
                    let (done_tx, done_rx) = std::sync::mpsc::channel();
                    let _ = play_tx.send(PlayMsg::Flush(done_tx));
                    let _ = done_rx.recv_timeout(Duration::from_secs(120));
                    let _ = live.end_agent_turn();
                    live.discard_turn();
                    if let Some(m) = mic.as_ref() {
                        m.drain();
                    }
                    let _ = stdio_proto::write_event(&OutEvent::Listening);
                }
                Ok(InCmd::Abort) => {
                    live.discard_turn();
                    if let Some(m) = mic.as_ref() {
                        m.drain();
                    }
                    let _ = stdio_proto::write_event(&OutEvent::Listening);
                }
                Ok(InCmd::Shutdown) => {
                    shutting = true;
                    break;
                }
                Err(e) => {
                    let _ = stdio_proto::write_event(&OutEvent::Error { message: e });
                }
            }
        }
        if shutting {
            break;
        }

        if let Some(mic) = mic.as_ref() {
            let chunk = tokio::task::block_in_place(|| mic.recv_timeout(Duration::from_millis(80)));
            if let Some(chunk) = chunk {
                if matches!(live.phase(), LivePhase::Listening | LivePhase::UserSpeaking) {
                    let pcm16 = resample_mono(&chunk, mic.sample_rate, WHISPER_SAMPLE_RATE);
                    if !pcm16.is_empty() {
                        if let Err(e) = live.push_pcm(&pcm16) {
                            let _ = stdio_proto::write_event(&OutEvent::Error {
                                message: e.to_string(),
                            });
                        }
                    }
                    if live.phase() == LivePhase::Ending {
                        match live.commit_user_turn().await {
                            Ok(stt) if !stt.text().trim().is_empty() => {
                                let _ = stdio_proto::write_event(&OutEvent::UserFinal {
                                    text: stt.text().to_string(),
                                    provider: stt.provider().to_string(),
                                    model: stt.model().to_string(),
                                });
                            }
                            Ok(_) => live.discard_turn(),
                            Err(e) => {
                                live.discard_turn();
                                let _ = stdio_proto::write_event(&OutEvent::Error {
                                    message: e.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        } else {
            tokio::task::block_in_place(|| {
                std::thread::sleep(Duration::from_millis(40));
            });
        }
    }

    drop(play_tx);
    let _ = play_thread.join();
    live.shutdown();
    let _ = stdio_proto::write_event(&OutEvent::Shutdown);
    Ok(())
}

fn drain_live(live: &mut LiveSession) {
    loop {
        if let LiveEvent::Idle = live.poll() {
            break;
        }
    }
}

struct PlayClip {
    pcm: Vec<i16>,
    rate: u32,
}

async fn speak_agent_turn(
    live: &mut LiveSession,
    agent: &AgentSource,
    user_text: &str,
    prior: &[(String, String)],
    save: Option<&std::path::Path>,
) -> Result<String> {
    let (play_tx, play_rx) = std::sync::mpsc::sync_channel::<PlayClip>(8);
    let play_thread = std::thread::spawn(move || {
        while let Ok(clip) = play_rx.recv() {
            if let Err(e) = play_i16_mono(&clip.pcm, clip.rate) {
                eprintln!("aurum: playback failed ({e})");
            }
        }
    });

    let send_clip = |clip: PlayClip, tx: &std::sync::mpsc::SyncSender<PlayClip>| -> Result<()> {
        if let Some(path) = save {
            let _ = aurum_core::write_wav_i16_mono_transaction(
                path,
                &clip.pcm,
                clip.rate,
                CommitMode::Replace,
            );
        }
        tx.send(clip).map_err(|_| UserError::Other {
            message: "playback queue closed".into(),
        })?;
        Ok(())
    };

    let full = match agent {
        AgentSource::Canned(text) => {
            eprintln!("aurum: {text}");
            let tts = live.speak_text(text).await?;
            drain_live(live);
            send_clip(
                PlayClip {
                    pcm: tts.pcm_i16_mono,
                    rate: tts.sample_rate_hz,
                },
                &play_tx,
            )?;
            text.clone()
        }
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
            let base = llm_base_url(cfg, *provider);
            let prior_owned = prior.to_vec();
            let (sent_tx, mut sent_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let produce = {
                let key = key.clone();
                let model = model.clone();
                let system = system.clone();
                let base = base.clone();
                let user_text = user_text.to_string();
                let provider = *provider;
                async move {
                    let prior_refs: Vec<(&str, &str)> = prior_owned
                        .iter()
                        .map(|(r, c)| (r.as_str(), c.as_str()))
                        .collect();
                    let mut buf = String::new();
                    let req = ChatRequest {
                        provider,
                        model: &model,
                        api_key: &key,
                        base_url: base.as_deref(),
                        user_text: &user_text,
                        system: system.as_deref().unwrap_or(""),
                        prior: &prior_refs,
                    };
                    let full = stream_chat(req, |d| {
                        buf.push_str(d);
                        while let Some(s) = take_speakable(&mut buf) {
                            let _ = sent_tx.send(s);
                        }
                    })
                    .await?;
                    if !buf.trim().is_empty() {
                        let _ = sent_tx.send(buf.trim().to_string());
                    }
                    Ok::<_, aurum_core::error::AurumError>(full)
                }
            };
            let consume = async {
                let mut shown = false;
                while let Some(s) = sent_rx.recv().await {
                    if s.trim().is_empty() {
                        continue;
                    }
                    if !shown {
                        eprint!("aurum: ");
                        shown = true;
                    }
                    eprint!("{s} ");
                    let tts = live.speak_text(&s).await?;
                    drain_live(live);
                    send_clip(
                        PlayClip {
                            pcm: tts.pcm_i16_mono,
                            rate: tts.sample_rate_hz,
                        },
                        &play_tx,
                    )?;
                }
                if shown {
                    eprintln!();
                }
                Ok::<_, aurum_core::error::AurumError>(())
            };
            let (full, cons) = tokio::join!(produce, consume);
            cons?;
            full?
        }
        AgentSource::Harness => {
            return Err(UserError::Other {
                message: "--stdio does not use in-process agent TTS".into(),
            }
            .into());
        }
    };
    drop(play_tx);
    let _ = play_thread.join();
    aurum_core::tts::validate::validate_text(&full)?;
    Ok(full)
}

#[derive(Debug)]
enum AgentSource {
    Canned(String),
    Llm {
        provider: LlmProvider,
        model: String,
        system: Option<String>,
    },
    /// Harness owns the LLM (`--stdio`).
    Harness,
}

fn resolve_agent_source(cli: &ConverseCli) -> Result<AgentSource> {
    let has_reply = cli.reply_text.is_some() || cli.reply_file.is_some();
    if cli.stdio {
        if cli.emit_json {
            return Err(UserError::Other {
                message: "--stdio and --emit-json both need stdout; pick one".into(),
            }
            .into());
        }
        if has_reply || cli.llm_provider.is_some() {
            return Err(UserError::Other {
                message: "--stdio is harness-brain; do not pass --llm-provider or --reply-*".into(),
            }
            .into());
        }
        return Ok(AgentSource::Harness);
    }
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
            AgentSource::Harness => panic!("expected llm"),
        }
    }

    #[test]
    fn clap_parses_mic() {
        let cli = crate::cli::Cli::try_parse_from([
            "aurum",
            "converse",
            "--mic",
            "--llm-provider",
            "openai",
            "--model",
            "tiny-q5_1",
        ])
        .unwrap();
        match cli.command {
            Some(crate::cli::Commands::Converse(c)) => {
                assert!(c.mic);
                assert!(c.audio_file.is_none());
                assert_eq!(c.llm_provider.as_deref(), Some("openai"));
            }
            other => panic!("expected Converse, got {other:?}"),
        }
    }

    #[test]
    fn clap_parses_stdio_and_rejects_llm() {
        let cli = crate::cli::Cli::try_parse_from([
            "aurum",
            "converse",
            "--mic",
            "--stdio",
            "--model",
            "tiny-q5_1",
        ])
        .unwrap();
        let Some(crate::cli::Commands::Converse(c)) = cli.command else {
            panic!("expected Converse");
        };
        assert!(c.stdio && c.mic);
        assert!(matches!(
            resolve_agent_source(&c).unwrap(),
            AgentSource::Harness
        ));

        let cli = crate::cli::Cli::try_parse_from([
            "aurum",
            "converse",
            "--mic",
            "--stdio",
            "--llm-provider",
            "openai",
        ])
        .unwrap();
        let Some(crate::cli::Commands::Converse(c)) = cli.command else {
            panic!("expected Converse");
        };
        assert!(resolve_agent_source(&c).is_err());
    }
}
