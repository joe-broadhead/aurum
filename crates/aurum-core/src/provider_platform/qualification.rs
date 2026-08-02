//! Cross-provider qualification tests (JOE-1943).
//!
//! Deterministic checks: isolation, local-only with synthetic keys present,
//! and registry coverage for the production vertical set.

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::engine::AurumEngine;
    use crate::provider_platform::{
        check_builtin_conformance, list_provider_summaries, ProviderId, ProviderRegistry,
        ProviderResolveOptions,
    };
    use crate::secret::SecretString;

    #[test]
    fn builtin_covers_all_epic_verticals() {
        let reg = ProviderRegistry::builtin().unwrap();
        let ids: Vec<_> = list_provider_summaries(&reg)
            .into_iter()
            .map(|s| s.id)
            .collect();
        // STT (+ dual) verticals are always registered. ElevenLabs is TTS-only and only
        // appears when the `tts` feature is enabled (e.g. default features / full build).
        for need in ["local", "openrouter", "openai", "xai"] {
            assert!(ids.iter().any(|i| i == need), "missing provider {need}");
        }
        #[cfg(feature = "tts")]
        assert!(
            ids.iter().any(|i| i == "elevenlabs"),
            "missing provider elevenlabs"
        );
        check_builtin_conformance(&reg).unwrap();
    }

    #[test]
    fn local_only_rejects_all_remote_factories() {
        let reg = ProviderRegistry::builtin().unwrap();
        let mut cfg = Config::load().unwrap();
        cfg.local_only = true;
        // Synthetic keys present — must still not allow remote builds under local_only.
        cfg.openrouter_api_key = Some(SecretString::new("sk-or-v1-SYNTH-QUAL-001"));
        cfg.providers.openai.api_key = Some(SecretString::new("sk-oai-SYNTH-QUAL-001"));
        cfg.providers.elevenlabs.api_key = Some(SecretString::new("el-SYNTH-QUAL-001"));
        cfg.providers.xai.api_key = Some(SecretString::new("xai-SYNTH-QUAL-001"));
        let engine = AurumEngine::from_config(cfg).unwrap();
        let opts = ProviderResolveOptions {
            local_only: Some(true),
            ..Default::default()
        };
        for id in ["openrouter", "openai", "xai"] {
            let pid = ProviderId::must(id);
            let err = engine.stt_provider_with(&pid, opts.clone());
            assert!(err.is_err(), "STT {id} should fail under local_only");
        }
        #[cfg(feature = "tts")]
        {
            for id in ["openrouter", "openai", "elevenlabs", "xai"] {
                let pid = ProviderId::must(id);
                let err = engine.tts_provider_with(&pid, opts.clone());
                assert!(err.is_err(), "TTS {id} should fail under local_only");
            }
            // Local still builds.
            assert!(engine
                .tts_provider_with(&ProviderId::local(), opts.clone())
                .is_ok());
        }
        assert!(engine
            .stt_provider_with(&ProviderId::local(), opts.clone())
            .is_ok());
        let _ = reg;
    }

    #[test]
    fn provider_policies_do_not_share_auth_headers() {
        use crate::remote::{
            ElevenLabsHttpPolicy, OpenAiHttpPolicy, OpenRouterHttpPolicy, ProviderHttpPolicy,
            XaiHttpPolicy,
        };
        use reqwest::Client;

        let key = "sk-CROSS-PROVIDER-CANARY-001";
        let client = Client::new();

        let or = OpenRouterHttpPolicy
            .apply_auth(client.get("https://openrouter.ai/api/v1/x"), key)
            .build()
            .unwrap();
        assert!(or.headers().get("Authorization").is_some());
        assert!(or.headers().get("xi-api-key").is_none());

        let oai = OpenAiHttpPolicy
            .apply_auth(client.get("https://api.openai.com/v1/x"), key)
            .build()
            .unwrap();
        assert!(oai.headers().get("Authorization").is_some());
        // OpenAI must not get OpenRouter-only title headers from its policy.
        let oai_extra = OpenAiHttpPolicy
            .apply_extra_headers(client.get("https://api.openai.com/v1/x"))
            .build()
            .unwrap();
        assert!(oai_extra.headers().get("X-Title").is_none());
        assert!(oai_extra.headers().get("X-OpenRouter-Title").is_none());

        let el = ElevenLabsHttpPolicy
            .apply_auth(client.get("https://api.elevenlabs.io/v1/x"), key)
            .build()
            .unwrap();
        assert!(el.headers().get("xi-api-key").is_some());
        assert!(el.headers().get("Authorization").is_none());

        let xai = XaiHttpPolicy
            .apply_auth(client.get("https://api.x.ai/v1/x"), key)
            .build()
            .unwrap();
        assert!(xai.headers().get("Authorization").is_some());
        assert!(xai.headers().get("xi-api-key").is_none());
    }

    #[test]
    fn grok_alias_is_canonical_xai() {
        let id = ProviderId::parse("grok").unwrap();
        assert_eq!(id.as_str(), "xai");
        let reg = ProviderRegistry::builtin().unwrap();
        assert!(reg.stt_factory(&id).is_ok());
        #[cfg(feature = "tts")]
        assert!(reg.tts_factory(&id).is_ok());
    }
}
