//! Startup readiness for the CoreLink runner mint seam.
//!
//! An armed mint is usable only after the dispatcher has accepted the exact
//! authenticated `{}` self-check. The check is deliberately weaker than a
//! mint: it proves dispatcher-key routing and the typed request envelope while
//! never naming a tenant, job, repo, or bearer token.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::sync::Notify;

use crate::runner_cas_mint::{MintHttp, MintHttpResponse, UreqMint};

const PROBE_BODY: &str = "{}";
const MAX_ATTEMPTS: u8 = 3;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const READINESS_WAIT: Duration = Duration::from_secs(11);
const MAX_RESPONSE_BYTES: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unknown,
    Running,
    Ready,
    Terminal,
}

struct Inner {
    state: Mutex<State>,
    changed: Notify,
}

/// Process-local, single-flight mint readiness.
pub struct MintReadiness {
    url: String,
    auth: String,
    http: Arc<dyn MintHttp>,
    inner: Arc<Inner>,
}

impl MintReadiness {
    /// Build a readiness checker over a testable transport seam.
    pub fn new(
        url: impl Into<String>,
        auth: impl Into<String>,
        http: Arc<dyn MintHttp>,
    ) -> Arc<Self> {
        Arc::new(Self {
            url: url.into().trim_end_matches('/').to_owned(),
            auth: auth.into(),
            http,
            inner: Arc::new(Inner {
                state: Mutex::new(State::Unknown),
                changed: Notify::new(),
            }),
        })
    }

    /// Production checker, using the same ureq transport and timeout policy as
    /// the mint client. It is created only when both mint env vars are armed.
    pub fn production(url: impl Into<String>, auth: impl Into<String>) -> Arc<Self> {
        Self::new(
            url,
            auth,
            Arc::new(UreqMint::new(PROBE_TIMEOUT).with_response_limit(MAX_RESPONSE_BYTES)),
        )
    }

    fn state(&self) -> State {
        *self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn start(self: &Arc<Self>) {
        let should_start = {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if *state == State::Unknown {
                *state = State::Running;
                true
            } else {
                false
            }
        };
        if !should_start {
            return;
        }
        let this = Arc::clone(self);
        // This task owns the probe. Dropping a request future cannot cancel it,
        // and therefore cannot permit a second probe to start.
        tokio::spawn(async move {
            let probe_owner = Arc::clone(&this);
            let result = tokio::task::spawn_blocking(move || probe_owner.probe()).await;
            let ready = matches!(result, Ok(true));
            let mut state = this.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            *state = if ready { State::Ready } else { State::Terminal };
            this.inner.changed.notify_waiters();
        });
    }

    fn probe(&self) -> bool {
        for attempt in 0..MAX_ATTEMPTS {
            match self.http.post(
                &format!("{}/internal/v1/runner/mint", self.url),
                &self.auth,
                None,
                PROBE_BODY,
            ) {
                Ok(response) => {
                    if is_expected(response) {
                        return true;
                    }
                    return false;
                }
                Err(_) if attempt + 1 < MAX_ATTEMPTS => continue,
                Err(_) => return false,
            }
        }
        false
    }

    /// Wait until the self-check has passed. Returns false for terminal failure
    /// or a bounded wait timeout.
    pub async fn wait_ready(self: &Arc<Self>) -> bool {
        self.start();
        loop {
            let notified = self.inner.changed.notified();
            match self.state() {
                State::Ready => return true,
                State::Terminal => return false,
                State::Unknown | State::Running => {
                    if tokio::time::timeout(READINESS_WAIT, notified)
                        .await
                        .is_err()
                    {
                        return false;
                    }
                }
            }
        }
    }

    /// The cheap unauthenticated readiness endpoint.
    pub async fn endpoint(AxumState(state): AxumState<crate::AppState>) -> Response {
        if Self::acquire_guard(state.mint_readiness.as_ref()).await {
            StatusCode::OK.into_response()
        } else {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }

    /// Fast guard used by acquire before any admission or provider work.
    pub async fn acquire_guard(readiness: Option<&Arc<Self>>) -> bool {
        match readiness {
            Some(readiness) => readiness.wait_ready().await,
            None => true,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedError {
    error: String,
    message: String,
    request_id: String,
}

fn is_expected(response: MintHttpResponse) -> bool {
    if response.status != 400 || response.body.len() > MAX_RESPONSE_BYTES {
        return false;
    }
    serde_json::from_str::<ExpectedError>(&response.body)
        .map(|body| {
            body.error == "BAD_REQUEST"
                && body.message == "job_id required"
                && !body.request_id.trim().is_empty()
        })
        .unwrap_or(false)
}

/// Test helper: classify the exact typed response without making a request.
pub fn accepts_probe_response(response: MintHttpResponse) -> bool {
    is_expected(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_typed_400_is_accepted() {
        let body = r#"{"error":"BAD_REQUEST","message":"job_id required","request_id":"r"}"#;
        assert!(accepts_probe_response(MintHttpResponse {
            status: 400,
            body: body.into()
        }));
        for (status, body) in [
            (401, body),
            (
                400,
                r#"{"error":"BAD_REQUEST","message":"other","request_id":"r"}"#,
            ),
            (400, "{}"),
            (200, body),
            (400, "<html>"),
        ] {
            assert!(!accepts_probe_response(MintHttpResponse {
                status,
                body: body.into()
            }));
        }
    }
}
