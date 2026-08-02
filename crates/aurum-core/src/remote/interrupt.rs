//! Race HTTP I/O against [`OpContext`] cancel and absolute deadline (JOE-1975).
//!
//! Reqwest's client-wide `timeout` is intentionally long for large uploads; per-operation
//! control must interrupt `.send()` and body reads independently so callers are not stuck
//! until that ceiling.

use super::limits::RemoteBodyLimits;
use crate::error::{ProviderError, Result};
use crate::runtime::OpContext;
use futures_util::StreamExt;
use reqwest::{RequestBuilder, Response};
use std::time::Duration;

/// Poll interval while waiting on interruptible I/O (cancel flags do not signal).
const POLL_SLICE: Duration = Duration::from_millis(25);

/// Sleep until cancel or deadline; never returns `Ok` successfully.
///
/// Used inside `tokio::select!` so pending HTTP work is dropped when the op ends.
async fn until_interrupt(op: &OpContext) {
    loop {
        if op.check().is_err() {
            return;
        }
        let slice = op
            .remaining()
            .map(|r| r.min(POLL_SLICE))
            .unwrap_or(POLL_SLICE);
        if slice.is_zero() {
            return;
        }
        tokio::time::sleep(slice).await;
    }
}

fn interrupt_error(op: &OpContext) -> crate::error::TranscriptionError {
    match op.check() {
        Err(e) => e,
        Ok(()) => ProviderError::DeadlineExceeded.into(),
    }
}

/// Send a request, aborting promptly on cancel or absolute deadline.
pub async fn send_with_op(req: RequestBuilder, op: &OpContext, provider: &str) -> Result<Response> {
    op.check()?;
    tokio::select! {
        biased;
        _ = until_interrupt(op) => Err(interrupt_error(op)),
        res = req.send() => {
            res.map_err(|e| {
                ProviderError::Network {
                    provider: provider.into(),
                    reason: e.to_string(),
                }
                .into()
            })
        }
    }
}

/// Stream a response body under a hard byte cap, interruptible by [`OpContext`].
pub async fn read_body_limited_with_op(
    response: Response,
    provider: &str,
    limits: RemoteBodyLimits,
    op: &OpContext,
) -> Result<Vec<u8>> {
    op.check()?;
    if let Some(cl) = response.content_length() {
        if cl as usize > limits.max_bytes {
            return Err(ProviderError::ResponseTooLarge {
                provider: provider.into(),
                reason: format!("Content-Length {cl} exceeds cap {}", limits.max_bytes),
            }
            .into());
        }
    }

    let mut out = Vec::new();
    let mut stream = response.bytes_stream();
    loop {
        op.check()?;
        let next = tokio::select! {
            biased;
            _ = until_interrupt(op) => {
                return Err(interrupt_error(op));
            }
            chunk = stream.next() => chunk,
        };
        match next {
            None => break,
            Some(Ok(chunk)) => {
                if out.len().saturating_add(chunk.len()) > limits.max_bytes {
                    return Err(ProviderError::ResponseTooLarge {
                        provider: provider.into(),
                        reason: format!("stream exceeded body cap of {} bytes", limits.max_bytes),
                    }
                    .into());
                }
                out.extend_from_slice(&chunk);
            }
            Some(Err(e)) => {
                return Err(ProviderError::Network {
                    provider: provider.into(),
                    reason: e.to_string(),
                }
                .into());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelFlag;
    use crate::error::TranscriptionError;
    use crate::remote::{HardenedHttpClient, RemotePolicy};
    use crate::runtime::OpContext;
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn send_cancelled_before_response_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
            .mount(&server)
            .await;

        let policy = RemotePolicy {
            allow_loopback_http: true,
            total_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(30),
            ..RemotePolicy::default()
        };
        let base = format!("{}/api/v1", server.uri());
        let http = HardenedHttpClient::openrouter(Some(&base), policy).unwrap();
        let cancel = CancelFlag::new();
        let op = OpContext::with_cancel(cancel.clone());
        let req = http
            .request(reqwest::Method::POST, "chat/completions", "sk-test")
            .unwrap()
            .json(&serde_json::json!({"model": "x", "messages": []}));

        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            cancel2.cancel();
        });

        let err = send_with_op(req, &op, "openrouter").await.unwrap_err();
        assert!(
            matches!(err, TranscriptionError::Provider(ProviderError::Cancelled)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn send_deadline_beats_long_client_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
            .mount(&server)
            .await;

        let policy = RemotePolicy {
            allow_loopback_http: true,
            total_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(30),
            ..RemotePolicy::default()
        };
        let base = format!("{}/api/v1", server.uri());
        let http = HardenedHttpClient::openrouter(Some(&base), policy).unwrap();
        let op = OpContext::new().with_deadline_from_now(Duration::from_millis(100));
        let req = http
            .request(reqwest::Method::POST, "chat/completions", "sk-test")
            .unwrap()
            .json(&serde_json::json!({"model":"x","messages":[]}));

        let t0 = std::time::Instant::now();
        let err = send_with_op(req, &op, "openrouter").await.unwrap_err();
        let elapsed = t0.elapsed();
        assert!(
            matches!(
                err,
                TranscriptionError::Provider(ProviderError::DeadlineExceeded)
            ),
            "got {err:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "deadline should interrupt well before client timeout, elapsed={elapsed:?}"
        );
    }

    #[tokio::test]
    async fn remote_permit_cap_isolates_engines() {
        use crate::runtime::{GovernorConfig, PermitKind, ResourceGovernor};

        let cfg = GovernorConfig {
            max_remote: 1,
            fail_fast: true,
            ..GovernorConfig::default()
        };
        let a = Arc::new(ResourceGovernor::try_new(cfg.clone()).unwrap());
        let b = Arc::new(ResourceGovernor::try_new(cfg).unwrap());
        let op = OpContext::new();
        let _p = a.acquire(PermitKind::Remote, Some(&op)).unwrap();
        assert!(a.acquire(PermitKind::Remote, Some(&op)).is_err());
        // Independent engine still has capacity.
        assert!(b.acquire(PermitKind::Remote, Some(&op)).is_ok());
    }
}
