use crate::auth::JwtConfig;
use crate::types::{Clients, Rooms};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

/// Everything the route handlers need, cloned per request (all fields are
/// `Arc`s, so this is cheap).
#[derive(Clone)]
pub struct AppState {
    pub clients: Clients,
    pub rooms: Rooms,
    pub jwt_config: Arc<JwtConfig>,
    pub allowed_origins: Arc<Vec<String>>,
}

pub fn get_allowed_origins() -> Vec<String> {
    std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:8096,https://localhost:8096".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_origin_allowed(origin: &str, allowed: &Arc<Vec<String>>) -> bool {
    if allowed.iter().any(|o| o == "*") {
        warn!("SECURITY: Wildcard origin (*) configured - ALL origins allowed. This disables CORS protection!");
        return true;
    }
    allowed.iter().any(|o| o == origin)
}

/// Rejects websocket upgrades whose `Origin` is not in the allow-list.
///
/// A *missing* `Origin` is allowed through deliberately: non-browser clients
/// (notably the Jellyfin plugin's `ClientWebSocket` host/follower bridges)
/// never send one, and browsers always do — so this still blocks the
/// cross-site-scripted-browser case it exists for. Header values that aren't
/// valid ASCII are treated as absent, matching the previous
/// `warp::header::optional::<String>` behaviour.
async fn origin_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());

    match origin {
        Some(o) if !is_origin_allowed(o, &state.allowed_origins) => {
            warn!("Rejected connection from origin: {}", o);
            (StatusCode::FORBIDDEN, "Origin not allowed").into_response()
        }
        _ => next.run(req).await,
    }
}

fn build_cors(allowed_origins: &Arc<Vec<String>>) -> CorsLayer {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE]);

    if allowed_origins.iter().any(|o| o == "*") {
        cors.allow_origin(Any)
    } else {
        let origins: Vec<HeaderValue> = allowed_origins
            .iter()
            .filter_map(|o| HeaderValue::from_str(o).ok())
            .collect();
        cors.allow_origin(origins)
    }
}

async fn ws_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let client_id = params.get("client_id").cloned();
    ws.on_upgrade(move |socket| {
        crate::ws::client_connection(
            socket,
            state.clients,
            state.rooms,
            state.jwt_config,
            client_id,
        )
    })
}

async fn health_handler(State(state): State<AppState>) -> Response {
    Json(serde_json::json!({
        "status": "ok",
        "auth_enabled": state.jwt_config.enabled
    }))
    .into_response()
}

pub fn build_router(
    clients: Clients,
    rooms: Rooms,
    jwt_config: Arc<JwtConfig>,
    allowed_origins: Arc<Vec<String>>,
) -> Router {
    let state = AppState {
        clients,
        rooms,
        jwt_config,
        allowed_origins,
    };
    let cors = build_cors(&state.allowed_origins);

    Router::new()
        .route(
            "/ws",
            get(ws_handler)
                .route_layer(middleware::from_fn_with_state(state.clone(), origin_guard)),
        )
        // `/health` is intentionally *not* origin-gated: container health
        // checks and uptime probes send no `Origin` and must always succeed.
        .route("/health", get(health_handler).route_layer(cors))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use axum::body::Body;
    use futures::StreamExt;
    use http_body_util::BodyExt;
    use std::net::SocketAddr;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tower::ServiceExt;

    fn test_router(allowed: Vec<&str>) -> Router {
        build_router(
            test_helpers::create_clients(),
            test_helpers::create_rooms(),
            Arc::new(JwtConfig {
                secret: String::new(),
                audience: "test".to_string(),
                issuer: "test".to_string(),
                enabled: false,
            }),
            Arc::new(allowed.into_iter().map(String::from).collect()),
        )
    }

    /// Binds the real router to an ephemeral port so the websocket tests go
    /// through an actual HTTP/1.1 upgrade instead of a synthesized request
    /// (`ServiceExt::oneshot` can't produce one — there is no `OnUpgrade`
    /// extension, so `WebSocketUpgrade` always rejects with 426).
    async fn spawn_server(allowed: Vec<&str>) -> SocketAddr {
        let app = test_router(allowed);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        addr
    }

    /// Connects like a real client would. `origin` is `None` for the
    /// non-browser case (the Jellyfin plugin's bridges).
    async fn connect_ws(
        addr: SocketAddr,
        query: &str,
        origin: Option<&str>,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::Error,
    > {
        let mut request = format!("ws://{}/ws{}", addr, query)
            .into_client_request()
            .unwrap();
        if let Some(o) = origin {
            request
                .headers_mut()
                .insert(header::ORIGIN, HeaderValue::from_str(o).unwrap());
        }
        tokio_tungstenite::connect_async(request)
            .await
            .map(|(stream, _)| stream)
    }

    #[test]
    fn is_origin_allowed_exact_match() {
        let allowed = Arc::new(vec!["https://example.com".to_string()]);
        assert!(is_origin_allowed("https://example.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_no_match() {
        let allowed = Arc::new(vec!["https://example.com".to_string()]);
        assert!(!is_origin_allowed("https://other.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_wildcard() {
        let allowed = Arc::new(vec!["*".to_string()]);
        assert!(is_origin_allowed("https://anything.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_empty_list() {
        let allowed = Arc::new(vec![]);
        assert!(!is_origin_allowed("https://example.com", &allowed));
    }

    #[test]
    fn is_origin_allowed_multiple_origins() {
        let allowed = Arc::new(vec![
            "https://a.com".to_string(),
            "https://b.com".to_string(),
        ]);
        assert!(is_origin_allowed("https://b.com", &allowed));
        assert!(!is_origin_allowed("https://c.com", &allowed));
    }

    #[test]
    fn get_allowed_origins_default() {
        let result = get_allowed_origins();
        assert!(!result.is_empty());
        for origin in &result {
            assert_eq!(origin, origin.trim());
            assert!(!origin.is_empty());
        }
    }

    #[tokio::test]
    async fn health_returns_ok_json() {
        let response = test_router(vec!["https://example.com"])
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("status").unwrap(), "ok");
        assert_eq!(json.get("auth_enabled").unwrap(), &serde_json::json!(false));
    }

    /// Container health checks send no `Origin`; the endpoint must not be
    /// gated on one.
    #[tokio::test]
    async fn health_is_not_origin_gated() {
        let response = test_router(vec!["https://example.com"])
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "https://evil.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn ws_upgrade_allowed_origin() {
        let addr = spawn_server(vec!["https://example.com"]).await;
        assert!(connect_ws(addr, "", Some("https://example.com"))
            .await
            .is_ok());
    }

    /// The Jellyfin plugin's host/follower bridges use .NET's
    /// `ClientWebSocket`, which sends no `Origin` header at all. If this ever
    /// starts rejecting anonymous-origin upgrades the bridges break silently
    /// in production — nothing else in the repo covers that path.
    #[tokio::test]
    async fn ws_upgrade_without_origin_is_allowed() {
        let addr = spawn_server(vec!["https://example.com"]).await;
        assert!(connect_ws(addr, "", None).await.is_ok());
    }

    #[tokio::test]
    async fn ws_upgrade_disallowed_origin_is_rejected() {
        let addr = spawn_server(vec!["https://example.com"]).await;
        let err = connect_ws(addr, "", Some("https://evil.com"))
            .await
            .expect_err("disallowed origin must not get a socket");

        match err {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                assert_eq!(response.status(), StatusCode::FORBIDDEN);
            }
            other => panic!("expected an HTTP rejection, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn ws_upgrade_wildcard_allows_any_origin() {
        let addr = spawn_server(vec!["*"]).await;
        assert!(connect_ws(addr, "", Some("https://anything.com"))
            .await
            .is_ok());
    }

    /// A connected client is greeted with `client_hello` carrying the
    /// `client_id` it asked for, proving the query extractor and the upgraded
    /// socket's outbound path both survived the framework swap.
    #[tokio::test]
    async fn ws_connection_echoes_requested_client_id() {
        let addr = spawn_server(vec!["https://example.com"]).await;
        let client_id = "550e8400-e29b-41d4-a716-446655440000";
        let mut socket = connect_ws(
            addr,
            &format!("?client_id={}", client_id),
            Some("https://example.com"),
        )
        .await
        .unwrap();

        let frame = socket.next().await.unwrap().unwrap();
        let text = frame.into_text().unwrap();
        let msg: crate::types::WsMessage = serde_json::from_str(&text).unwrap();

        assert_eq!(msg.msg_type, "client_hello");
        assert_eq!(msg.client.as_deref(), Some(client_id));
    }
}
