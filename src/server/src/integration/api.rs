//! The integration API: what sidecars (the Discord bot) call. Its own
//! listener (`INTEGRATION_HOST:INTEGRATION_PORT`), never published; every
//! request needs `Authorization: Bearer <token>`, and the token says which
//! platform is calling. Errors are `{error, reason, retry_after_ms?}`.

use super::{Actor, IntError, Integration};
use crate::password::ct_eq;
use crate::utils::now_ms;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use log::{error, info, warn};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;

const MAX_BODY: usize = 16 * 1024;

#[derive(Clone)]
pub struct ApiState {
    hub: Integration,
    /// `(provider, SHA-256 of its token)`.
    tokens: Arc<Vec<(String, [u8; 32])>>,
}

impl ApiState {
    pub fn new(hub: Integration, tokens: &[(String, String)]) -> Self {
        Self {
            hub,
            tokens: Arc::new(tokens.iter().map(|(p, t)| (p.clone(), digest(t))).collect()),
        }
    }
}

fn digest(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

/// The platform the calling sidecar speaks for.
#[derive(Clone)]
struct Provider(String);

async fn require_token(State(state): State<ApiState>, mut req: Request, next: Next) -> Response {
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| digest(t.trim()));
    // Compare against every token, without stopping at the first match.
    let mut found = None;
    if let Some(p) = presented {
        for (provider, d) in state.tokens.iter() {
            if ct_eq(&p, d) {
                found = Some(provider.clone());
            }
        }
    }
    match found {
        Some(provider) => {
            req.extensions_mut().insert(Provider(provider));
            next.run(req).await
        }
        None => {
            warn!(
                "integration API: refused {} {} (bad or missing token)",
                req.method(),
                req.uri().path()
            );
            IntError::new(StatusCode::UNAUTHORIZED, "unauthorized", "Bad token").into_response()
        }
    }
}

type Body<T> = Result<Json<T>, JsonRejection>;

fn body<T>(b: Body<T>) -> Result<T, IntError> {
    b.map(|Json(v)| v)
        .map_err(|e| IntError::new(e.status(), "invalid", e.body_text()))
}

fn reply(r: Result<serde_json::Value, IntError>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => e.into_response(),
    }
}

// --- sidecar bookkeeping ---------------------------------------------------

async fn config(
    State(s): State<ApiState>,
    Extension(Provider(p)): Extension<Provider>,
) -> Response {
    let settings = s.hub.store().read(|d| d.settings.for_provider(&p).cloned());
    Json(serde_json::json!({
        "provider": p,
        "version": s.hub.settings_version(),
        "settings": settings,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct HeartbeatBody {
    #[serde(default)]
    bot_name: String,
}

async fn heartbeat(
    State(s): State<ApiState>,
    Extension(Provider(p)): Extension<Provider>,
    b: Body<HeartbeatBody>,
) -> Response {
    let b = match body(b) {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    s.hub.heartbeat(&p, super::clean_display_name(&b.bot_name));
    Json(serde_json::json!({ "ok": true, "now": now_ms() })).into_response()
}

// --- user actions ----------------------------------------------------------

#[derive(Deserialize)]
struct ActorOnly {
    actor: Actor,
}

#[derive(Deserialize)]
struct LinkBody {
    actor: Actor,
    #[serde(default)]
    username: String,
    #[serde(default)]
    code: String,
}

macro_rules! action {
    ($name:ident, $body:ty, |$s:ident, $p:ident, $b:ident| $call:expr) => {
        async fn $name(
            State($s): State<ApiState>,
            Extension(Provider($p)): Extension<Provider>,
            b: Body<$body>,
        ) -> Response {
            let $b = match body(b) {
                Ok(b) => b,
                Err(e) => return e.into_response(),
            };
            reply($call.await)
        }
    };
}

action!(link, LinkBody, |s, p, b| s.hub.link(
    &p,
    b.actor,
    &b.username,
    &b.code
));
action!(unlink, ActorOnly, |s, p, b| s.hub.unlink(&p, b.actor));
action!(me, ActorOnly, |s, p, b| s.hub.whoami(&p, b.actor));
async fn not_found() -> Response {
    IntError::new(StatusCode::NOT_FOUND, "invalid", "No such endpoint").into_response()
}

pub fn build_router(state: ApiState) -> Router {
    let v1 = Router::new()
        .route("/config", get(config))
        .route("/heartbeat", post(heartbeat))
        .route("/link", post(link))
        .route("/unlink", post(unlink))
        .route("/me", post(me));
    Router::new()
        .nest("/v1", v1)
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

pub async fn serve(
    state: ApiState,
    addr: SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) {
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Integration API disabled: failed to bind {}: {}", addr, e);
            return;
        }
    };
    info!("Integration API (chat bots) listening on http://{}", addr);
    if let Err(e) = axum::serve(listener, build_router(state))
        .with_graceful_shutdown(shutdown)
        .await
    {
        error!("Integration API error: {}", e);
    }
}
