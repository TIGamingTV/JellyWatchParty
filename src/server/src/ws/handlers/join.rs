use super::super::constants::MAX_CLIENTS_PER_ROOM;
use super::super::dispatch::{is_authenticated, send_error};
use super::super::validation::sanitize_name;
use crate::messaging::{broadcast_to_room, build_room_state_payload, send_to_client};
use crate::password::verify_password;
use crate::types::{Client, Clients, IncomingMessage, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
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

    if !is_existing_member {
        if let Some((salt, hash)) = &room.password_hash {
            let provided = parsed
                .payload
                .as_ref()
                .and_then(|p| p.get("password"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !verify_password(provided, salt, hash) {
                send_to_client(
                    client_id,
                    &locked_clients,
                    &WsMessage {
                        msg_type: "error".to_string(),
                        room: Some(room_id.clone()),
                        client: Some(client_id.to_string()),
                        payload: Some(serde_json::json!({
                            "message": "Incorrect password",
                            "reason": "wrong_password"
                        })),
                        ts: now_ms(),
                        server_ts: Some(now_ms()),
                    },
                );
                return;
            }
        }
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
}
