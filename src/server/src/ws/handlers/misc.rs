use super::super::constants::PLAY_SCHEDULE_MS;
use super::super::dispatch::send_error;
use super::super::pending_play::{all_ready, broadcast_scheduled_play};
use crate::messaging::{broadcast_room_list, broadcast_to_room, send_to_client};
use crate::room::handle_leave;
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
            room.ready_clients.insert(client_id.to_string());
            if room.pending_play.is_some() && all_ready(room) {
                let target_server_ts = now_ms() + PLAY_SCHEDULE_MS;
                let position = room
                    .pending_play
                    .as_ref()
                    .map(|p| p.position)
                    .unwrap_or(room.state.position);
                room.pending_play = None;
                broadcast_scheduled_play(room, clients, position, target_server_ts).await;
            }
        }
    }
}

pub(in crate::ws) async fn handle_toggle_democratic_mode(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    let Some(ref room_id) = parsed.room else {
        return;
    };
    let enabled = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("enabled"))
        .and_then(|v| v.as_bool());
    let Some(enabled) = enabled else {
        send_error(client_id, clients, "Missing 'enabled' boolean").await;
        return;
    };

    let mut locked_rooms = rooms.write().await;
    let Some(room) = locked_rooms.get_mut(room_id) else {
        return;
    };
    if room.host_id != client_id {
        drop(locked_rooms);
        send_error(client_id, clients, "Only the host can change this setting").await;
        return;
    }

    room.democratic_mode = enabled;
    let locked_clients = clients.read().await;
    broadcast_to_room(
        room,
        &locked_clients,
        &WsMessage {
            msg_type: "democratic_mode_changed".to_string(),
            room: Some(room_id.clone()),
            client: None,
            payload: Some(serde_json::json!({ "enabled": enabled })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
        None,
    );
}

pub(in crate::ws) async fn handle_leave_room(client_id: &str, clients: &Clients, rooms: &Rooms) {
    info!("Client {} leaving room", client_id);
    {
        let mut locked_clients = clients.write().await;
        let mut locked_rooms = rooms.write().await;
        handle_leave(client_id, &mut locked_clients, &mut locked_rooms);
    }
    broadcast_room_list(clients, rooms).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[tokio::test]
    async fn handle_toggle_democratic_mode_host_can_enable() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, mut rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        clients.write().await.insert("host".to_string(), host);
        rooms.write().await.insert(
            "room-1".to_string(),
            test_helpers::create_room("room-1", "host"),
        );

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::ToggleDemocraticMode,
            room: Some("room-1".to_string()),
            client: Some("host".to_string()),
            payload: Some(serde_json::json!({ "enabled": true })),
            ts: 0,
            server_ts: None,
        };
        handle_toggle_democratic_mode("host", &parsed, &clients, &rooms).await;

        assert!(rooms.read().await.get("room-1").unwrap().democratic_mode);
        let msg = test_helpers::recv_msg(&mut rx_h).unwrap();
        assert_eq!(msg.msg_type, "democratic_mode_changed");
    }

    #[tokio::test]
    async fn handle_toggle_democratic_mode_rejects_non_host() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, _rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        clients.write().await.insert("host".to_string(), host);
        clients.write().await.insert("guest".to_string(), guest);
        {
            let mut room = test_helpers::create_room("room-1", "host");
            room.clients.push("guest".to_string());
            rooms.write().await.insert("room-1".to_string(), room);
        }

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::ToggleDemocraticMode,
            room: Some("room-1".to_string()),
            client: Some("guest".to_string()),
            payload: Some(serde_json::json!({ "enabled": true })),
            ts: 0,
            server_ts: None,
        };
        handle_toggle_democratic_mode("guest", &parsed, &clients, &rooms).await;

        assert!(!rooms.read().await.get("room-1").unwrap().democratic_mode);
        let msg = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg.msg_type, "error");
    }

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
}
