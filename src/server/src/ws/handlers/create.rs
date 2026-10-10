use super::super::dispatch::{is_authenticated, send_error};
use super::super::validation::{
    bridge_device_id, is_valid_media_id, is_valid_position, sanitize_name,
};
use crate::messaging::{
    broadcast_participants, broadcast_room_list, build_room_state_payload, send_to_client,
};
use crate::password::hash_password;
use crate::room::{close_room, handle_leave};
use crate::types::{Clients, IncomingMessage, PlaybackState, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::collections::{HashMap, HashSet, VecDeque};

fn resolve_host_name(
    payload: Option<&serde_json::Value>,
    clients: &std::collections::HashMap<String, crate::types::Client>,
    client_id: &str,
) -> (String, Option<String>) {
    let payload_name = payload
        .and_then(|p| p.get("user_name"))
        .and_then(|v| v.as_str())
        .and_then(sanitize_name);
    let host_name = match &payload_name {
        Some(name) => name.clone(),
        None => clients
            .get(client_id)
            .map(|c| c.user_name.clone())
            .unwrap_or_else(|| "Anonymous".to_string()),
    };
    (host_name, payload_name)
}

fn build_room(client_id: &str, host_name: &str, payload: Option<&serde_json::Value>) -> Room {
    let room_id = uuid::Uuid::new_v4().to_string();
    let raw_start_pos = payload
        .and_then(|p| p.get("start_pos"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let start_pos = if is_valid_position(raw_start_pos) {
        raw_start_pos
    } else {
        0.0
    };
    let media_id = payload
        .and_then(|p| p.get("media_id"))
        .and_then(|v| v.as_str())
        .filter(|id| is_valid_media_id(id))
        .map(|v| v.to_string());
    let password_hash = payload
        .and_then(|p| p.get("password"))
        .and_then(|v| v.as_str())
        .filter(|pw| !pw.is_empty())
        .map(hash_password);
    // The start countdown is opt-in: only a host that sends `started: false`
    // gets it. Hosts that don't send the flag (the native Host Bridge, older
    // web clients) play straight away, so the server must not hold guests.
    let started = payload
        .and_then(|p| p.get("started"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let room_name = format!("Room de {}", host_name);

    info!(
        "Creating room '{}' ({}) for {} (media_id: {:?}, start_pos: {}, has_password: {})",
        room_name,
        room_id,
        client_id,
        media_id,
        start_pos,
        password_hash.is_some()
    );

    Room {
        room_id,
        name: room_name,
        host_id: client_id.to_string(),
        media_id,
        clients: vec![client_id.to_string()],
        ready_clients: HashSet::from([client_id.to_string()]),
        pending_play: None,
        state: PlaybackState {
            position: start_pos,
            play_state: "paused".to_string(),
        },
        last_state_ts: now_ms(),
        last_command_ts: 0,
        chat_history: VecDeque::new(),
        password_hash,
        client_status: HashMap::new(),
        failed_joins: HashMap::new(),
        started,
        admin_created: false,
        created_at: now_ms(),
        chat: None,
    }
}

fn insert_and_notify(
    client_id: &str,
    room: Room,
    payload_name: &Option<String>,
    bridge_device: Option<String>,
    locked_clients: &mut std::collections::HashMap<String, crate::types::Client>,
    locked_rooms: &mut std::collections::HashMap<String, Room>,
) {
    let room_id = room.room_id.clone();
    locked_rooms.insert(room_id.clone(), room.clone());
    if let Some(client) = locked_clients.get_mut(client_id) {
        client.room_id = Some(room_id.clone());
        if let Some(ref name) = payload_name {
            client.user_name = name.clone();
        }
        if let Some(device) = bridge_device {
            client.bridge_device = Some(device);
        }
    }
    send_to_client(
        client_id,
        locked_clients,
        &WsMessage {
            msg_type: "room_state".to_string(),
            room: Some(room_id),
            client: Some(client_id.to_string()),
            payload: Some(build_room_state_payload(&room, 1)),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
    broadcast_participants(&room, locked_clients);
}

pub(in crate::ws) async fn handle_create_room(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    if !is_authenticated(client_id, clients).await {
        send_error(client_id, clients, "Authentication required").await;
        return;
    }

    let taken = {
        let locked_clients = clients.read().await;
        super::join::device_taken_error(client_id, parsed.payload.as_ref(), &locked_clients)
    };
    if let Some(err) = taken {
        let locked_clients = clients.read().await;
        send_to_client(
            client_id,
            &locked_clients,
            &WsMessage {
                msg_type: "error".to_string(),
                room: None,
                client: Some(client_id.to_string()),
                payload: Some(err),
                ts: now_ms(),
                server_ts: Some(now_ms()),
            },
        );
        return;
    }

    // A host starting a new room closes its old one - unless that is a chat
    // room: only its owner (or an admin) may close it, so the host just
    // leaves it below (the next member takes over).
    let existing_room_id = {
        let locked_rooms = rooms.read().await;
        locked_rooms
            .values()
            .find(|r| r.host_id == client_id && r.chat.is_none())
            .map(|r| r.room_id.clone())
    };
    if let Some(room_id) = existing_room_id {
        close_room(&room_id, "Host started a new room", clients, rooms).await;
    }

    let payload_ref = parsed.payload.as_ref();
    let (host_name, payload_name) = {
        let locked_clients = clients.read().await;
        resolve_host_name(payload_ref, &locked_clients, client_id)
    };
    let room = build_room(client_id, &host_name, payload_ref);

    {
        let mut locked_rooms = rooms.write().await;
        let mut locked_clients = clients.write().await;
        // Still a guest somewhere else: leave that room properly first rather
        // than lingering in its member list.
        if locked_clients
            .get(client_id)
            .is_some_and(|c| c.room_id.is_some())
        {
            handle_leave(client_id, &mut locked_clients, &mut locked_rooms);
        }
        insert_and_notify(
            client_id,
            room,
            &payload_name,
            bridge_device_id(payload_ref),
            &mut locked_clients,
            &mut locked_rooms,
        );
    }

    broadcast_room_list(clients, rooms).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[test]
    fn build_room_valid() {
        let room = build_room(
            "host-1",
            "Alice",
            Some(&serde_json::json!({
                "media_id": "550e8400e29b41d4a716446655440000",
                "start_pos": 42.5
            })),
        );
        assert_eq!(room.host_id, "host-1");
        assert_eq!(room.name, "Room de Alice");
        assert_eq!(
            room.media_id,
            Some("550e8400e29b41d4a716446655440000".to_string())
        );
        assert!((room.state.position - 42.5).abs() < f64::EPSILON);
        assert_eq!(room.state.play_state, "paused");
        assert!(room.clients.contains(&"host-1".to_string()));
    }

    #[test]
    fn build_room_started_flag() {
        // Missing flag (native Host Bridge, older web clients): no countdown.
        assert!(build_room("c1", "Host", None).started);
        assert!(build_room("c1", "Host", Some(&serde_json::json!({}))).started);
        let fresh = build_room("c1", "Host", Some(&serde_json::json!({ "started": false })));
        assert!(!fresh.started);
        let playing = build_room("c1", "Host", Some(&serde_json::json!({ "started": true })));
        assert!(playing.started);
    }

    #[test]
    fn build_room_no_media_id() {
        let room = build_room("host-1", "Bob", Some(&serde_json::json!({})));
        assert_eq!(room.media_id, None);
    }

    #[test]
    fn build_room_invalid_media_id() {
        let room = build_room(
            "host-1",
            "Bob",
            Some(&serde_json::json!({ "media_id": "not-valid-hex" })),
        );
        assert_eq!(room.media_id, None);
    }

    #[test]
    fn build_room_clamps_position() {
        let room = build_room(
            "host-1",
            "Bob",
            Some(&serde_json::json!({ "start_pos": -10.0 })),
        );
        assert!((room.state.position - 0.0).abs() < f64::EPSILON);

        let room2 = build_room(
            "host-1",
            "Bob",
            Some(&serde_json::json!({ "start_pos": 100000.0 })),
        );
        assert!((room2.state.position - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn build_room_no_password_by_default() {
        let room = build_room("host-1", "Bob", Some(&serde_json::json!({})));
        assert!(room.password_hash.is_none());
    }

    #[test]
    fn build_room_hashes_provided_password() {
        let room = build_room(
            "host-1",
            "Bob",
            Some(&serde_json::json!({ "password": "hunter2" })),
        );
        assert!(room.password_hash.is_some());
        let (salt, hash) = room.password_hash.unwrap();
        assert!(crate::password::verify_password("hunter2", &salt, &hash));
    }

    #[test]
    fn build_room_ignores_empty_password() {
        let room = build_room(
            "host-1",
            "Bob",
            Some(&serde_json::json!({ "password": "" })),
        );
        assert!(room.password_hash.is_none());
    }

    #[test]
    fn build_room_starts_with_empty_history() {
        let room = build_room("host-1", "Bob", Some(&serde_json::json!({})));
        assert!(room.chat_history.is_empty());
    }

    #[test]
    fn resolve_host_name_from_payload() {
        let mut clients = std::collections::HashMap::new();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "Default", true);
        clients.insert("c1".to_string(), client);
        let (name, payload_name) = resolve_host_name(
            Some(&serde_json::json!({ "user_name": "Custom" })),
            &clients,
            "c1",
        );
        assert_eq!(name, "Custom");
        assert_eq!(payload_name, Some("Custom".to_string()));
    }

    fn create_msg() -> IncomingMessage {
        IncomingMessage {
            msg_type: crate::types::ClientMessageType::CreateRoom,
            room: None,
            client: None,
            payload: Some(serde_json::json!({})),
            ts: 0,
            server_ts: None,
        }
    }

    #[tokio::test]
    async fn creating_a_room_closes_the_hosts_old_room() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let _rx = {
            let mut lc = clients.write().await;
            let mut lr = rooms.write().await;
            test_helpers::setup_room_with_host(&mut lc, &mut lr, "host")
        };
        handle_create_room("host", &create_msg(), &clients, &rooms).await;
        let lr = rooms.read().await;
        assert!(!lr.contains_key("room-1"));
        assert_eq!(lr.len(), 1);
    }

    #[tokio::test]
    async fn a_web_host_cannot_close_a_chat_room_by_starting_another() {
        // Only a chat room's owner (or an admin) may close it. A panel user
        // who became its host just leaves it.
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let _rx = {
            let mut lc = clients.write().await;
            let mut lr = rooms.write().await;
            let rx = test_helpers::setup_room_with_host(&mut lc, &mut lr, "host");
            let (mut guest, _) = test_helpers::create_client_with_rx("ug", "Guest", true);
            guest.room_id = Some("room-1".into());
            lc.insert("guest".into(), guest);
            let room = lr.get_mut("room-1").unwrap();
            room.clients.push("guest".into());
            room.chat = Some(crate::types::ChatRoom {
                provider: "discord".into(),
                owner: "owner".into(),
                owner_name: "Owner".into(),
                participants: Vec::new(),
                panel: None,
                empty_since: None,
            });
            rx
        };
        handle_create_room("host", &create_msg(), &clients, &rooms).await;
        let lr = rooms.read().await;
        let chat_room = lr.get("room-1").expect("the chat room stays open");
        assert!(!chat_room.clients.contains(&"host".to_string()));
        assert_eq!(chat_room.host_id, "guest");
        let lc = clients.read().await;
        let new_room = lc["host"].room_id.clone().unwrap();
        assert_ne!(new_room, "room-1");
        assert_eq!(lr[&new_room].host_id, "host");
    }

    #[test]
    fn resolve_host_name_from_client() {
        let mut clients = std::collections::HashMap::new();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "FromClient", true);
        clients.insert("c1".to_string(), client);
        let (name, payload_name) = resolve_host_name(Some(&serde_json::json!({})), &clients, "c1");
        assert_eq!(name, "FromClient");
        assert_eq!(payload_name, None);
    }
}
