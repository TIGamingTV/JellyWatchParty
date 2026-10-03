//! Admin panel: a second HTTP listener (own port) serving a small web UI and
//! a JSON API to watch and manage rooms. Kept off the public websocket port
//! so `/ws` never exposes admin routes, and so it can be firewalled or put
//! behind its own reverse-proxy rule.

mod api;
pub mod auth;
pub mod config;
mod ui;

use crate::types::{Clients, Rooms};
use crate::utils::now_ms;
use auth::AuthStore;
use axum::extract::connect_info::ConnectInfo;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use config::AdminConfig;
use log::{error, info, warn};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

/// Header every state-changing request must carry. Browsers can't add a
/// custom header cross-origin without a CORS preflight, which this server
/// never approves - so together with the `SameSite=Strict` cookie this
/// blocks cross-site request forgery.
pub const CSRF_HEADER: &str = "x-jwp-admin";

#[derive(Clone)]
pub struct AdminState {
    pub clients: Clients,
    pub rooms: Rooms,
    pub cfg: Arc<AdminConfig>,
    pub auth: Arc<Mutex<AuthStore>>,
    pub started_at: u64,
    pub jwt_enabled: bool,
}

impl AdminState {
    pub fn new(clients: Clients, rooms: Rooms, cfg: AdminConfig, jwt_enabled: bool) -> Self {
        Self {
            clients,
            rooms,
            cfg: Arc::new(cfg),
            auth: Arc::new(Mutex::new(AuthStore::default())),
            started_at: now_ms(),
            jwt_enabled,
        }
    }

    fn auth(&self) -> std::sync::MutexGuard<'_, AuthStore> {
        // A panic while holding this lock can't leave it inconsistent in a
        // way that matters (worst case: a stale session or counter).
        self.auth.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The caller's IP, for login throttling. Uses the TCP peer address, or the
/// last `X-Forwarded-For` hop when `ADMIN_TRUST_X_FORWARDED_FOR` is set (the
/// address your own proxy saw; earlier entries are client-controlled).
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AdminState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AdminState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        if state.cfg.trust_forwarded_for {
            if let Some(ip) = forwarded_ip(&parts.headers) {
                return Ok(ClientIp(ip));
            }
        }
        Ok(ClientIp(peer))
    }
}

fn forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")?
        .to_str()
        .ok()?
        .rsplit(',')
        .next()?
        .trim()
        .parse()
        .ok()
}

pub fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

async fn require_session(State(state): State<AdminState>, req: Request, next: Next) -> Response {
    let ok = auth::cookie_token(req.headers()).is_some_and(|t| state.auth().is_valid(&t, now_ms()));
    if ok {
        next.run(req).await
    } else {
        error_response(StatusCode::UNAUTHORIZED, "Login required")
    }
}

/// `host[:port]` of an `Origin` header value.
fn origin_authority(origin: &str) -> Option<&str> {
    origin.split_once("://").map(|(_, rest)| rest)
}

fn csrf_ok(method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    if headers.get(CSRF_HEADER).is_none() {
        return false;
    }
    // Belt and braces: if the browser says where the request came from, it
    // must be this panel (as reached directly or through a proxy).
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(authority) = origin_authority(origin) else {
        return false;
    };
    ["x-forwarded-host", "host"].iter().any(|h| {
        headers
            .get(*h)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .any(|v| v.trim().eq_ignore_ascii_case(authority))
            })
    })
}

async fn csrf_guard(req: Request, next: Next) -> Response {
    if csrf_ok(req.method(), req.headers()) {
        next.run(req).await
    } else {
        warn!(
            "admin: refused {} {} (missing {} header or foreign Origin)",
            req.method(),
            req.uri().path(),
            CSRF_HEADER
        );
        error_response(StatusCode::FORBIDDEN, "Cross-site request refused")
    }
}

async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
             connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

pub fn build_router(state: AdminState) -> Router {
    let private = Router::new()
        .route("/me", get(api::me))
        .route("/logout", post(api::logout))
        .route("/overview", get(api::overview))
        .route("/rooms", post(api::create_room))
        .route(
            "/rooms/{id}",
            patch(api::update_room).delete(api::delete_room),
        )
        .route("/rooms/{id}/members", post(api::add_member))
        .route("/rooms/{id}/members/{member}", delete(api::remove_member))
        .route("/rooms/{id}/host", put(api::set_host))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));
    let public = Router::new().route("/login", post(api::login));

    Router::new()
        .route("/", get(ui::index))
        .route("/app.js", get(ui::app_js))
        .route("/app.css", get(ui::app_css))
        .nest("/api", public.merge(private))
        .layer(middleware::from_fn(csrf_guard))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

/// Binds the admin listener and serves until `shutdown` resolves. A bind
/// failure is logged and leaves the main server running.
pub async fn serve(
    state: AdminState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) {
    let addr = state.cfg.addr;
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Admin panel disabled: failed to bind {}: {}", addr, e);
            return;
        }
    };
    info!("Admin panel listening on http://{}", addr);
    let app = build_router(state).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
    {
        error!("Admin panel error: {}", e);
    }
}

#[cfg(test)]
mod tests;
