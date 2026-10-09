//! The integration API: what sidecars (the Discord bot) call. Its own
//! listener (`INTEGRATION_HOST:INTEGRATION_PORT`), never published; every
//! request needs `Authorization: Bearer <token>`, and the token says which
//! platform is calling. Errors are `{error, reason, retry_after_ms?}`.

use super::{Actor, IntError, Integration, KickTarget, RoomUpdate};
use crate::integration::DeviceRole;
use crate::password::ct_eq;
use crate::utils::now_ms;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use log::{error, info, warn};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// A long poll waits this long for a change before answering anyway.
const LONG_POLL: Duration = Duration::from_secs(25);
/// After a change, wait this long so a burst of changes is sent once.
const COALESCE: Duration = Duration::from_millis(250);
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

#[derive(Deserialize)]
struct Since {
    #[serde(default)]
    since: Option<u64>,
}

/// The platform's rooms. With `?since=<version>`, waits (up to 25 s) until
/// something changed after that version.
async fn rooms(
    State(s): State<ApiState>,
    Extension(Provider(p)): Extension<Provider>,
    Query(q): Query<Since>,
) -> Response {
    if let Some(since) = q.since {
        let mut rx = crate::events::subscribe();
        let current = *rx.borrow_and_update();
        if current == since && tokio::time::timeout(LONG_POLL, rx.changed()).await.is_ok() {
            tokio::time::sleep(COALESCE).await;
        }
    }
    let version = crate::events::version();
    let owners = s.hub.bridges().owners();
    let list = {
        let rooms = s.hub.0.rooms.read().await;
        let clients = s.hub.0.clients.read().await;
        s.hub
            .store()
            .read(|d| super::view::rooms_json(&p, &rooms, &clients, &owners, d))
    };
    Json(serde_json::json!({ "version": version, "rooms": list })).into_response()
}

#[derive(Deserialize)]
struct PanelBody {
    channel_id: String,
    message_id: String,
}

async fn panel(
    State(s): State<ApiState>,
    Extension(Provider(p)): Extension<Provider>,
    Path(id): Path<String>,
    b: Body<PanelBody>,
) -> Response {
    let b = match body(b) {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    reply(s.hub.set_panel(&p, &id, &b.channel_id, &b.message_id).await)
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

#[derive(Deserialize)]
struct CreateBody {
    actor: Actor,
    name: String,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
struct JoinBody {
    actor: Actor,
    #[serde(default)]
    password: Option<String>,
}

/// Distinguishes a missing field (`None`) from an explicit `null`.
fn double_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct UpdateBody {
    actor: Actor,
    #[serde(default)]
    name: Option<String>,
    /// Absent: keep. `null` or `""`: remove. Anything else: set.
    #[serde(default, deserialize_with = "double_option")]
    password: Option<Option<String>>,
}

#[derive(Deserialize)]
struct OwnerBody {
    actor: Actor,
    /// The new owner's chat account id.
    to: String,
}

#[derive(Deserialize)]
struct KickBody {
    actor: Actor,
    /// A room member (client id)...
    #[serde(default)]
    member: Option<String>,
    /// ...or a participant, by chat account id.
    #[serde(default)]
    user: Option<String>,
}

#[derive(Deserialize)]
struct MemberBody {
    actor: Actor,
    member: String,
}

#[derive(Deserialize)]
struct DeviceBody {
    actor: Actor,
    session_id: String,
    role: DeviceRole,
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
    ($name:ident, $body:ty, |$s:ident, $p:ident, $id:ident, $b:ident| $call:expr) => {
        async fn $name(
            State($s): State<ApiState>,
            Extension(Provider($p)): Extension<Provider>,
            Path($id): Path<String>,
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
action!(devices, ActorOnly, |s, p, b| s.hub.devices(&p, b.actor));
action!(create, CreateBody, |s, p, b| s.hub.create_room(
    &p,
    b.actor,
    &b.name,
    b.password.as_deref()
));
action!(join, JoinBody, |s, p, id, b| s.hub.join_room(
    &p,
    b.actor,
    &id,
    b.password.as_deref()
));
action!(leave, ActorOnly, |s, p, id, b| s
    .hub
    .leave_room(&p, b.actor, &id));
action!(update, UpdateBody, |s, p, id, b| s.hub.update_room(
    &p,
    b.actor,
    &id,
    RoomUpdate {
        name: b.name,
        password: b.password
    }
));
action!(close, ActorOnly, |s, p, id, b| s
    .hub
    .close_room(&p, b.actor, &id));
action!(owner, OwnerBody, |s, p, id, b| s
    .hub
    .transfer_room(&p, b.actor, &id, &b.to));
action!(host, MemberBody, |s, p, id, b| s
    .hub
    .set_host(&p, b.actor, &id, &b.member));
action!(add_device, DeviceBody, |s, p, id, b| s.hub.add_device(
    &p,
    b.actor,
    &id,
    &b.session_id,
    b.role
));
action!(remove_device, MemberBody, |s, p, id, b| s
    .hub
    .remove_device(&p, b.actor, &id, &b.member));

async fn kick(
    State(s): State<ApiState>,
    Extension(Provider(p)): Extension<Provider>,
    Path(id): Path<String>,
    b: Body<KickBody>,
) -> Response {
    let b = match body(b) {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let target = match (b.member, b.user) {
        (Some(m), None) => KickTarget::Member(m),
        (None, Some(u)) => KickTarget::User(u),
        _ => {
            return IntError::new(
                StatusCode::BAD_REQUEST,
                "invalid",
                "Give either member or user",
            )
            .into_response()
        }
    };
    reply(s.hub.kick(&p, b.actor, &id, target).await)
}

async fn not_found() -> Response {
    IntError::new(StatusCode::NOT_FOUND, "invalid", "No such endpoint").into_response()
}

pub fn build_router(state: ApiState) -> Router {
    let v1 = Router::new()
        .route("/config", get(config))
        .route("/heartbeat", post(heartbeat))
        .route("/link", post(link))
        .route("/unlink", post(unlink))
        .route("/me", post(me))
        .route("/devices", post(devices))
        .route("/rooms", get(rooms).post(create))
        .route("/rooms/{id}/panel", put(panel))
        .route("/rooms/{id}/join", post(join))
        .route("/rooms/{id}/leave", post(leave))
        .route("/rooms/{id}/update", post(update))
        .route("/rooms/{id}/close", post(close))
        .route("/rooms/{id}/owner", post(owner))
        .route("/rooms/{id}/kick", post(kick))
        .route("/rooms/{id}/host", post(host))
        .route("/rooms/{id}/devices", post(add_device))
        .route("/rooms/{id}/devices/remove", post(remove_device));
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
