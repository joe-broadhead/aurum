//! [`LiveSession`] — engine-backed conversation loop.

use super::turn::LiveTurn;
use super::{LiveEvent, LivePhase, LiveSessionConfig};
use crate::cancel::CancelFlag;
use crate::config::Config;
use crate::engine::AurumEngine;
use crate::error::{ProviderError, Result, UserError};
use crate::provider_platform::{preflight_stt_with_registry, preflight_tts_with_registry};
use crate::providers::{OpenRouterSttMode, TranscriptionOptions, TranscriptionResult};
use crate::tts::provider::{SynthesisOptions, SynthesisResult};
use crate::tts::validate::validate_text;
use std::collections::VecDeque;

/// Device-agnostic conversation session.
///
/// STT/TTS providers are taken from the engine config (independent `[stt]` /
/// `[tts]`). This type does not take provider ids of its own.
pub struct LiveSession {
    engine: AurumEngine,
    turn: LiveTurn,
    cfg: LiveSessionConfig,
    events: VecDeque<LiveEvent>,
    cancel: CancelFlag,
    pending_agent: usize,
    sidecar_busy: bool,
}

impl LiveSession {
    /// Preflight STT + TTS from the engine registry, then open a listening turn.
    pub fn new(engine: AurumEngine, config: LiveSessionConfig) -> Result<Self> {
        config.validate()?;
        if engine.is_closed() {
            return Err(UserError::Other {
                message: "AurumEngine is closed".into(),
            }
            .into());
        }
        preflight_engine(&engine)?;
        Ok(Self {
            engine,
            turn: LiveTurn::new(config.clone()),
            cfg: config,
            events: VecDeque::new(),
            cancel: CancelFlag::new(),
            pending_agent: 0,
            sidecar_busy: false,
        })
    }

    pub fn phase(&self) -> LivePhase {
        self.turn.phase()
    }

    pub fn engine(&self) -> &AurumEngine {
        &self.engine
    }

    pub fn config(&self) -> &LiveSessionConfig {
        &self.cfg
    }

    pub fn ignored_while_speaking_samples(&self) -> usize {
        self.turn.ignored_total()
    }

    /// Cooperative cancel for in-flight STT/TTS (does not change phase).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Clone of the session cancel flag (stdin watchers / abort).
    pub fn cancel_flag(&self) -> CancelFlag {
        self.cancel.clone()
    }

    /// Exclusive sidecar op (transcribe/synthesize). Fail closed if busy.
    pub fn begin_sidecar_op(&mut self) -> Result<()> {
        if self.sidecar_busy {
            return Err(ProviderError::Overload {
                reason: "sidecar is busy".into(),
            }
            .into());
        }
        self.sidecar_busy = true;
        Ok(())
    }

    pub fn end_sidecar_op(&mut self) {
        self.sidecar_busy = false;
    }

    pub fn is_sidecar_busy(&self) -> bool {
        self.sidecar_busy
    }

    /// Push 16 kHz mono f32. Ignored while [`LivePhase::Speaking`].
    pub fn push_pcm(&mut self, samples: &[f32]) -> Result<()> {
        let became = self.turn.push_pcm(samples)?;
        if became {
            self.events.push_back(LiveEvent::UserSpeaking {
                samples: self.turn.buffered_samples(),
            });
        }
        Ok(())
    }

    /// Drain the next event (`Idle` when the queue is empty).
    pub fn poll(&mut self) -> LiveEvent {
        if let Some(ev) = self.events.pop_front() {
            if matches!(ev, LiveEvent::AgentPcm { .. }) {
                self.pending_agent = self.pending_agent.saturating_sub(1);
            }
            return ev;
        }
        let ignored = self.turn.take_ignored_pending();
        if ignored > 0 {
            return LiveEvent::IgnoredWhileSpeaking { samples: ignored };
        }
        LiveEvent::Idle
    }

    /// Explicit user endpoint (replay EOF). Does not run STT.
    pub fn end_user_turn(&mut self) -> Result<()> {
        self.turn.end_user_turn()
    }

    /// Run engine STT on the frozen utterance and emit [`LiveEvent::UserFinal`].
    pub async fn commit_user_turn(&mut self) -> Result<TranscriptionResult> {
        if self.turn.phase() == LivePhase::UserSpeaking || self.turn.phase() == LivePhase::Listening
        {
            self.turn.end_user_turn()?;
        }
        let samples = self.turn.take_utterance()?;
        self.cancel.reset();
        let cfg = self.engine.config();
        let model = cfg.resolve_model(cfg.model.is_some())?;
        let opts = TranscriptionOptions {
            model,
            language: cfg.language.clone(),
            timestamps: false,
            cancel: Some(self.cancel.clone()),
            op: None,
        };
        match self.engine.transcribe_pcm(&samples, &opts).await {
            Ok(result) => self.finish_user_result(result),
            Err(e) => {
                // take_utterance already consumed the buffer; do not stay in Ending.
                self.discard_turn();
                Err(e)
            }
        }
    }

    /// Host-supplied final transcript (tests / external ASR). Still requires an
    /// ended user turn with PCM so duration honesty is preserved.
    pub fn complete_user_turn(
        &mut self,
        result: TranscriptionResult,
    ) -> Result<TranscriptionResult> {
        if self.turn.phase() == LivePhase::UserSpeaking || self.turn.phase() == LivePhase::Listening
        {
            self.turn.end_user_turn()?;
        }
        let _samples = self.turn.take_utterance()?;
        self.finish_user_result(result)
    }

    fn finish_user_result(&mut self, result: TranscriptionResult) -> Result<TranscriptionResult> {
        self.turn.enter_thinking()?;
        self.events.push_back(LiveEvent::UserFinal {
            text: result.text().to_string(),
            result: result.clone(),
        });
        Ok(result)
    }

    /// Synthesize `text` via the engine TTS provider and enqueue [`LiveEvent::AgentPcm`].
    pub async fn speak_text(&mut self, text: &str) -> Result<SynthesisResult> {
        validate_text(text)?;
        self.prepare_speak()?;
        self.cancel.reset();
        let cfg = self.engine.config();
        let id = self.engine.tts_provider_id()?;
        let model = crate::resolve_tts_model(id.as_str(), None, &cfg.tts_model)?;
        // Pass a non-empty config voice as explicit so ElevenLabs (no default
        // voice) can use `[tts].voice` rather than requiring a CLI flag.
        let voice_cfg = cfg.tts_voice.trim();
        let voice = crate::resolve_tts_voice(
            id.as_str(),
            (!voice_cfg.is_empty()).then_some(voice_cfg),
            &cfg.tts_voice,
        )?;
        let opts = SynthesisOptions {
            model,
            voice,
            language: cfg.tts_language.clone(),
            speaking_rate: cfg.tts_speaking_rate,
            timeout_ms: cfg.tts_timeout_ms,
            cancel: Some(self.cancel.clone()),
            local_only: cfg.local_only,
            ..Default::default()
        };
        let result = self.engine.synthesize(text, &opts).await?;
        self.enqueue_agent(result)
    }

    /// Host-supplied agent PCM (tests / pre-rendered audio).
    pub fn speak_result(&mut self, result: SynthesisResult) -> Result<SynthesisResult> {
        self.prepare_speak()?;
        if result.pcm_i16_mono.is_empty() {
            return Err(UserError::Other {
                message: "agent PCM is empty".into(),
            }
            .into());
        }
        self.enqueue_agent(result)
    }

    fn prepare_speak(&mut self) -> Result<()> {
        if self.pending_agent >= self.cfg.max_speak_ahead {
            return Err(ProviderError::Overload {
                reason: format!(
                    "live speak-ahead cap reached ({})",
                    self.cfg.max_speak_ahead
                ),
            }
            .into());
        }
        self.turn.begin_speaking()
    }

    fn enqueue_agent(&mut self, result: SynthesisResult) -> Result<SynthesisResult> {
        self.pending_agent = self.pending_agent.saturating_add(1);
        self.events.push_back(LiveEvent::AgentPcm {
            result: result.clone(),
        });
        Ok(result)
    }

    /// Host has finished playing agent audio; return to listening.
    pub fn end_agent_turn(&mut self) -> Result<()> {
        self.turn.end_agent_turn()?;
        self.pending_agent = 0;
        // Drop unconsumed agent PCM so a new turn cannot play stale audio.
        self.events
            .retain(|e| !matches!(e, LiveEvent::AgentPcm { .. }));
        Ok(())
    }

    /// Abort the current user/agent turn and return to listening.
    pub fn discard_turn(&mut self) {
        self.turn.discard_to_listening();
        self.pending_agent = 0;
        self.events.clear();
    }

    /// Close the session and shut down the engine (Metal-safe).
    pub fn shutdown(&mut self) {
        self.turn.close();
        self.engine.shutdown();
        self.events.clear();
        self.pending_agent = 0;
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

fn preflight_engine(engine: &AurumEngine) -> Result<()> {
    let cfg: &Config = engine.config();
    let stt_id = engine.stt_provider_id()?;
    let tts_id = engine.tts_provider_id()?;
    let mode = OpenRouterSttMode::parse(&cfg.openrouter_stt_mode)?;
    let model = cfg.resolve_model(cfg.model.is_some())?;
    preflight_stt_with_registry(
        engine.registry(),
        &stt_id,
        &model,
        false,
        cfg.local_only,
        mode,
    )?;
    preflight_tts_with_registry(
        engine.registry(),
        &tts_id,
        &cfg.tts_model,
        &cfg.tts_language,
        cfg.local_only,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::provider_platform::ProviderId;
    use crate::providers::elevenlabs_tts::EXAMPLE_ELEVENLABS_VOICE_ID;
    use crate::secret::SecretString;
    use crate::tts::provider::BackendKind;
    use wiremock::matchers::{header, method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn isolated_engine() -> (tempfile::TempDir, AurumEngine) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        cfg.local_only = true;
        cfg.model = Some("tiny-q5_1".into());
        let engine = AurumEngine::from_config(cfg).unwrap();
        (dir, engine)
    }

    fn canned_tts() -> SynthesisResult {
        SynthesisResult {
            pcm_i16_mono: vec![0; 240],
            sample_rate_hz: 24_000,
            channels: 1,
            backend_kind: BackendKind::Local,
            provider: "local".into(),
            model: "kitten-nano-int8".into(),
            voice: "Luna".into(),
            language: "en".into(),
            duration_ms: 10,
            text_chars: 5,
            text_truncated: false,
            chunk_count: 1,
            synthesized_chars: 5,
            adapter: None,
            trust: None,
            provenance: None,
        }
    }

    fn canned_stt(text: &str, duration_secs: f64) -> TranscriptionResult {
        TranscriptionResult::try_local(
            text.into(),
            vec![],
            Some("en".into()),
            "tiny-q5_1".into(),
            duration_secs,
        )
        .unwrap()
    }

    fn fixture_wav() -> std::path::PathBuf {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            root.join("../../tests/fixtures/sample.wav"),
            root.join("tests/fixtures/sample.wav"),
            std::path::PathBuf::from("tests/fixtures/sample.wav"),
        ];
        candidates
            .into_iter()
            .find(|p| p.is_file())
            .expect("tests/fixtures/sample.wav")
    }

    #[test]
    fn new_preflights_local() {
        let (_dir, engine) = isolated_engine();
        let live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        assert_eq!(live.phase(), LivePhase::Listening);
        assert_eq!(
            live.engine().stt_provider_id().unwrap(),
            ProviderId::local()
        );
    }

    #[test]
    fn local_only_engine_rejects_elevenlabs_tts_at_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        cfg.local_only = true;
        cfg.tts_provider = "elevenlabs".into();
        let err = AurumEngine::from_config(cfg).unwrap_err();
        assert!(
            err.to_string().contains("local_only") || err.to_string().contains("elevenlabs"),
            "{err}"
        );
    }

    #[test]
    fn fixture_replay_half_duplex_without_models() {
        let (_dir, engine) = isolated_engine();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        let wav = crate::audio::try_load_wav_file(
            &fixture_wav(),
            crate::audio::DEFAULT_MAX_DURATION_SECS,
            crate::audio::DEFAULT_MAX_DECODED_BYTES,
        )
        .unwrap();
        let pcm = wav.samples();
        for chunk in pcm.chunks(320) {
            live.push_pcm(chunk).unwrap();
        }
        live.end_user_turn().unwrap();
        let duration = pcm.len() as f64 / 16_000.0;
        live.complete_user_turn(canned_stt("hello from fixture", duration))
            .unwrap();

        match live.poll() {
            LiveEvent::UserSpeaking { .. } => {
                assert!(
                    matches!(live.poll(), LiveEvent::UserFinal { ref text, .. } if text.contains("hello"))
                );
            }
            LiveEvent::UserFinal { text, .. } => assert!(text.contains("hello")),
            other => panic!("unexpected {other:?}"),
        }

        live.speak_result(canned_tts()).unwrap();
        assert!(matches!(live.poll(), LiveEvent::AgentPcm { .. }));

        live.push_pcm(&[0.8; 160]).unwrap();
        assert!(matches!(
            live.poll(),
            LiveEvent::IgnoredWhileSpeaking { samples: 160 }
        ));
        assert_eq!(live.ignored_while_speaking_samples(), 160);

        live.end_agent_turn().unwrap();
        assert_eq!(live.phase(), LivePhase::Listening);
    }

    #[test]
    fn speak_before_user_final_fails() {
        let (_dir, engine) = isolated_engine();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        let err = live.speak_result(canned_tts()).unwrap_err();
        assert!(err.to_string().contains("Thinking") || err.to_string().contains("speak"));
    }

    #[tokio::test]
    async fn empty_speak_text_fails() {
        let (_dir, engine) = isolated_engine();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        live.push_pcm(&[0.3; 800]).unwrap();
        live.end_user_turn().unwrap();
        live.complete_user_turn(canned_stt("hi", 0.05)).unwrap();
        let err = live.speak_text("   ").await.unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
    }

    #[test]
    fn speak_ahead_cap() {
        let (_dir, engine) = isolated_engine();
        let cfg = LiveSessionConfig {
            max_speak_ahead: 1,
            ..Default::default()
        };
        let mut live = LiveSession::new(engine, cfg).unwrap();
        live.push_pcm(&[0.3; 800]).unwrap();
        live.end_user_turn().unwrap();
        live.complete_user_turn(canned_stt("hi", 0.05)).unwrap();
        live.speak_result(canned_tts()).unwrap();
        let err = live.speak_result(canned_tts()).unwrap_err();
        assert!(err.to_string().contains("speak-ahead") || err.to_string().contains("overload"));
    }

    #[test]
    fn shutdown_rejects_push() {
        let (_dir, engine) = isolated_engine();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        live.shutdown();
        assert!(live.push_pcm(&[0.1; 10]).is_err());
    }

    #[tokio::test]
    async fn stt_failure_returns_to_listening() {
        let (_dir, engine) = isolated_engine();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        live.push_pcm(&[0.3; 1600]).unwrap();
        live.end_user_turn().unwrap();
        assert_eq!(live.phase(), LivePhase::Ending);
        assert!(live.commit_user_turn().await.is_err());
        assert_eq!(live.phase(), LivePhase::Listening);
    }

    #[tokio::test]
    async fn mixed_openai_stt_elevenlabs_tts_via_engine() {
        let stt_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "hello from openai mock"
            })))
            .mount(&stt_server)
            .await;

        let n = 2_400;
        let mut pcm = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = ((i % 30) as i16) * 15;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        let tts_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/text-to-speech/[^/]+$"))
            .and(header("xi-api-key", "el-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "audio/pcm")
                    .set_body_bytes(pcm),
            )
            .mount(&tts_server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        cfg.local_only = false;
        cfg.provider = "openai".into();
        cfg.model = Some("whisper-1".into());
        cfg.language = "en".into();
        cfg.providers.openai.api_key = Some(SecretString::new("sk-test"));
        cfg.providers.openai.base_url = Some(stt_server.uri());
        cfg.tts_provider = "elevenlabs".into();
        cfg.tts_model = "eleven_flash_v2_5".into();
        cfg.tts_voice = EXAMPLE_ELEVENLABS_VOICE_ID.into();
        cfg.providers.elevenlabs.api_key = Some(SecretString::new("el-test"));
        cfg.providers.elevenlabs.base_url = Some(tts_server.uri());

        let engine = AurumEngine::from_config(cfg).unwrap();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        live.push_pcm(&vec![0.1f32; 16_000]).unwrap();
        live.end_user_turn().unwrap();
        let stt = live.commit_user_turn().await.unwrap();
        assert_eq!(stt.text(), "hello from openai mock");
        assert_eq!(stt.provider(), "openai");

        let tts = live.speak_text("Hello ElevenLabs").await.unwrap();
        assert_eq!(tts.provider, "elevenlabs");
        assert_eq!(tts.backend_kind, BackendKind::Remote);
        assert_eq!(tts.pcm_i16_mono.len(), n);
        assert_eq!(tts.voice, EXAMPLE_ELEVENLABS_VOICE_ID);
    }

    #[tokio::test]
    async fn elevenlabs_local_alias_voice_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::load_from(&dir.path().join("nope.toml")).unwrap();
        cfg.cache_dir = dir.path().join("cache");
        cfg.provider = "local".into();
        cfg.tts_provider = "elevenlabs".into();
        cfg.tts_model = "eleven_flash_v2_5".into();
        cfg.tts_voice = "Luna".into(); // local alias — must fail closed
        cfg.providers.elevenlabs.api_key = Some(SecretString::new("el-test"));
        let engine = AurumEngine::from_config(cfg).unwrap();
        let mut live = LiveSession::new(engine, LiveSessionConfig::default()).unwrap();
        live.push_pcm(&[0.3; 800]).unwrap();
        live.end_user_turn().unwrap();
        live.complete_user_turn(canned_stt("hi", 0.05)).unwrap();
        let err = live.speak_text("hi").await.unwrap_err();
        let msg = err.to_string().to_ascii_lowercase();
        assert!(msg.contains("voice") || msg.contains("eleven"), "{err}");
    }
}
