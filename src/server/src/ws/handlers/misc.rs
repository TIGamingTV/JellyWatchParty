use super::super::dispatch::send_error;
use super::super::pending_play::{all_ready, broadcast_scheduled_play};
use crate::messaging::{broadcast_room_list, send_to_client};
use crate::room::handle_leave;
use crate::room::ops::set_democratic;
use crate::types::{Clients, IncomingMessage, Rooms, WsMessage};
use crate::utils::now_ms;
use log::{info, warn};

pub(in crate::ws) async fn handle_ping(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
) {
    let locked_clients = clients.read().await;
    send_to_client(
        client_id,
        &locked_clients,
        &WsMessage {
            msg_type: "pong".to_string(),
            room: parsed.room.clone(),
            client: parsed.client.clone(),
            payload: parsed.payload.clone(),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
}

pub(in crate::ws) fn handle_client_log(client_id: &str, parsed: &IncomingMessage) {
    if let Some(payload) = &parsed.payload {
        let category = payload
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("LOG");
        let message = payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let short_id = &client_id[..8];
        info!("[CLIENT:{}:{}] {}", short_id, category, message);
    }
}

pub(in crate::ws) async fn handle_unknown(client_id: &str, clients: &Clients) {
    warn!("Unknown message type from client {}", client_id);
    send_error(client_id, clients, "Unknown message type").await;
}

pub(in crate::ws) async fn handle_ready(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    if let Some(ref room_id) = parsed.room {
        let mut locked_rooms = rooms.write().await;
        if let Some(room) = locked_rooms.get_mut(room_id) {
            // all_ready compares counts, so a `ready` from someone who isn't
            // (or no longer is) a member must not count.
            if !room.clients.iter().any(|id| id == client_id) {
                return;
            }
            room.ready_clients.insert(client_id.to_string());
            if room.pending_play.is_some() && all_ready(room) {
                let position = room
                    .pending_play
                    .as_ref()
                    .map(|p| p.position)
                    .unwrap_or(room.state.position);
                broadcast_scheduled_play(room, clients, position).await;
            }
        }
    }
}

/// Host-only: `set_democratic {enabled}` turns the room's democratic mode
/// on or off (see `room::ops::set_democratic`).
pub(in crate::ws) async fn handle_set_democratic(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    let Some(ref room_id) = parsed.room else {
        return;
    };
    let Some(enabled) = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("enabled"))
        .and_then(|v| v.as_bool())
    else {
        send_error(client_id, clients, "enabled is required").await;
        return;
    };
    let changed = {
        let mut locked_rooms = rooms.write().await;
        let locked_clients = clients.read().await;
        let Some(room) = locked_rooms.get(room_id) else {
            return;
        };
        if room.host_id != client_id {
            drop(locked_clients);
            drop(locked_rooms);
            send_error(client_id, clients, "Only the host can change this setting").await;
            return;
        }
        let by = locked_clients
            .get(client_id)
            .map(|c| c.user_name.clone())
            .unwrap_or_default();
        set_democratic(
            room_id,
            Some(enabled),
            None,
            &by,
            &mut locked_rooms,
            &locked_clients,
        )
        .unwrap_or(false)
    };
    if changed {
        broadcast_room_list(clients, rooms).await;
    }
}

pub(in crate::ws) async fn handle_leave_room(client_id: &str, clients: &Clients, rooms: &Rooms) {
    info!("Client {} leaving room", client_id);
    {
        // Lock order everywhere is rooms -> clients; taking them the other
        // way round here could deadlock against a concurrent join/create.
        let mut locked_rooms = rooms.write().await;
        let mut locked_clients = clients.write().await;
        handle_leave(client_id, &mut locked_clients, &mut locked_rooms);
    }
    broadcast_room_list(clients, rooms).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[tokio::test]
    async fn handle_ping_responds_pong() {
        let clients = test_helpers::create_clients();
        let (client, mut rx) = test_helpers::create_client_with_rx("u1", "User", true);
        clients.write().await.insert("c1".to_string(), client);

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::Ping,
            room: Some("room-1".to_string()),
            client: Some("c1".to_string()),
            payload: Some(serde_json::json!({ "seq": 42 })),
            ts: 12345,
            server_ts: None,
        };
        handle_ping("c1", &parsed, &clients).await;

        let msg = test_helpers::recv_msg(&mut rx).unwrap();
        assert_eq!(msg.msg_type, "pong");
        assert_eq!(msg.room, Some("room-1".to_string()));
        let seq = msg.payload.unwrap().get("seq").unwrap().as_i64().unwrap();
        assert_eq!(seq, 42);
    }

    #[tokio::test]
    async fn handle_ready_adds_to_set() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, _rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, _rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);

        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.clients.push("guest".to_string());
            room.ready_clients.clear();
            lr.insert("room-1".to_string(), room);
        }

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::Ready,
            room: Some("room-1".to_string()),
            client: Some("guest".to_string()),
            payload: None,
            ts: 0,
            server_ts: None,
        };
        handle_ready("guest", &parsed, &clients, &rooms).await;

        let lr = rooms.read().await;
        let room = lr.get("room-1").unwrap();
        assert!(room.ready_clients.contains("guest"));
    }

    #[tokio::test]
    async fn handle_ready_all_ready_triggers_play() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, mut rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);

        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.clients = vec!["host".to_string(), "guest".to_string()];
            room.ready_clients.clear();
            room.ready_clients.insert("host".to_string());
            room.pending_play = Some(crate::types::PendingPlay {
                position: 10.0,
                created_at: crate::utils::now_ms(),
            });
            lr.insert("room-1".to_string(), room);
        }

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::Ready,
            room: Some("room-1".to_string()),
            client: Some("guest".to_string()),
            payload: None,
            ts: 0,
            server_ts: None,
        };
        handle_ready("guest", &parsed, &clients, &rooms).await;

        // Both should receive a player_event (play broadcast)
        let msg_h = test_helpers::recv_msg(&mut rx_h).unwrap();
        assert_eq!(msg_h.msg_type, "player_event");
        let msg_g = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg_g.msg_type, "player_event");

        // pending_play should be cleared
        let lr = rooms.read().await;
        assert!(lr.get("room-1").unwrap().pending_play.is_none());
    }

    #[tokio::test]
    async fn handle_ready_ignores_non_members() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.ready_clients.clear();
            lr.insert("room-1".to_string(), room);
        }
        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::Ready,
            room: Some("room-1".to_string()),
            client: None,
            payload: None,
            ts: 0,
            server_ts: None,
        };
        handle_ready("stranger", &parsed, &clients, &rooms).await;
        assert!(rooms.read().await["room-1"].ready_clients.is_empty());
    }

    #[tokio::test]
    async fn only_the_host_switches_democratic_mode() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = {
            let mut lc = clients.write().await;
            let mut lr = rooms.write().await;
            let host_rx = test_helpers::setup_room_with_host(&mut lc, &mut lr, "host");
            let (mut guest, guest_rx) = test_helpers::create_client_with_rx("ug", "Guest", true);
            guest.room_id = Some("room-1".into());
            lc.insert("guest".into(), guest);
            lr.get_mut("room-1").unwrap().clients.push("guest".into());
            (host_rx, guest_rx)
        };
        let toggle = |client: &str, enabled: bool| IncomingMessage {
            msg_type: crate::types::ClientMessageType::SetDemocratic,
            room: Some("room-1".to_string()),
            client: Some(client.to_string()),
            payload: Some(serde_json::json!({ "enabled": enabled })),
            ts: 0,
            server_ts: None,
        };

        handle_set_democratic("guest", &toggle("guest", true), &clients, &rooms).await;
        assert!(!rooms.read().await["room-1"].democratic);
        assert_eq!(
            test_helpers::recv_msg(&mut guest_rx).unwrap().msg_type,
            "error"
        );

        handle_set_democratic("host", &toggle("host", true), &clients, &rooms).await;
        assert!(rooms.read().await["room-1"].democratic);
        for rx in [&mut host_rx, &mut guest_rx] {
            let msg = test_helpers::recv_msg(rx).unwrap();
            assert_eq!(msg.msg_type, "democratic_changed");
            let p = msg.payload.unwrap();
            assert_eq!(p["enabled"], true);
            assert_eq!(p["device_control"], false);
            // Then the refreshed room list, which flags the room.
            let list = test_helpers::recv_msg(rx).unwrap();
            assert_eq!(list.msg_type, "room_list");
            assert_eq!(list.payload.unwrap()[0]["democratic"], true);
        }
        // Joiners learn it from room_state.
        let payload = crate::messaging::build_room_state_payload(&rooms.read().await["room-1"], 2);
        assert_eq!(payload["democratic"], true);

        // No change, no broadcast.
        handle_set_democratic("host", &toggle("host", true), &clients, &rooms).await;
        assert!(test_helpers::recv_msg(&mut host_rx).is_none());
    }
}
