use super::store::{self, ChatSettings, CODE_FAILS_BEFORE_FREEZE};
use super::*;
use crate::jellyfin::api::JfUserPolicy;
use crate::jellyfin::JellyfinConfig;
use crate::room::ops::{self as room_ops, AddOptions};
use crate::test_helpers;
use axum::body::Body;
use http_body_util::BodyExt;
use std::path::PathBuf;
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef-test";
const TG_TOKEN: &str = "fedcba9876543210fedcba9876543210-test";
const TG_GROUP: &str = "-1001234567890";
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
        tokens: vec![
            ("discord".into(), TOKEN.into()),
            ("telegram".into(), TG_TOKEN.into()),
        ],
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
    let hub = Integration::new(&cfg, bridges, clients, rooms).unwrap();
    hub.set_users(vec![
        user(ALICE, "Alice", false, false),
        user(BOB, "Bob", false, false),
        user(CAROL, "Carol", true, false),
        user(DAVE, "Dave", false, true),
    ])
    .await;
    configure(&hub, |_| {});
    configure_telegram(&hub, |_| {});
    Fixture { hub, dir }
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

fn configure_telegram(hub: &Integration, f: impl FnOnce(&mut ChatSettings)) {
    hub.store().update(|d| {
        let mut s = ChatSettings {
            enabled: true,
            guild_id: TG_GROUP.into(),
            ..Default::default()
        };
        f(&mut s);
        d.settings.telegram = s;
    });
}

/// A Telegram user asking about the configured group.
fn tg_actor(id: &str) -> Actor {
    Actor {
        id: id.into(),
        name: format!("tg{}", id),
        guild_id: TG_GROUP.into(),
        channel_id: TG_GROUP.into(),
        roles: vec![],
    }
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
        reason(f.hub.whoami("matrix", actor("100")).await),
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

// --- Telegram ----------------------------------------------------------------

#[tokio::test]
async fn one_code_links_an_account_on_each_platform() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    f.hub
        .link("discord", actor("100"), "alice", &code)
        .await
        .unwrap();
    f.hub
        .link("telegram", tg_actor("555"), "alice", &code)
        .await
        .unwrap();
    let me = f.hub.whoami("telegram", tg_actor("555")).await.unwrap();
    assert_eq!(me["user_id"], ALICE);
    // The same number on the other platform is another account.
    assert_eq!(
        reason(f.hub.whoami("telegram", tg_actor("100")).await),
        "not_linked"
    );

    // Unlinking on one platform keeps the other.
    f.hub.unlink("telegram", tg_actor("555")).await.unwrap();
    assert!(f.hub.whoami("discord", actor("100")).await.is_ok());

    // A new code disconnects every platform.
    f.hub
        .link("telegram", tg_actor("555"), "alice", &code)
        .await
        .unwrap();
    f.hub.assign_code(ALICE).await.unwrap();
    for (p, a) in [("discord", actor("100")), ("telegram", tg_actor("555"))] {
        assert_eq!(reason(f.hub.whoami(p, a).await), "not_linked", "{}", p);
    }
}

#[tokio::test]
async fn telegram_requests_must_come_from_the_group() {
    let f = fixture().await;
    let code = f.hub.assign_code(BOB).await.unwrap();
    f.hub
        .link("telegram", tg_actor("555"), "bob", &code)
        .await
        .unwrap();

    let mut a = tg_actor("555");
    a.guild_id = "-1009".into();
    assert_eq!(reason(f.hub.whoami("telegram", a).await), "wrong_guild");
    // Settings of one platform don't open the other.
    let mut a = tg_actor("555");
    a.guild_id = "1".into();
    assert_eq!(reason(f.hub.whoami("telegram", a).await), "wrong_guild");

    // Group administrators are admins only when the setting says so.
    let mut boss = tg_actor("555");
    boss.roles = vec![store::GROUP_ADMIN_ROLE.into()];
    let me = f.hub.whoami("telegram", boss.clone()).await.unwrap();
    assert_eq!(me["is_admin"], false);
    configure_telegram(&f.hub, |s| s.admin_role_id = store::GROUP_ADMIN_ROLE.into());
    let me = f.hub.whoami("telegram", boss).await.unwrap();
    assert_eq!(me["is_admin"], true);
    let me = f.hub.whoami("telegram", tg_actor("555")).await.unwrap();
    assert_eq!(me["is_admin"], false);

    configure_telegram(&f.hub, |s| s.enabled = false);
    assert_eq!(
        reason(f.hub.whoami("telegram", tg_actor("555")).await),
        "disabled"
    );
    // Discord is unaffected.
    link(&f.hub, "100", "alice", ALICE).await;
    assert!(f.hub.whoami("discord", actor("100")).await.is_ok());
}

#[tokio::test]
async fn platforms_do_not_see_each_others_rooms() {
    let f = fixture().await;
    let code = f.hub.assign_code(ALICE).await.unwrap();
    f.hub
        .link("discord", actor("100"), "alice", &code)
        .await
        .unwrap();
    f.hub
        .link("telegram", tg_actor("555"), "alice", &code)
        .await
        .unwrap();
    let room = f
        .hub
        .create_room("telegram", tg_actor("555"), "Telegram night", None)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(
        reason(f.hub.close_room("discord", actor("100"), &room).await),
        "not_found"
    );
    let me = f.hub.whoami("discord", actor("100")).await.unwrap();
    assert!(me["owns"].as_array().unwrap().is_empty());
    let me = f.hub.whoami("telegram", tg_actor("555")).await.unwrap();
    assert_eq!(me["owns"][0], room.as_str());

    let (tg, dc) = {
        let rooms = f.hub.0.rooms.read().await;
        let clients = f.hub.0.clients.read().await;
        f.hub.store().read(|d| {
            (
                view::rooms_json("telegram", &rooms, &clients, &HashMap::new(), d),
                view::rooms_json("discord", &rooms, &clients, &HashMap::new(), d),
            )
        })
    };
    assert_eq!(tg.len(), 1);
    assert_eq!(tg[0]["owner"]["external_id"], "555");
    assert!(dc.is_empty());

    // Each platform's room limits count only its own rooms.
    configure(&f.hub, |s| s.max_rooms_per_user = 1);
    configure_telegram(&f.hub, |s| s.max_rooms_per_user = 1);
    assert!(f
        .hub
        .create_room("discord", actor("100"), "Discord night", None)
        .await
        .is_ok());
}

#[tokio::test]
async fn the_telegram_token_acts_for_telegram() {
    let f = fixture().await;
    let (s, j) = http(&f, "GET", "/v1/config", Some(TG_TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["provider"], "telegram");
    assert_eq!(j["settings"]["guild_id"], TG_GROUP);

    let code = f.hub.assign_code(ALICE).await.unwrap();
    let a = serde_json::json!({ "id": "555", "name": "al", "guild_id": TG_GROUP, "channel_id": TG_GROUP });
    let (s, j) = http(
        &f,
        "POST",
        "/v1/link",
        Some(TG_TOKEN),
        Some(serde_json::json!({ "actor": a, "username": "alice", "code": code })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    let (s, j) = http(
        &f,
        "POST",
        "/v1/rooms",
        Some(TG_TOKEN),
        Some(serde_json::json!({ "actor": a, "name": "Night" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    let id = j["id"].as_str().unwrap().to_string();

    // Telegram group ids are negative.
    let panel =
        |channel: &str| Some(serde_json::json!({ "channel_id": channel, "message_id": "42" }));
    for bad in ["-", "--1", "1-2", "-12a"] {
        let (s, _) = http(
            &f,
            "PUT",
            &format!("/v1/rooms/{}/panel", id),
            Some(TG_TOKEN),
            panel(bad),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{}", bad);
    }
    let (s, j) = http(
        &f,
        "PUT",
        &format!("/v1/rooms/{}/panel", id),
        Some(TG_TOKEN),
        panel(TG_GROUP),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    let (_, j) = http(&f, "GET", "/v1/rooms?since=0", Some(TG_TOKEN), None).await;
    assert_eq!(j["rooms"][0]["panel"]["channel_id"], TG_GROUP);

    // The Discord bot neither sees nor reaches it.
    let (_, j) = http(&f, "GET", "/v1/rooms?since=0", Some(TOKEN), None).await;
    assert!(j["rooms"].as_array().unwrap().is_empty());
    let (s, _) = http(
        &f,
        "PUT",
        &format!("/v1/rooms/{}/panel", id),
        Some(TOKEN),
        Some(serde_json::json!({ "channel_id": "10", "message_id": "42" })),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// --- rooms -----------------------------------------------------------------

async fn owner_and_guest(f: &Fixture) -> String {
    link(&f.hub, "100", "alice", ALICE).await;
    link(&f.hub, "200", "bob", BOB).await;
    let r = f
        .hub
        .create_room("discord", actor("100"), "Movie night", Some("pw"))
        .await
        .unwrap();
    r["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn owner_controls_the_room_participants_do_not() {
    let f = fixture().await;
    let room = owner_and_guest(&f).await;

    // Joining needs the password.
    let r = f
        .hub
        .join_room("discord", actor("200"), &room, Some("nope"))
        .await;
    assert_eq!(reason(r), "wrong_password");
    f.hub
        .join_room("discord", actor("200"), &room, Some("pw"))
        .await
        .unwrap();

    // Bob is a participant, not the owner.
    for r in [
        f.hub.close_room("discord", actor("200"), &room).await,
        f.hub.set_host("discord", actor("200"), &room, "x").await,
        f.hub
            .update_room(
                "discord",
                actor("200"),
                &room,
                RoomUpdate {
                    name: Some("Mine".into()),
                    password: None,
                },
            )
            .await,
        f.hub
            .transfer_room("discord", actor("200"), &room, "200")
            .await,
        f.hub
            .kick(
                "discord",
                actor("200"),
                &room,
                KickTarget::User("100".into()),
            )
            .await,
    ] {
        assert_eq!(reason(r), "not_owner");
    }

    // The owner can't leave, but can hand the room over.
    assert_eq!(
        reason(f.hub.leave_room("discord", actor("100"), &room).await),
        "owner_cannot_leave"
    );
    f.hub
        .transfer_room("discord", actor("100"), &room, "200")
        .await
        .unwrap();
    f.hub
        .leave_room("discord", actor("100"), &room)
        .await
        .unwrap();
    f.hub
        .update_room(
            "discord",
            actor("200"),
            &room,
            RoomUpdate {
                name: Some("Bob's".into()),
                password: Some(None),
            },
        )
        .await
        .unwrap();
    {
        let rooms = f.hub.0.rooms.read().await;
        assert_eq!(rooms[&room].name, "Bob's");
        assert!(rooms[&room].password_hash.is_none());
        assert_eq!(rooms[&room].chat.as_ref().unwrap().owner, BOB);
    }
    f.hub
        .close_room("discord", actor("200"), &room)
        .await
        .unwrap();
    assert!(f.hub.0.rooms.read().await.is_empty());
}

#[tokio::test]
async fn wrong_room_passwords_are_throttled() {
    let f = fixture().await;
    let room = owner_and_guest(&f).await;
    for _ in 0..5 {
        let r = f
            .hub
            .join_room("discord", actor("200"), &room, Some("x"))
            .await;
        assert_eq!(reason(r), "wrong_password");
    }
    let r = f
        .hub
        .join_room("discord", actor("200"), &room, Some("pw"))
        .await;
    assert_eq!(reason(r), "too_many_attempts");
}

#[tokio::test]
async fn admins_manage_any_room() {
    let f = fixture().await;
    let room = owner_and_guest(&f).await;
    link(&f.hub, "300", "carol", CAROL).await;
    // Jellyfin admins skip the password and may close.
    f.hub
        .join_room("discord", actor("300"), &room, None)
        .await
        .unwrap();
    f.hub
        .close_room("discord", actor("300"), &room)
        .await
        .unwrap();

    // So does the configured admin role.
    let room = f
        .hub
        .create_room("discord", actor("100"), "Again", None)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    configure(&f.hub, |s| s.admin_role_id = "55".into());
    let mut bob = actor("200");
    bob.roles = vec!["55".into()];
    f.hub.close_room("discord", bob, &room).await.unwrap();
}

#[tokio::test]
async fn room_limits_and_required_passwords() {
    let f = fixture().await;
    link(&f.hub, "100", "alice", ALICE).await;
    configure(&f.hub, |s| {
        s.max_rooms_per_user = 1;
        s.require_password = true;
    });
    let r = f
        .hub
        .create_room("discord", actor("100"), "Open", None)
        .await;
    assert_eq!(reason(r), "password_required");
    f.hub
        .create_room("discord", actor("100"), "One", Some("pw"))
        .await
        .unwrap();
    let r = f
        .hub
        .create_room("discord", actor("100"), "Two", Some("pw"))
        .await;
    assert_eq!(reason(r), "room_limit");

    configure(&f.hub, |s| s.max_rooms_total = 1);
    link(&f.hub, "200", "bob", BOB).await;
    let r = f
        .hub
        .create_room("discord", actor("200"), "Bob", None)
        .await;
    assert_eq!(reason(r), "room_limit");
}

#[tokio::test]
async fn kicking_a_participant() {
    let f = fixture().await;
    let room = owner_and_guest(&f).await;
    f.hub
        .join_room("discord", actor("200"), &room, Some("pw"))
        .await
        .unwrap();
    f.hub
        .kick(
            "discord",
            actor("100"),
            &room,
            KickTarget::User("200".into()),
        )
        .await
        .unwrap();
    let r = f.hub.leave_room("discord", actor("200"), &room).await;
    assert_eq!(reason(r), "not_participant");
    let r = f
        .hub
        .kick(
            "discord",
            actor("100"),
            &room,
            KickTarget::User("100".into()),
        )
        .await;
    assert_eq!(reason(r), "is_owner");
}

#[tokio::test]
async fn devices_need_participation_and_the_right_role() {
    let f = fixture().await;
    let room = owner_and_guest(&f).await;
    let r = f
        .hub
        .add_device("discord", actor("200"), &room, "s1", DeviceRole::Receiver)
        .await;
    assert_eq!(reason(r), "not_participant");
    f.hub
        .join_room("discord", actor("200"), &room, Some("pw"))
        .await
        .unwrap();

    // A web client is host now: a participant may not take over.
    {
        let mut rooms = f.hub.0.rooms.write().await;
        let mut clients = f.hub.0.clients.write().await;
        let (c, _rx) = test_helpers::create_client_with_rx("w", "Web", true);
        clients.insert("web-1".into(), c);
        room_ops::add_member(
            &room,
            "web-1",
            &mut rooms,
            &mut clients,
            AddOptions {
                by_admin: false,
                promote_if_hostless: true,
            },
        )
        .unwrap();
    }
    let r = f
        .hub
        .add_device("discord", actor("200"), &room, "s1", DeviceRole::Host)
        .await;
    assert_eq!(reason(r), "not_owner");
    // Participants can't remove others' members.
    let r = f
        .hub
        .remove_device("discord", actor("200"), &room, "web-1")
        .await;
    assert_eq!(reason(r), "not_your_device");

    configure(&f.hub, |s| s.allow_receiver = false);
    let r = f
        .hub
        .add_device("discord", actor("200"), &room, "s1", DeviceRole::Receiver)
        .await;
    assert_eq!(reason(r), "role_not_allowed");
    configure(&f.hub, |_| {});

    // Past the checks, the device is looked up in Jellyfin (unreachable here).
    let r = f
        .hub
        .add_device("discord", actor("200"), &room, "s1", DeviceRole::Receiver)
        .await;
    assert_eq!(reason(r), "jellyfin_unavailable");

    // The owner may remove anyone; the room stays open and hostless.
    f.hub
        .remove_device("discord", actor("100"), &room, "web-1")
        .await
        .unwrap();
    let rooms = f.hub.0.rooms.read().await;
    assert!(rooms[&room].is_hostless());
}

#[tokio::test]
async fn other_rooms_are_out_of_reach() {
    let f = fixture().await;
    link(&f.hub, "100", "alice", ALICE).await;
    {
        let mut rooms = f.hub.0.rooms.write().await;
        let mut clients = f.hub.0.clients.write().await;
        let _rx = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "h");
    }
    let r = f
        .hub
        .join_room("discord", actor("100"), "room-1", None)
        .await;
    assert_eq!(reason(r), "not_found");
    let r = f.hub.close_room("discord", actor("100"), "room-1").await;
    assert_eq!(reason(r), "not_found");
}

#[tokio::test]
async fn empty_chat_rooms_wait_then_close() {
    let f = fixture().await;
    link(&f.hub, "100", "alice", ALICE).await;
    let room = f
        .hub
        .create_room("discord", actor("100"), "Later", None)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let now = crate::utils::now_ms();
    assert!(f.hub.close_idle_rooms(now).await.is_empty());
    let later = now + 31 * 60_000;
    assert_eq!(f.hub.close_idle_rooms(later).await, vec![room.clone()]);
    assert!(f.hub.0.rooms.read().await.is_empty());
}

#[tokio::test]
async fn the_room_view_names_owner_participants_and_members() {
    let f = fixture().await;
    let room = owner_and_guest(&f).await;
    f.hub
        .join_room("discord", actor("200"), &room, Some("pw"))
        .await
        .unwrap();
    let list = {
        let rooms = f.hub.0.rooms.read().await;
        let clients = f.hub.0.clients.read().await;
        f.hub
            .store()
            .read(|d| view::rooms_json("discord", &rooms, &clients, &HashMap::new(), d))
    };
    assert_eq!(list.len(), 1);
    let r = &list[0];
    assert_eq!(r["name"], "Movie night");
    assert_eq!(r["has_password"], true);
    assert_eq!(r["owner"]["external_id"], "100");
    assert_eq!(r["participants"][1]["name"], "Bob");
    assert_eq!(r["participants"][1]["external_id"], "200");
    assert!(r["host"].is_null());

    // The sidecar's fixture must stay in step with what the server sends.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../integrations/fixtures/rooms.json")).unwrap();
    assert_keys_subset(&fixture["rooms"][0], r, "room");
    assert_keys_subset(
        &fixture["rooms"][0]["members"][0],
        &serde_json::json!({
            "id": "", "name": "", "kind": "", "is_host": false, "status": "",
            "owner_user_id": null, "owner_external_id": null
        }),
        "member",
    );
}

/// Every key in `expected` (recursively for objects) exists in `actual`.
fn assert_keys_subset(expected: &serde_json::Value, actual: &serde_json::Value, path: &str) {
    if let (Some(e), Some(a)) = (expected.as_object(), actual.as_object()) {
        for (k, v) in e {
            assert!(
                a.contains_key(k),
                "{}.{} missing from the server's output",
                path,
                k
            );
            if v.is_object() && a[k].is_object() {
                assert_keys_subset(v, &a[k], &format!("{}.{}", path, k));
            }
        }
    }
}

// --- HTTP ------------------------------------------------------------------

async fn http(
    f: &Fixture,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let state = api::ApiState::new(
        f.hub.clone(),
        &[
            ("discord".into(), TOKEN.into()),
            ("telegram".into(), TG_TOKEN.into()),
        ],
    );
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
async fn the_api_runs_actions_and_reports_reasons() {
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
        "/v1/rooms",
        Some(TOKEN),
        Some(serde_json::json!({ "actor": a, "name": "Night" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    let id = j["id"].as_str().unwrap().to_string();

    let (s, j) = http(
        &f,
        "PUT",
        &format!("/v1/rooms/{}/panel", id),
        Some(TOKEN),
        Some(serde_json::json!({ "channel_id": "10", "message_id": "99" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);

    let (s, j) = http(&f, "GET", "/v1/rooms?since=0", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["rooms"][0]["panel"]["message_id"], "99");
    assert!(j["version"].as_u64().is_some());

    let (s, j) = http(
        &f,
        "POST",
        &format!("/v1/rooms/{}/kick", id),
        Some(TOKEN),
        Some(serde_json::json!({ "actor": a })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(j["reason"], "invalid");

    let (s, j) = http(
        &f,
        "POST",
        "/v1/rooms/missing/close",
        Some(TOKEN),
        Some(serde_json::json!({ "actor": a })),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(j["reason"], "not_found");

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

#[tokio::test]
async fn long_poll_answers_at_once_when_behind() {
    let f = fixture().await;
    let start = std::time::Instant::now();
    let (s, j) = http(&f, "GET", "/v1/rooms?since=0", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(j["rooms"].as_array().unwrap().is_empty());
}

// --- admin -----------------------------------------------------------------

#[tokio::test]
async fn blank_room_names_are_refused() {
    let f = fixture().await;
    link(&f.hub, "100", "alice", ALICE).await;
    let r = f
        .hub
        .create_room("discord", actor("100"), "  \u{7} ", None)
        .await;
    assert_eq!(reason(r), "invalid");
}

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
        f.hub.0.clients.clone(),
        f.hub.0.rooms.clone(),
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
        crate::admin::JellyfinStatus::Enabled(f.hub.bridges().clone()),
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
    let (s, j, _) = admin_call(
        &router,
        "PUT",
        "/api/integrations/telegram",
        t,
        Some(serde_json::json!({ "enabled": true, "guild_id": "42" })),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "a Telegram group id is negative: {}",
        j
    );
    let (s, j, _) = admin_call(
        &router,
        "PUT",
        "/api/integrations/telegram",
        t,
        Some(serde_json::json!({ "enabled": true, "guild_id": "-1009", "admin_role_id": "admin" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", j);
    let (_, j, _) = admin_call(&router, "GET", "/api/integrations", t, None).await;
    assert_eq!(j["providers"][1]["provider"], "telegram");
    assert_eq!(j["providers"][1]["settings"]["guild_id"], "-1009");
    assert_eq!(j["providers"][0]["settings"]["guild_id"], "42");
    let (s, _, _) = admin_call(
        &router,
        "PUT",
        "/api/integrations/matrix",
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
    assert_eq!(s["providers"][1]["provider"], "telegram");
    assert_eq!(s["providers"][1]["token_var"], "TELEGRAM_INTEGRATION_TOKEN");
    assert_eq!(s["providers"][1]["token_set"], true);
    assert!(s["providers"][1]["sidecar"].is_null());
}
