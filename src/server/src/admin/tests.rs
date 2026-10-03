use super::*;
use crate::test_helpers;
use crate::types::ClientReceiver;
use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn cfg() -> AdminConfig {
    AdminConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        username: "admin".into(),
        password: "s3cret-admin-pw".into(),
        session_ttl_ms: 60_000,
        cookie_secure: false,
        empty_group_ttl_ms: 60_000,
        trust_forwarded_for: false,
    }
}

fn state() -> AdminState {
    AdminState::new(
        test_helpers::create_clients(),
        test_helpers::create_rooms(),
        cfg(),
        false,
    )
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    json: serde_json::Value,
}

async fn call(
    state: &AdminState,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
) -> Resp {
    call_with(state, method, path, cookie, body, true, None).await
}

async fn call_with(
    state: &AdminState,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
    csrf: bool,
    origin: Option<&str>,
) -> Resp {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "admin.local:3001");
    if csrf {
        req = req.header(CSRF_HEADER, "1");
    }
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, format!("{}={}", auth::COOKIE_NAME, c));
    }
    if let Some(o) = origin {
        req = req.header(header::ORIGIN, o);
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = build_router(state.clone()).oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    Resp {
        status,
        headers,
        json,
    }
}

async fn login(state: &AdminState) -> String {
    let r = call(
        state,
        "POST",
        "/api/login",
        None,
        Some(serde_json::json!({ "username": "admin", "password": "s3cret-admin-pw" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let cookie = r.headers[header::SET_COOKIE].to_str().unwrap().to_string();
    assert!(cookie.contains("HttpOnly"));
    cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1
        .to_string()
}

async fn add_client(state: &AdminState, id: &str, authenticated: bool) -> ClientReceiver {
    let (c, rx) = test_helpers::create_client_with_rx(id, id, authenticated);
    state.clients.write().await.insert(id.to_string(), c);
    rx
}

fn drain_types(rx: &mut ClientReceiver) -> Vec<(String, serde_json::Value)> {
    std::iter::from_fn(|| test_helpers::recv_msg(rx))
        .map(|m| (m.msg_type, m.payload.unwrap_or_default()))
        .collect()
}

#[tokio::test]
async fn ui_is_served_with_security_headers() {
    let s = state();
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let res = build_router(s).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let h = res.headers();
    assert!(h[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));
    assert_eq!(h[header::X_FRAME_OPTIONS], "DENY");
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
    assert!(h[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
}

#[tokio::test]
async fn api_requires_login() {
    let s = state();
    for (m, p) in [
        ("GET", "/api/overview"),
        ("GET", "/api/me"),
        ("POST", "/api/rooms"),
        ("DELETE", "/api/rooms/x"),
    ] {
        let r = call(&s, m, p, None, Some(serde_json::json!({ "name": "x" }))).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{} {}", m, p);
    }
    let r = call(&s, "GET", "/api/overview", Some("forged"), None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_logout_cycle() {
    let s = state();
    let bad = call(
        &s,
        "POST",
        "/api/login",
        None,
        Some(serde_json::json!({ "username": "admin", "password": "nope" })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    assert!(bad.headers.get(header::SET_COOKIE).is_none());

    let token = login(&s).await;
    let me = call(&s, "GET", "/api/me", Some(&token), None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json["username"], "admin");

    let out = call(&s, "POST", "/api/logout", Some(&token), None).await;
    assert_eq!(out.status, StatusCode::OK);
    let me = call(&s, "GET", "/api/me", Some(&token), None).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_is_throttled_even_for_the_right_password() {
    let s = state();
    for _ in 0..auth::LOGIN_FAILS_PER_IP {
        call(
            &s,
            "POST",
            "/api/login",
            None,
            Some(serde_json::json!({ "username": "admin", "password": "guess" })),
        )
        .await;
    }
    let r = call(
        &s,
        "POST",
        "/api/login",
        None,
        Some(serde_json::json!({ "username": "admin", "password": "s3cret-admin-pw" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.headers.contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn mutations_need_the_csrf_header_and_a_matching_origin() {
    let s = state();
    let body = serde_json::json!({ "username": "admin", "password": "s3cret-admin-pw" });
    let r = call_with(
        &s,
        "POST",
        "/api/login",
        None,
        Some(body.clone()),
        false,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let r = call_with(
        &s,
        "POST",
        "/api/login",
        None,
        Some(body.clone()),
        true,
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let r = call_with(
        &s,
        "POST",
        "/api/login",
        None,
        Some(body),
        true,
        Some("http://admin.local:3001"),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[test]
fn csrf_accepts_proxied_origin() {
    let mut h = HeaderMap::new();
    h.insert(CSRF_HEADER, HeaderValue::from_static("1"));
    h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:3001"));
    h.insert(
        "x-forwarded-host",
        HeaderValue::from_static("jwp.example.com"),
    );
    h.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://jwp.example.com"),
    );
    assert!(csrf_ok(&Method::POST, &h));
    assert!(csrf_ok(&Method::GET, &HeaderMap::new()));
    assert!(!csrf_ok(&Method::DELETE, &HeaderMap::new()));
}

#[test]
fn forwarded_ip_uses_the_last_hop() {
    let mut h = HeaderMap::new();
    h.insert(
        "x-forwarded-for",
        HeaderValue::from_static("6.6.6.6, 10.1.2.3"),
    );
    assert_eq!(forwarded_ip(&h), Some("10.1.2.3".parse().unwrap()));
    assert_eq!(forwarded_ip(&HeaderMap::new()), None);
}

#[tokio::test]
async fn group_lifecycle_through_the_api() {
    let s = state();
    let token = login(&s).await;
    let t = Some(token.as_str());
    let mut rx_a = add_client(&s, "client-a", true).await;
    let mut rx_b = add_client(&s, "client-b", true).await;
    let _rx_anon = add_client(&s, "client-anon", false).await;

    // Create a password-protected group.
    let r = call(
        &s,
        "POST",
        "/api/rooms",
        t,
        Some(serde_json::json!({ "name": "Movie night", "password": "pw" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let room = r.json["id"].as_str().unwrap().to_string();

    let ov = call(&s, "GET", "/api/overview", t, None).await.json;
    assert_eq!(ov["rooms"][0]["name"], "Movie night");
    assert_eq!(ov["rooms"][0]["admin_created"], true);
    assert_eq!(ov["rooms"][0]["has_password"], true);
    assert!(ov["rooms"][0]["host_id"].is_null());
    assert_eq!(ov["unassigned"].as_array().unwrap().len(), 3);

    // Add two clients without the password; the first becomes host.
    let members = format!("/api/rooms/{}/members", room);
    for id in ["client-a", "client-b"] {
        let r = call(
            &s,
            "POST",
            &members,
            t,
            Some(serde_json::json!({ "client_id": id })),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json);
    }
    let (_, payload) = drain_types(&mut rx_a)
        .into_iter()
        .find(|(ty, _)| ty == "room_state")
        .expect("the added client is sent the room state");
    assert_eq!(payload["admin_moved"], true);
    assert_eq!(payload["host_id"], "client-a");
    drain_types(&mut rx_b);

    // A connection that never signed in can't be added.
    let r = call(
        &s,
        "POST",
        &members,
        t,
        Some(serde_json::json!({ "client_id": "client-anon" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    // Hand the host role over.
    let r = call(
        &s,
        "PUT",
        &format!("/api/rooms/{}/host", room),
        t,
        Some(serde_json::json!({ "member": "client-b" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(drain_types(&mut rx_a)
        .iter()
        .any(|(ty, p)| ty == "host_changed" && p["host_id"] == "client-b"));
    let ov = call(&s, "GET", "/api/overview", t, None).await.json;
    let m = &ov["rooms"][0]["members"];
    assert_eq!(m[0]["is_host"], false);
    assert_eq!(m[1]["is_host"], true);

    // Remove a member.
    let r = call(
        &s,
        "DELETE",
        &format!("/api/rooms/{}/members/client-a", room),
        t,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(drain_types(&mut rx_a)
        .iter()
        .any(|(ty, _)| ty == "room_closed"));

    // Remove the password, then close the room.
    let r = call(
        &s,
        "PATCH",
        &format!("/api/rooms/{}", room),
        t,
        Some(serde_json::json!({ "password": null, "name": "Renamed" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    {
        let lr = s.rooms.read().await;
        assert!(lr[&room].password_hash.is_none());
        assert_eq!(lr[&room].name, "Renamed");
    }
    let r = call(&s, "DELETE", &format!("/api/rooms/{}", room), t, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(s.rooms.read().await.is_empty());
    assert!(s.clients.read().await["client-b"].room_id.is_none());

    let r = call(&s, "DELETE", &format!("/api/rooms/{}", room), t, None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_rejects_blank_names() {
    let s = state();
    let token = login(&s).await;
    let r = call(
        &s,
        "POST",
        "/api/rooms",
        Some(&token),
        Some(serde_json::json!({ "name": "   " })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}
