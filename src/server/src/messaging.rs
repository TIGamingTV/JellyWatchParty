use crate::types::{Client, Clients, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use std::collections::HashMap;

/// Builds the payload shared by every place a client needs to be told (or
/// reminded) of a room's full state: initial create, join, and reattach
/// after a dropped-connection reconnect.
pub fn build_room_state_payload(room: &Room, participant_count: usize) -> serde_json::Value {
    let chat_history: Vec<serde_json::Value> = room
        .chat_history
        .iter()
        .map(|entry| {
            serde_json::json!({
                "client_id": entry.client_id,
                "username": entry.username,
                "text": entry.text,
                "server_ts": entry.server_ts,
            })
        })
        .collect();
    serde_json::json!({
        "name": room.name,
        "host_id": room.host_id,
        "state": room.state,
        "participant_count": participant_count,
        "media_id": room.media_id,
        "chat_history": chat_history,
        "democratic_mode": room.democratic_mode,
    })
}

fn build_room_list_msg(rooms: &HashMap<String, Room>) -> WsMessage {
    let list: Vec<serde_json::Value> = rooms
        .values()
        .map(|r| {
            serde_json::json!({
                "id": r.room_id,
                "name": r.name,
                "count": r.clients.len(),
                "media_id": r.media_id,
                "has_password": r.password_hash.is_some(),
            })
        })
        .collect();
    WsMessage {
        msg_type: "room_list".to_string(),
        room: None,
        client: None,
        payload: Some(serde_json::json!(list)),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    }
}

pub async fn send_room_list(client_id: &str, clients: &Clients, rooms: &Rooms) {
    let locked_rooms = rooms.read().await;
    let msg = build_room_list_msg(&locked_rooms);
    let locked_clients = clients.read().await;
    send_to_client(client_id, &locked_clients, &msg);
}

pub async fn broadcast_room_list(clients: &Clients, rooms: &Rooms) {
    let json = {
        let locked_rooms = rooms.read().await;
        let msg = build_room_list_msg(&locked_rooms);
        match serde_json::to_string(&msg) {
            Ok(j) => j,
            Err(e) => {
                log::error!("Failed to serialize room list: {}", e);
                return;
            }
        }
    };
    let locked_clients = clients.read().await;
    let warp_msg = warp::ws::Message::text(json);
    for client in locked_clients.values() {
        if let Err(e) = client.sender.try_send(Ok(warp_msg.clone())) {
            log::warn!("Failed to send room list (buffer full or closed): {}", e);
        }
    }
}

pub fn send_to_client(client_id: &str, clients: &HashMap<String, Client>, msg: &WsMessage) {
    if let Some(client) = clients.get(client_id) {
        match serde_json::to_string(msg) {
            Ok(json) => {
                if let Err(e) = client.sender.try_send(Ok(warp::ws::Message::text(json))) {
                    log::warn!(
                        "Failed to send to client {} (buffer full or closed): {}",
                        client_id,
                        e
                    );
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to serialize message for client {}: {}",
                    client_id,
                    e
                );
            }
        }
    }
}

pub fn broadcast_to_room(
    room: &Room,
    clients: &HashMap<String, Client>,
    msg: &WsMessage,
    exclude: Option<&str>,
) {
    let json = match serde_json::to_string(msg) {
        Ok(j) => j,
        Err(e) => {
            log::error!(
                "Failed to serialize broadcast message for room {}: {}",
                room.room_id,
                e
            );
            return;
        }
    };
    let warp_msg = warp::ws::Message::text(json);
    for client_id in &room.clients {
        if Some(client_id.as_str()) == exclude {
            continue;
        }
        if let Some(client) = clients.get(client_id) {
            if let Err(e) = client.sender.try_send(Ok(warp_msg.clone())) {
                log::warn!(
                    "Failed to broadcast to client {} (buffer full or closed): {}",
                    client_id,
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use crate::types::PlaybackState;
    use std::collections::{HashSet, VecDeque};

    #[test]
    fn room_list_msg_reports_has_password() {
        let mut rooms = HashMap::new();
        let mut room = test_helpers::create_room("r1", "host1");
        room.password_hash = Some(crate::password::hash_password("secret"));
        rooms.insert("r1".to_string(), room);

        let msg = build_room_list_msg(&rooms);
        let list = msg.payload.unwrap();
        let entry = &list.as_array().unwrap()[0];
        assert_eq!(entry.get("has_password").unwrap(), true);
    }

    #[test]
    fn room_state_payload_includes_chat_history_and_democratic_mode() {
        let mut room = test_helpers::create_room("r1", "host1");
        room.chat_history.push_back(crate::types::ChatHistoryEntry {
            client_id: "host1".to_string(),
            username: "Host".to_string(),
            text: "hi".to_string(),
            server_ts: 123,
        });
        room.democratic_mode = true;

        let payload = build_room_state_payload(&room, 2);
        assert_eq!(payload.get("participant_count").unwrap(), 2);
        assert_eq!(payload.get("democratic_mode").unwrap(), true);
        let history = payload.get("chat_history").unwrap().as_array().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].get("text").unwrap(), "hi");
    }

    #[test]
    fn build_room_list_msg_empty() {
        let rooms = HashMap::new();
        let msg = build_room_list_msg(&rooms);
        assert_eq!(msg.msg_type, "room_list");
        let payload = msg.payload.unwrap();
        let list = payload.as_array().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn build_room_list_msg_multiple() {
        let mut rooms = HashMap::new();
        rooms.insert(
            "r1".to_string(),
            Room {
                room_id: "r1".to_string(),
                name: "Room 1".to_string(),
                host_id: "host1".to_string(),
                media_id: None,
                clients: vec!["a".to_string(), "b".to_string()],
                ready_clients: HashSet::new(),
                pending_play: None,
                state: PlaybackState {
                    position: 0.0,
                    play_state: "paused".to_string(),
                },
                last_state_ts: 0,
                last_command_ts: 0,
                chat_history: VecDeque::new(),
                democratic_mode: false,
                password_hash: None,
            },
        );
        rooms.insert(
            "r2".to_string(),
            Room {
                room_id: "r2".to_string(),
                name: "Room 2".to_string(),
                host_id: "host2".to_string(),
                media_id: Some("abc".to_string()),
                clients: vec!["c".to_string()],
                ready_clients: HashSet::new(),
                pending_play: None,
                state: PlaybackState {
                    position: 10.0,
                    play_state: "playing".to_string(),
                },
                last_state_ts: 0,
                last_command_ts: 0,
                chat_history: VecDeque::new(),
                democratic_mode: false,
                password_hash: None,
            },
        );
        let msg = build_room_list_msg(&rooms);
        let list = msg.payload.unwrap();
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn send_to_client_success() {
        let (client, mut rx) = test_helpers::create_client_with_rx("user1", "User1", true);
        let mut clients = HashMap::new();
        clients.insert("c1".to_string(), client);
        let msg = WsMessage {
            msg_type: "test".to_string(),
            room: None,
            client: None,
            payload: None,
            ts: 0,
            server_ts: None,
        };
        send_to_client("c1", &clients, &msg);
        let received = test_helpers::recv_msg(&mut rx);
        assert!(received.is_some());
        assert_eq!(received.unwrap().msg_type, "test");
    }

    #[test]
    fn send_to_client_not_found() {
        let clients = HashMap::new();
        let msg = WsMessage {
            msg_type: "test".to_string(),
            room: None,
            client: None,
            payload: None,
            ts: 0,
            server_ts: None,
        };
        // Should not panic
        send_to_client("nonexistent", &clients, &msg);
    }

    #[test]
    fn broadcast_to_room_excludes_sender() {
        let (client_a, mut _rx_a) = test_helpers::create_client_with_rx("ua", "A", true);
        let (client_b, mut rx_b) = test_helpers::create_client_with_rx("ub", "B", true);
        let (client_c, mut rx_c) = test_helpers::create_client_with_rx("uc", "C", true);
        let mut clients = HashMap::new();
        clients.insert("a".to_string(), client_a);
        clients.insert("b".to_string(), client_b);
        clients.insert("c".to_string(), client_c);
        let mut room = test_helpers::create_room("room-1", "a");
        room.clients = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let msg = WsMessage {
            msg_type: "event".to_string(),
            room: None,
            client: None,
            payload: None,
            ts: 0,
            server_ts: None,
        };
        broadcast_to_room(&room, &clients, &msg, Some("a"));
        // a should NOT receive (excluded)
        assert!(_rx_a.try_recv().is_err());
        // b and c should receive
        assert!(test_helpers::recv_msg(&mut rx_b).is_some());
        assert!(test_helpers::recv_msg(&mut rx_c).is_some());
    }

    #[test]
    fn broadcast_to_room_no_exclude() {
        let (client_a, mut rx_a) = test_helpers::create_client_with_rx("ua", "A", true);
        let (client_b, mut rx_b) = test_helpers::create_client_with_rx("ub", "B", true);
        let mut clients = HashMap::new();
        clients.insert("a".to_string(), client_a);
        clients.insert("b".to_string(), client_b);
        let mut room = test_helpers::create_room("room-1", "a");
        room.clients = vec!["a".to_string(), "b".to_string()];
        let msg = WsMessage {
            msg_type: "event".to_string(),
            room: None,
            client: None,
            payload: None,
            ts: 0,
            server_ts: None,
        };
        broadcast_to_room(&room, &clients, &msg, None);
        assert!(test_helpers::recv_msg(&mut rx_a).is_some());
        assert!(test_helpers::recv_msg(&mut rx_b).is_some());
    }
}
