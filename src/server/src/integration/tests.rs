use super::store::{self, ChatSettings, CODE_FAILS_BEFORE_FREEZE};
use super::*;
use crate::jellyfin::api::JfUserPolicy;
use crate::jellyfin::JellyfinConfig;
use crate::test_helpers;
use axum::body::Body;
use http_body_util::BodyExt;
use std::path::PathBuf;
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef-test";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CAROL: &str = "cccccccccccccccccccccccccccccccc";
const DAVE: &str = "dddddddddddddddddddddddddddddddd";

fn user(id: &str, name: &str, admin: bool, disabled: bool) -> JfUser {
    JfUser {
        id: id.into(),
        name: name.into(),
        policy: Some(JfUserPolicy {
            is_administrator: admin,
            is_disabled: disabled,
        }),
    }
}

struct Fixture {
    hub: Integration,
    bridges: Bridges,
    clients: crate::types::Clients,
    rooms: crate::types::Rooms,
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn fixture() -> Fixture {
    let dir = store::temp_dir();
    let cfg = config::IntegrationConfig {
        data_dir: dir.clone(),
        addr: "127.0.0.1:0".parse().unwrap(),
        tokens: vec![("discord".into(), TOKEN.into())],
    };
    let clients = test_helpers::create_clients();
    let rooms = test_helpers::create_rooms();
    let bridges = Bridges::start(
        &JellyfinConfig {
            url: "http://127.0.0.1:9".into(),
            api_key: "k".into(),
            poll_interval_ms: 60_000,
        },
        clients.clone(),
        rooms.clone(),
    )
    .unwrap();
    let hub = Integration::new(&cfg, bridges.clone()).unwrap();
    hub.set_users(vec![
        user(ALICE, "Alice", false, false),
        user(BOB, "Bob", false, false),
        user(CAROL, "Carol", true, false),
        user(DAVE, "Dave", false, true),
    ])
    .await;
    configure(&hub, |_| {});
    Fixture {
        hub,
        bridges,
        clients,
        rooms,
        dir,
    }
}

fn configure(hub: &Integration, f: impl FnOnce(&mut ChatSettings)) {
    hub.store().update(|d| {
        let mut s = ChatSettings {
            enabled: true,
            guild_id: "1".into(),
            ..Default::default()
        };
        f(&mut s);
        d.settings.discord = s;
    });
}

fn actor(id: &str) -> Actor {
    Actor {
        id: id.into(),
        name: format!("acct{}", id),
        guild_id: "1".into(),
        channel_id: "10".into(),
        roles: vec![],
    }
}

async fn link(hub: &Integration, account: &str, name: &str, user_id: &str) {
    let code = hub.assign_code(user_id).await.unwrap();
    hub.link("discord", actor(account), name, &code)
        .await
        .unwrap();
}

fn reason(r: IntResult) -> &'static str {
    match r {
        Ok(v) => panic!("expected an error, got {}", v),
        Err(e) => e.reason,
    }
}

fn wrong_code(code: &str) -> String {
    format!("{:04}", (code.parse::<u32>().unwrap() + 1) % 10_000)
}

// --- linking ---------------------------------------------------------------

#[tokio::test]
async fn link_with_username_and_code() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    assert_eq!(code.len(), 4);

    let bad = f
        .hub
        .link("discord", actor("100"), "alice", &wrong_code(&code))
        .await;
    assert_eq!(reason(bad), "bad_credentials");
    // An unknown name gives the same answer as a wrong code.
    let unknown = f.hub.link("discord", actor("100"), "mallory", &code).await;
    assert_eq!(reason(unknown), "bad_credentials");
    // The code is tied to its user.
    let other = f.hub.link("discord", actor("100"), "bob", &code).await;
    assert_eq!(reason(other), "bad_credentials");

    // Case-insensitive name, spaces in the code are fine.
    let spaced = format!("{} {}", &code[..2], &code[2..]);
    let ok = f
        .hub
        .link("discord", actor("100"), "  ALICE ", &spaced)
        .await
        .unwrap();
    assert_eq!(ok["user_name"], "Alice");
    let me = f.hub.whoami("discord", actor("100")).await.unwrap();
    assert_eq!(me["user_id"], ALICE);
    assert_eq!(me["is_admin"], false);

    // Reusable: the same account may link again with the same code.
    assert!(f
        .hub
        .link("discord", actor("100"), "alice", &code)
        .await
        .is_ok());
    // The audit log names the failures.
    assert!(f.hub.audit_log().iter().any(|e| e.kind == "link_failed"));
}

#[tokio::test]
async fn another_account_cannot_take_over_a_link() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    f.hub
        .link("discord", actor("100"), "alice", &code)
        .await
        .unwrap();
    let r = f.hub.link("discord", actor("200"), "alice", &code).await;
    assert_eq!(reason(r), "already_linked_elsewhere");
    assert_eq!(
        reason(f.hub.whoami("discord", actor("200")).await),
        "not_linked"
    );
    // After the admin reassigns, the old link is gone and a new one works.
    let code = f.hub.assign_code(ALICE).await.unwrap();
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "not_linked"
    );
    f.hub
        .link("discord", actor("200"), "alice", &code)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_account_is_locked_out_after_wrong_attempts() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    for _ in 0..LINK_FAILS_PER_ACCOUNT {
        let r = f.hub.link("discord", actor("100"), "nobody", "0000").await;
        assert_eq!(reason(r), "bad_credentials");
    }
    // Even the right code waits now.
    let r = f.hub.link("discord", actor("100"), "alice", &code).await;
    let e = r.unwrap_err();
    assert_eq!(e.reason, "locked_out");
    assert!(e.retry_after_ms.unwrap() > 0);
    // Other accounts aren't affected.
    assert!(f
        .hub
        .link("discord", actor("101"), "alice", &code)
        .await
        .is_ok());
}

#[tokio::test]
async fn a_code_freezes_after_wrong_guesses_from_many_accounts() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    let wrong = wrong_code(&code);
    // Spread over accounts, so no account lockout kicks in.
    let mut n = 0;
    'outer: for account in 300..400 {
        for _ in 0..3 {
            let _ = f
                .hub
                .link("discord", actor(&account.to_string()), "alice", &wrong)
                .await;
            n += 1;
            if n == CODE_FAILS_BEFORE_FREEZE {
                break 'outer;
            }
        }
    }
    let r = f.hub.link("discord", actor("999"), "alice", &code).await;
    assert_eq!(
        reason(r),
        "bad_credentials",
        "frozen: even the right code fails"
    );
    assert!(f.hub.audit_log().iter().any(|e| e.kind == "code_frozen"));
    assert!(f.hub.store().read(|d| d.users[ALICE].frozen));

    let code = f.hub.assign_code(ALICE).await.unwrap();
    assert!(f
        .hub
        .link("discord", actor("999"), "alice", &code)
        .await
        .is_ok());
}

#[tokio::test]
async fn disabled_users_cannot_link_or_act() {
    let f = fixture().await;
    let code = f.hub.assign_code(DAVE).await.unwrap();
    let r = f.hub.link("discord", actor("100"), "dave", &code).await;
    assert_eq!(reason(r), "bad_credentials");

    // Linked first, disabled later: refused from then on.
    link(&f.hub, "101", "bob", BOB).await;
    f.hub.set_users(vec![user(BOB, "Bob", false, true)]).await;
    assert_eq!(
        reason(f.hub.whoami("discord", actor("101")).await),
        "account_disabled"
    );
}

#[tokio::test]
async fn unlink_removes_the_link() {
    let f = fixture().await;
    link(&f.hub, "100", "alice", ALICE).await;
    f.hub.unlink("discord", actor("100")).await.unwrap();
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "not_linked"
    );
    assert_eq!(
        reason(f.hub.unlink("discord", actor("100")).await),
        "not_linked"
    );
}

// --- platform gates --------------------------------------------------------

#[tokio::test]
async fn requests_must_come_from_the_configured_place() {
    let f = fixture().await;
    link(&f.hub, "100", "alice", ALICE).await;

    let mut a = actor("100");
    a.guild_id = "2".into();
    assert_eq!(reason(f.hub.whoami("discord", a).await), "wrong_guild");

    configure(&f.hub, |s| s.channel_ids = vec!["11".into()]);
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "channel_not_allowed"
    );

    configure(&f.hub, |s| s.required_role_id = "77".into());
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "missing_role"
    );
    let mut a = actor("100");
    a.roles = vec!["77".into()];
    assert!(f.hub.whoami("discord", a).await.is_ok());

    configure(&f.hub, |s| s.enabled = false);
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "disabled"
    );
    assert_eq!(
        reason(f.hub.whoami("telegram", actor("100")).await),
        "not_configured"
    );
    assert_eq!(
        reason(f.hub.whoami("discord", actor("12ab")).await),
        "invalid"
    );
}

#[tokio::test]
async fn requests_are_rate_limited_per_account() {
    let f = fixture().await;
    for _ in 0..REQUESTS_PER_ACTOR {
        assert_ne!(
            reason(f.hub.whoami("discord", actor("100")).await),
            "rate_limited"
        );
    }
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "rate_limited"
    );
    assert_ne!(
        reason(f.hub.whoami("discord", actor("101")).await),
        "rate_limited"
    );
}

// --- HTTP ------------------------------------------------------------------

async fn http(
    f: &Fixture,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let state = api::ApiState::new(f.hub.clone(), &[("discord".into(), TOKEN.into())]);
    let mut req = axum::http::Request::builder().method(method).uri(path);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {}", t));
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = api::build_router(state).oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn the_api_needs_the_token() {
    let f = fixture().await;
    for (m, p) in [
        ("GET", "/v1/config"),
        ("GET", "/v1/rooms"),
        ("POST", "/v1/me"),
    ] {
        let (s, j) = http(&f, m, p, None, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{} {}", m, p);
        assert_eq!(j["reason"], "unauthorized");
        let (s, _) = http(&f, m, p, Some("wrong-token"), None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, j) = http(&f, "GET", "/v1/config", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["provider"], "discord");
    assert_eq!(j["settings"]["guild_id"], "1");
    let (s, _) = http(&f, "GET", "/v1/nope", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_api_links_and_reports_reasons() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    let a = serde_json::json!({ "id": "100", "name": "al", "guild_id": "1", "channel_id": "10" });
    let (s, j) = http(
        &f,
        "POST",
        "/v1/link",
        Some(TOKEN),
        Some(serde_json::json!({ "actor": a, "username": "alice", "code": code })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);

    let (s, j) = http(
        &f,
        "POST",
        "/v1/me",
        Some(TOKEN),
        Some(serde_json::json!({ "actor": a })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    assert_eq!(j["user_name"], "Alice");

    let (s, j) = http(
        &f,
        "POST",
        "/v1/heartbeat",
        Some(TOKEN),
        Some(serde_json::json!({ "bot_name": "Bot" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);

    let (s, j) = http(
        &f,
        "POST",
        "/v1/me",
        Some(TOKEN),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(j["reason"], "invalid");

    let big = "x".repeat(20_000);
    let (s, _) = http(
        &f,
        "POST",
        "/v1/link",
        Some(TOKEN),
        Some(serde_json::json!({ "actor": a, "username": big, "code": "1" })),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

// --- admin -----------------------------------------------------------------

async fn admin_call(
    router: &axum::Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
) -> (
    axum::http::StatusCode,
    serde_json::Value,
    axum::http::HeaderMap,
) {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "admin.local")
        .header(crate::admin::CSRF_HEADER, "1");
    if let Some(c) = cookie {
        req = req.header(
            "cookie",
            format!("{}={}", crate::admin::auth::COOKIE_NAME, c),
        );
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        headers,
    )
}

#[tokio::test]
async fn admins_manage_codes_and_settings_in_the_panel() {
    let f = fixture().await;
    let state = crate::admin::AdminState::new(
        f.clients.clone(),
        f.rooms.clone(),
        crate::admin::config::AdminConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            username: "admin".into(),
            password: "pw-pw-pw-pw-pw".into(),
            session_ttl_ms: 60_000,
            cookie_secure: false,
            empty_group_ttl_ms: 60_000,
            trust_forwarded_for: false,
        },
        false,
        crate::admin::JellyfinStatus::Enabled(f.bridges.clone()),
        IntegrationStatus::Enabled(f.hub.clone()),
    );
    let router = crate::admin::build_router(state);
    let (_, _, h) = admin_call(
        &router,
        "POST",
        "/api/login",
        None,
        Some(serde_json::json!({ "username": "admin", "password": "pw-pw-pw-pw-pw" })),
    )
    .await;
    let cookie = h["set-cookie"].to_str().unwrap();
    let token = cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1
        .to_string();
    let t = Some(token.as_str());

    // Not without a session.
    let (s, _, _) = admin_call(&router, "GET", "/api/users", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, j, _) = admin_call(
        &router,
        "POST",
        &format!("/api/users/{}/code", ALICE),
        t,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    let code = j["code"].as_str().unwrap().to_string();
    f.hub
        .link("discord", actor("100"), "alice", &code)
        .await
        .unwrap();

    let (_, j, _) = admin_call(&router, "GET", "/api/users", t, None).await;
    let alice = j["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == ALICE)
        .unwrap()
        .clone();
    assert!(alice["code"].is_object());
    assert_eq!(alice["links"]["discord"]["external_id"], "100");
    assert!(
        !j.to_string().contains(&format!("\"{}\"", code)),
        "codes are never listed"
    );

    let (s, _, _) = admin_call(
        &router,
        "DELETE",
        &format!("/api/users/{}/links/discord", ALICE),
        t,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        reason(f.hub.whoami("discord", actor("100")).await),
        "not_linked"
    );
    let (s, _, _) = admin_call(
        &router,
        "DELETE",
        &format!("/api/users/{}/code", ALICE),
        t,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = admin_call(
        &router,
        "DELETE",
        &format!("/api/users/{}/code", ALICE),
        t,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = admin_call(
        &router,
        "POST",
        "/api/users/ffffffffffffffffffffffffffffffff/code",
        t,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Settings are validated and saved.
    let (s, j, _) = admin_call(
        &router,
        "PUT",
        "/api/integrations/discord",
        t,
        Some(serde_json::json!({ "enabled": true, "guild_id": "" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{}", j);
    let before = f.hub.settings_version();
    let (s, _, _) = admin_call(
        &router,
        "PUT",
        "/api/integrations/discord",
        t,
        Some(serde_json::json!({ "enabled": true, "guild_id": "42", "channel_ids": ["7"] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(f.hub.settings_version() > before);
    let (_, j, _) = admin_call(&router, "GET", "/api/integrations", t, None).await;
    assert_eq!(j["providers"][0]["settings"]["guild_id"], "42");
    let (s, _, _) = admin_call(
        &router,
        "PUT",
        "/api/integrations/telegram",
        t,
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, j, _) = admin_call(&router, "GET", "/api/audit", t, None).await;
    let kinds: Vec<_> = j["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    for k in ["code_assign", "link", "unlink", "code_revoke", "settings"] {
        assert!(kinds.contains(&k.to_string()), "{} in {:?}", k, kinds);
    }
}

#[tokio::test]
async fn admin_status_shows_settings_and_sidecar() {
    let f = fixture().await;
    f.hub.heartbeat("discord", "JWP Bot".into());
    let s = f.hub.status_json();
    assert_eq!(s["providers"][0]["provider"], "discord");
    assert_eq!(s["providers"][0]["token_set"], true);
    assert_eq!(s["providers"][0]["sidecar"]["online"], true);
    assert_eq!(s["providers"][0]["settings"]["guild_id"], "1");
}
