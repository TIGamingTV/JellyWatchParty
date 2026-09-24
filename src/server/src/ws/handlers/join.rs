use super::super::constants::{FAILED_JOIN_WINDOW_MS, MAX_CLIENTS_PER_ROOM, MAX_FAILED_JOINS};
use super::super::dispatch::{is_authenticated, send_error};
use super::super::validation::sanitize_name;
use crate::messaging::{
    broadcast_participants, broadcast_to_room, build_room_state_payload, send_to_client,
};
use crate::password::verify_password;
use crate::types::{Client, Clients, IncomingMessage, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use log::{info, warn};
use std::collections::HashMap;

fn add_client_to_room(
    client_id: &str,
    room: &mut Room,
    locked_clients: &mut HashMap<String, Client>,
    payload_name: &Option<String>,
) {
    if !room.clients.contains(&client_id.to_string()) {
        room.clients.push(client_id.to_string());
    }
    room.ready_clients.remove(client_id);
    if let Some(client) = locked_clients.get_mut(client_id) {
        client.room_id = Some(room.room_id.clone());
        if let Some(ref name) = payload_name {
            client.user_name = name.clone();
        }
    }
}

fn notify_join(client_id: &str, room: &Room, locked_clients: &HashMap<String, Client>) {
    send_to_client(
        client_id,
        locked_clients,
        &WsMessage {
            msg_type: "room_state".to_string(),
            room: Some(room.room_id.clone()),
            client: Some(client_id.to_string()),
            payload: Some(build_room_state_payload(room, room.clients.len())),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
    broadcast_to_room(
        room,
        locked_clients,
        &WsMessage {
            msg_type: "participants_update".to_string(),
            room: Some(room.room_id.clone()),
            client: None,
            payload: Some(serde_json::json!({ "participant_count": room.clients.len() })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
        Some(client_id),
    );
    broadcast_participants(room, locked_clients);
}

/// If `user_id` has used up its wrong-password budget for this room and the
/// window hasn't expired yet, returns how many ms until it may try again.
fn lockout_remaining_ms(
    failed_joins: &HashMap<String, (u32, u64)>,
    user_id: &str,
    now: u64,
) -> Option<u64> {
    let &(count, window_start) = failed_joins.get(user_id)?;
    let elapsed = now.saturating_sub(window_start);
    (count >= MAX_FAILED_JOINS && elapsed < FAILED_JOIN_WINDOW_MS)
        .then(|| FAILED_JOIN_WINDOW_MS - elapsed)
}

/// Counts one wrong password for `user_id`, starting a fresh window if the
/// previous one expired. Expired entries for other users are pruned at the
/// same time so the map stays bounded by recently-active guessers.
fn record_failed_join(failed_joins: &mut HashMap<String, (u32, u64)>, user_id: &str, now: u64) {
    failed_joins.retain(|_, &mut (_, start)| now.saturating_sub(start) < FAILED_JOIN_WINDOW_MS);
    let entry = failed_joins.entry(user_id.to_string()).or_insert((0, now));
    entry.0 += 1;
}

fn send_join_error(
    client_id: &str,
    room_id: &str,
    locked_clients: &HashMap<String, Client>,
    payload: serde_json::Value,
) {
    send_to_client(
        client_id,
        locked_clients,
        &WsMessage {
            msg_type: "error".to_string(),
            room: Some(room_id.to_string()),
            client: Some(client_id.to_string()),
            payload: Some(payload),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
}

pub(in crate::ws) async fn handle_join_room(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    if !is_authenticated(client_id, clients).await {
        send_error(client_id, clients, "Authentication required").await;
        return;
    }
    let Some(ref room_id) = parsed.room else {
        return;
    };

    let payload_name = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("user_name"))
        .and_then(|v| v.as_str())
        .and_then(sanitize_name);

    let mut locked_rooms = rooms.write().await;
    let mut locked_clients = clients.write().await;

    let Some(room) = locked_rooms.get_mut(room_id) else {
        return;
    };

    let is_existing_member = room.clients.contains(&client_id.to_string());

    if !is_existing_member && room.clients.len() >= MAX_CLIENTS_PER_ROOM {
        send_to_client(
            client_id,
            &locked_clients,
            &WsMessage {
                msg_type: "error".to_string(),
                room: Some(room_id.clone()),
                client: Some(client_id.to_string()),
                payload: Some(serde_json::json!({ "message": "Room is full" })),
                ts: now_ms(),
                server_ts: Some(now_ms()),
            },
        );
        return;
    }

    if !is_existing_member && room.password_hash.is_some() {
        let user_id = locked_clients
            .get(client_id)
            .map(|c| c.user_id.clone())
            .unwrap_or_default();
        let now = now_ms();

        if let Some(retry_after_ms) = lockout_remaining_ms(&room.failed_joins, &user_id, now) {
            warn!(
                "Client {} (user {}) locked out of room {} after repeated wrong passwords",
                client_id, user_id, room_id
            );
            let retry_after_secs = retry_after_ms.div_ceil(1000);
            send_join_error(
                client_id,
                room_id,
                &locked_clients,
                serde_json::json!({
                    "message": format!(
                        "Too many incorrect password attempts. Try again in {}s",
                        retry_after_secs
                    ),
                    "reason": "too_many_attempts",
                    "retry_after_ms": retry_after_ms
                }),
            );
            return;
        }

        let provided = parsed
            .payload
            .as_ref()
            .and_then(|p| p.get("password"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ok = room
            .password_hash
            .as_ref()
            .is_some_and(|(salt, hash)| verify_password(provided, salt, hash));
        if !ok {
            record_failed_join(&mut room.failed_joins, &user_id, now);
            send_join_error(
                client_id,
                room_id,
                &locked_clients,
                serde_json::json!({
                    "message": "Incorrect password",
                    "reason": "wrong_password"
                }),
            );
            return;
        }
        room.failed_joins.remove(&user_id);
    }

    info!("Client {} joining room {}", client_id, room_id);
    add_client_to_room(client_id, room, &mut locked_clients, &payload_name);
    notify_join(client_id, room, &locked_clients);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[test]
    fn add_client_to_room_updates_state() {
        let mut clients = HashMap::new();
        let (client, _rx) = test_helpers::create_client_with_rx("u2", "Guest", true);
        clients.insert("guest-1".to_string(), client);
        let mut room = test_helpers::create_room("room-1", "host-1");

        add_client_to_room("guest-1", &mut room, &mut clients, &None);

        assert!(room.clients.contains(&"guest-1".to_string()));
        assert_eq!(
            clients.get("guest-1").unwrap().room_id,
            Some("room-1".to_string())
        );
    }

    #[test]
    fn add_client_to_room_clears_ready() {
        let mut clients = HashMap::new();
        let (client, _rx) = test_helpers::create_client_with_rx("u2", "Guest", true);
        clients.insert("guest-1".to_string(), client);
        let mut room = test_helpers::create_room("room-1", "host-1");
        room.ready_clients.insert("guest-1".to_string());

        add_client_to_room("guest-1", &mut room, &mut clients, &None);

        assert!(!room.ready_clients.contains("guest-1"));
    }

    #[test]
    fn add_client_to_room_with_payload_name() {
        let mut clients = HashMap::new();
        let (client, _rx) = test_helpers::create_client_with_rx("u2", "OldName", true);
        clients.insert("guest-1".to_string(), client);
        let mut room = test_helpers::create_room("room-1", "host-1");

        let payload_name = Some("NewName".to_string());
        add_client_to_room("guest-1", &mut room, &mut clients, &payload_name);

        assert_eq!(clients.get("guest-1").unwrap().user_name, "NewName");
    }

    #[tokio::test]
    async fn handle_join_room_rejects_wrong_password() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, _rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.password_hash = Some(crate::password::hash_password("secret"));
            lr.insert("room-1".to_string(), room);
        }

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::JoinRoom,
            room: Some("room-1".to_string()),
            client: Some("guest".to_string()),
            payload: Some(serde_json::json!({ "password": "wrong" })),
            ts: 0,
            server_ts: None,
        };
        handle_join_room("guest", &parsed, &clients, &rooms).await;

        let msg = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg.msg_type, "error");
        assert_eq!(
            msg.payload.unwrap().get("reason").unwrap(),
            "wrong_password"
        );
        let lr = rooms.read().await;
        assert!(!lr
            .get("room-1")
            .unwrap()
            .clients
            .contains(&"guest".to_string()));
    }

    #[tokio::test]
    async fn handle_join_room_accepts_correct_password() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, _rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.password_hash = Some(crate::password::hash_password("secret"));
            lr.insert("room-1".to_string(), room);
        }

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::JoinRoom,
            room: Some("room-1".to_string()),
            client: Some("guest".to_string()),
            payload: Some(serde_json::json!({ "password": "secret" })),
            ts: 0,
            server_ts: None,
        };
        handle_join_room("guest", &parsed, &clients, &rooms).await;

        let msg = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg.msg_type, "room_state");
        let lr = rooms.read().await;
        assert!(lr
            .get("room-1")
            .unwrap()
            .clients
            .contains(&"guest".to_string()));
    }

    #[tokio::test]
    async fn handle_join_room_reattach_skips_password_check() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, _rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.password_hash = Some(crate::password::hash_password("secret"));
            room.clients.push("guest".to_string()); // already a member
            lr.insert("room-1".to_string(), room);
        }

        // No password in payload at all — should still succeed since guest
        // is already a room member (e.g. re-sending join after a panel refresh).
        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::JoinRoom,
            room: Some("room-1".to_string()),
            client: Some("guest".to_string()),
            payload: None,
            ts: 0,
            server_ts: None,
        };
        handle_join_room("guest", &parsed, &clients, &rooms).await;

        let msg = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg.msg_type, "room_state");
    }

    // --- failed-join throttle ---

    #[test]
    fn lockout_not_triggered_below_limit() {
        let mut fj = HashMap::new();
        for _ in 0..MAX_FAILED_JOINS - 1 {
            record_failed_join(&mut fj, "u", 1_000);
        }
        assert_eq!(lockout_remaining_ms(&fj, "u", 1_000), None);
    }

    #[test]
    fn lockout_triggered_at_limit_and_reports_remaining() {
        let mut fj = HashMap::new();
        for _ in 0..MAX_FAILED_JOINS {
            record_failed_join(&mut fj, "u", 1_000);
        }
        assert_eq!(
            lockout_remaining_ms(&fj, "u", 1_000 + 10_000),
            Some(FAILED_JOIN_WINDOW_MS - 10_000)
        );
        assert_eq!(lockout_remaining_ms(&fj, "other", 1_000), None);
    }

    #[test]
    fn lockout_expires_after_window() {
        let mut fj = HashMap::new();
        for _ in 0..MAX_FAILED_JOINS {
            record_failed_join(&mut fj, "u", 1_000);
        }
        let later = 1_000 + FAILED_JOIN_WINDOW_MS;
        assert_eq!(lockout_remaining_ms(&fj, "u", later), None);
        // A new failure after expiry starts a fresh window at count 1.
        record_failed_join(&mut fj, "u", later);
        assert_eq!(fj.get("u"), Some(&(1, later)));
    }

    #[test]
    fn record_failed_join_prunes_expired_entries() {
        let mut fj = HashMap::new();
        record_failed_join(&mut fj, "stale", 0);
        record_failed_join(&mut fj, "fresh", FAILED_JOIN_WINDOW_MS);
        assert!(!fj.contains_key("stale"));
        assert!(fj.contains_key("fresh"));
    }

    fn join_msg(client: &str, password: &str) -> IncomingMessage {
        IncomingMessage {
            msg_type: crate::types::ClientMessageType::JoinRoom,
            room: Some("room-1".to_string()),
            client: Some(client.to_string()),
            payload: Some(serde_json::json!({ "password": password })),
            ts: 0,
            server_ts: None,
        }
    }

    async fn setup_password_room(
        guests: &[(&str, &str)],
    ) -> (
        Clients,
        Rooms,
        HashMap<String, crate::types::ClientReceiver>,
    ) {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let mut rxs = HashMap::new();
        {
            let mut lc = clients.write().await;
            let (host, rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
            lc.insert("host".to_string(), host);
            rxs.insert("host".to_string(), rx_h);
            for (cid, uid) in guests {
                let (c, rx) = test_helpers::create_client_with_rx(uid, "Guest", true);
                lc.insert(cid.to_string(), c);
                rxs.insert(cid.to_string(), rx);
            }
        }
        {
            let mut room = test_helpers::create_room("room-1", "host");
            room.password_hash = Some(crate::password::hash_password("secret"));
            rooms.write().await.insert("room-1".to_string(), room);
        }
        (clients, rooms, rxs)
    }

    fn drain(rx: &mut crate::types::ClientReceiver) -> Vec<WsMessage> {
        std::iter::from_fn(|| test_helpers::recv_msg(rx)).collect()
    }

    fn last_reason(rx: &mut crate::types::ClientReceiver) -> Option<String> {
        drain(rx).last().and_then(|m| {
            m.payload
                .as_ref()
                .and_then(|p| p.get("reason"))
                .and_then(|r| r.as_str())
                .map(String::from)
        })
    }

    #[tokio::test]
    async fn handle_join_room_locks_out_after_repeated_wrong_passwords() {
        let (clients, rooms, mut rxs) = setup_password_room(&[("guest", "ug")]).await;
        let rx = rxs.get_mut("guest").unwrap();

        for _ in 0..MAX_FAILED_JOINS {
            handle_join_room("guest", &join_msg("guest", "wrong"), &clients, &rooms).await;
            assert_eq!(last_reason(rx).as_deref(), Some("wrong_password"));
        }

        // Even the correct password is refused while locked out, and the
        // password isn't evaluated.
        handle_join_room("guest", &join_msg("guest", "secret"), &clients, &rooms).await;
        let msgs = drain(rx);
        let payload = msgs.last().unwrap().payload.clone().unwrap();
        assert_eq!(payload["reason"], "too_many_attempts");
        assert!(payload["retry_after_ms"].as_u64().unwrap() > 0);
        assert!(!rooms.read().await["room-1"]
            .clients
            .contains(&"guest".to_string()));
    }

    #[tokio::test]
    async fn handle_join_room_lockout_survives_client_id_rotation() {
        // Same user reconnecting under a fresh client id stays locked out.
        let (clients, rooms, mut rxs) =
            setup_password_room(&[("guest-a", "ug"), ("guest-b", "ug")]).await;

        for _ in 0..MAX_FAILED_JOINS {
            handle_join_room("guest-a", &join_msg("guest-a", "wrong"), &clients, &rooms).await;
        }
        handle_join_room("guest-b", &join_msg("guest-b", "secret"), &clients, &rooms).await;
        assert_eq!(
            last_reason(rxs.get_mut("guest-b").unwrap()).as_deref(),
            Some("too_many_attempts")
        );
    }

    #[tokio::test]
    async fn handle_join_room_lockout_is_per_user() {
        let (clients, rooms, mut rxs) =
            setup_password_room(&[("attacker", "ua"), ("friend", "uf")]).await;

        for _ in 0..MAX_FAILED_JOINS {
            handle_join_room("attacker", &join_msg("attacker", "wrong"), &clients, &rooms).await;
        }
        handle_join_room("friend", &join_msg("friend", "secret"), &clients, &rooms).await;

        let msgs = drain(rxs.get_mut("friend").unwrap());
        assert_eq!(msgs.first().unwrap().msg_type, "room_state");
        assert!(rooms.read().await["room-1"]
            .clients
            .contains(&"friend".to_string()));
    }

    #[tokio::test]
    async fn handle_join_room_allows_retry_after_window() {
        let (clients, rooms, mut rxs) = setup_password_room(&[("guest", "ug")]).await;
        rooms
            .write()
            .await
            .get_mut("room-1")
            .unwrap()
            .failed_joins
            .insert(
                "ug".to_string(),
                (MAX_FAILED_JOINS, now_ms() - FAILED_JOIN_WINDOW_MS - 1),
            );

        handle_join_room("guest", &join_msg("guest", "secret"), &clients, &rooms).await;

        let msgs = drain(rxs.get_mut("guest").unwrap());
        assert_eq!(msgs.first().unwrap().msg_type, "room_state");
        assert!(rooms.read().await["room-1"].failed_joins.is_empty());
    }

    #[tokio::test]
    async fn handle_join_room_success_resets_failure_count() {
        let (clients, rooms, _rxs) = setup_password_room(&[("guest", "ug")]).await;

        for _ in 0..MAX_FAILED_JOINS - 1 {
            handle_join_room("guest", &join_msg("guest", "wrong"), &clients, &rooms).await;
        }
        assert_eq!(
            rooms.read().await["room-1"]
                .failed_joins
                .get("ug")
                .map(|e| e.0),
            Some(MAX_FAILED_JOINS - 1)
        );
        handle_join_room("guest", &join_msg("guest", "secret"), &clients, &rooms).await;
        assert!(!rooms.read().await["room-1"].failed_joins.contains_key("ug"));
    }
}
