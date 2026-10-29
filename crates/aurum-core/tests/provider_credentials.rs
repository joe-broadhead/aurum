//! Missing credentials fail before constructing an HTTP client or making a request.

use aurum_core::dto::ErrorDto;
use aurum_core::error::{AurumError, ErrorCategory, Result, UserError};
use aurum_core::remote::RemotePolicy;
use aurum_core::secret::SecretString;

fn assert_missing_credentials<T>(
    provider: &str,
    env_key: &str,
    build: impl Fn(Option<SecretString>) -> Result<T>,
) {
    for key in [None, Some(""), Some(" \t\r\n")] {
        let err = build(key.map(SecretString::new))
            .err()
            .expect("absent, empty and whitespace-only keys must fail");
        match &err {
            AurumError::User(UserError::MissingProviderCredential { provider: actual }) => {
                assert_eq!(actual, provider);
            }
            other => panic!("expected a credential error for {provider}, got {other:?}"),
        }
        assert_eq!(err.category(), "user");
        assert_eq!(err.error_category(), ErrorCategory::Auth);
        assert_eq!(err.exit_code(), 2);
        assert!(!err.retryable());

        let dto = ErrorDto::from_error(&err);
        assert_eq!(dto.category, "auth");
        assert_eq!(dto.exit_code, 2);
        assert!(!dto.retryable);
        assert!(dto.message.contains(&format!("provider '{provider}'")));
        assert!(dto.message.contains(env_key), "{}", dto.message);
        assert!(dto.message.contains(&format!("[providers.{provider}]")));
        assert!(!dto.message.contains("OpenRouter"));
        assert!(!dto.message.contains("OPENROUTER_API_KEY"));
    }
}

#[test]
fn openai_stt_missing_credentials_are_provider_scoped() {
    assert_missing_credentials("openai", "OPENAI_API_KEY", |key| {
        aurum_core::providers::openai_stt::OpenAiSttProvider::with_policy(
            key,
            None,
            RemotePolicy::default(),
        )
    });
}

#[test]
fn xai_stt_missing_credentials_are_provider_scoped() {
    assert_missing_credentials("xai", "XAI_API_KEY", |key| {
        aurum_core::providers::xai_stt::XaiSttProvider::with_policy(
            key,
            None,
            RemotePolicy::default(),
        )
    });
}

#[cfg(feature = "tts")]
#[test]
fn openai_tts_missing_credentials_are_provider_scoped() {
    assert_missing_credentials("openai", "OPENAI_API_KEY", |key| {
        aurum_core::providers::openai_tts::OpenAiTtsProvider::with_policy(
            key,
            None,
            RemotePolicy::default(),
        )
    });
}

#[cfg(feature = "tts")]
#[test]
fn elevenlabs_tts_missing_credentials_are_provider_scoped() {
    assert_missing_credentials("elevenlabs", "ELEVENLABS_API_KEY", |key| {
        aurum_core::providers::elevenlabs_tts::ElevenLabsTtsProvider::with_policy(
            key,
            None,
            RemotePolicy::default(),
        )
    });
}

#[cfg(feature = "tts")]
#[test]
fn xai_tts_missing_credentials_are_provider_scoped() {
    assert_missing_credentials("xai", "XAI_API_KEY", |key| {
        aurum_core::providers::xai_tts::XaiTtsProvider::with_policy(
            key,
            None,
            RemotePolicy::default(),
        )
    });
}

#[test]
fn openrouter_credential_guidance_remains_available() {
    for err in [
        UserError::MissingApiKey,
        UserError::MissingProviderCredential {
            provider: "openrouter".into(),
        },
    ] {
        let message = err.to_string();
        assert!(message.contains("OPENROUTER_API_KEY"));
        assert!(message.contains("[providers.openrouter]"));
    }
}

#[test]
fn unknown_provider_credential_guidance_does_not_invent_an_environment_variable() {
    let message = UserError::MissingProviderCredential {
        provider: "custom-provider".into(),
    }
    .to_string();
    assert!(message.contains("provider 'custom-provider'"));
    assert!(message.contains("matching environment variable or config section"));
    assert!(!message.contains("_API_KEY"));
}
