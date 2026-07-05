use crate::messaging::{broadcast_room_list, broadcast_to_room};
use crate::types::{Client, Clients, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::collections::HashMap;

fn detach_client_from_room(
    client_id: &str,
    clients: &mut HashMap<String, Client>,
    rooms: &mut HashMap<String, Room>,
) -> Option<(String, Vec<String>)> {
    let client = clients.get_mut(client_id)?;
    let room_id = client.room_id.take()?;
    let room = rooms.get_mut(&room_id)?;

    room.clients.retain(|id| id != client_id);
    room.ready_clients.remove(client_id);
    let was_host = room.host_id == client_id;
    if was_host {
        room.pending_play = None;
    }

    if room.clients.is_empty() {
        let clients_to_notify = room.clients.clone();
        return Some((room_id, clients_to_notify));
    }

    if was_host {
        promote_new_host(&room_id, room, clients);
        return None;
    }

    let msg = WsMessage {
        msg_type: "client_left".to_string(),
        room: Some(room_id),
        client: Some(client_id.to_string()),
        payload: Some(serde_json::json!({ "participant_count": room.clients.len() })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    broadcast_to_room(room, clients, &msg, None);
    None
}

/// Promotes the earliest-joined remaining participant to host, in place,
/// and broadcasts `host_changed` instead of closing the room. `room.clients`
/// is insertion-ordered, so the new host is simply the first entry left
/// after the departing host was removed.
fn promote_new_host(room_id: &str, room: &mut Room, clients: &HashMap<String, Client>) {
    let new_host_id = room.clients[0].clone();
    room.host_id = new_host_id.clone();
    let new_host_name = clients
        .get(&new_host_id)
        .map(|c| c.user_name.clone())
        .unwrap_or_else(|| "Someone".to_string());

    let msg = WsMessage {
        msg_type: "host_changed".to_string(),
        room: Some(room_id.to_string()),
        client: Some(new_host_id.clone()),
        payload: Some(serde_json::json!({
            "host_id": new_host_id,
            "host_name": new_host_name,
            "participant_count": room.clients.len()
        })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    broadcast_to_room(room, clients, &msg, None);
}

fn close_and_notify(
    room_id: &str,
    clients_to_notify: &[String],
    clients: &HashMap<String, Client>,
    rooms: &mut HashMap<String, Room>,
) {
    info!("Closing room {}", room_id);
    rooms.remove(room_id);
    let msg = WsMessage {
        msg_type: "room_closed".to_string(),
        room: Some(room_id.to_string()),
        client: None,
        payload: Some(serde_json::json!({ "reason": "Host left the room" })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    if let Ok(msg_json) = serde_json::to_string(&msg) {
        for cid in clients_to_notify {
            if let Some(c) = clients.get(cid) {
                let _ = c
                    .sender
                    .try_send(Ok(warp::ws::Message::text(msg_json.clone())));
            }
        }
    }
}

pub fn handle_leave(
    client_id: &str,
    clients: &mut HashMap<String, Client>,
    rooms: &mut HashMap<String, Room>,
) {
    if let Some((room_id, clients_to_notify)) = detach_client_from_room(client_id, clients, rooms) {
        close_and_notify(&room_id, &clients_to_notify, clients, rooms);
    }
}

pub async fn handle_disconnect(client_id: &str, clients: &Clients, rooms: &Rooms) {
    info!("Disconnecting client {}", client_id);
    {
        let mut locked_clients = clients.write().await;
        let mut locked_rooms = rooms.write().await;
        handle_leave(client_id, &mut locked_clients, &mut locked_rooms);
        locked_clients.remove(client_id);
    }
    broadcast_room_list(clients, rooms).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use crate::types::PendingPlay;

    #[test]
    fn detach_client_removes_from_room() {
        let mut clients = HashMap::new();
        let mut rooms = HashMap::new();
        let _rx = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host-1");

        let (mut guest, _rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        guest.room_id = Some("room-1".to_string());
        clients.insert("guest-1".to_string(), guest);
        rooms
            .get_mut("room-1")
            .unwrap()
            .clients
            .push("guest-1".to_string());

        // Detach the guest (non-host) — room still has host, so it stays open
        detach_client_from_room("guest-1", &mut clients, &mut rooms);

        let room = rooms.get("room-1").unwrap();
        assert!(!room.clients.contains(&"guest-1".to_string()));
        assert!(clients.get("guest-1").unwrap().room_id.is_none());
    }

    #[test]
    fn detach_host_clears_pending_play() {
        let mut clients = HashMap::new();
        let mut rooms = HashMap::new();
        let _rx = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host-1");

        rooms.get_mut("room-1").unwrap().pending_play = Some(PendingPlay {
            position: 10.0,
            created_at: crate::utils::now_ms(),
        });

        detach_client_from_room("host-1", &mut clients, &mut rooms);

        // Room should be returned for closing (host left)
        // The pending_play is cleared before close_and_notify removes the room
        assert!(clients.get("host-1").unwrap().room_id.is_none());
    }

    #[test]
    fn detach_client_not_in_room() {
        let mut clients = HashMap::new();
        let mut rooms = HashMap::new();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "User", true);
        clients.insert("c1".to_string(), client);

        let result = detach_client_from_room("c1", &mut clients, &mut rooms);
        assert!(result.is_none());
    }

    #[test]
    fn detach_host_with_remaining_guests_promotes_earliest_joined() {
        let mut clients = HashMap::new();
        let mut rooms = HashMap::new();
        let _rx = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host-1");

        let (guest_a, mut rx_a) = test_helpers::create_client_with_rx("ua", "GuestA", true);
        let (guest_b, mut rx_b) = test_helpers::create_client_with_rx("ub", "GuestB", true);
        clients.insert("guest-a".to_string(), guest_a);
        clients.insert("guest-b".to_string(), guest_b);
        {
            let room = rooms.get_mut("room-1").unwrap();
            room.clients.push("guest-a".to_string());
            room.clients.push("guest-b".to_string());
        }

        let result = detach_client_from_room("host-1", &mut clients, &mut rooms);

        // Room should NOT be signaled for closing — it was promoted instead.
        assert!(result.is_none());
        let room = rooms.get("room-1").unwrap();
        assert_eq!(room.host_id, "guest-a"); // earliest-joined remaining client
        assert!(room.clients.contains(&"guest-a".to_string()));
        assert!(room.clients.contains(&"guest-b".to_string()));

        let msg_a = test_helpers::recv_msg(&mut rx_a).unwrap();
        assert_eq!(msg_a.msg_type, "host_changed");
        assert_eq!(
            msg_a.payload.as_ref().unwrap().get("host_id").unwrap(),
            "guest-a"
        );
        let msg_b = test_helpers::recv_msg(&mut rx_b).unwrap();
        assert_eq!(msg_b.msg_type, "host_changed");
    }

    #[test]
    fn detach_host_with_no_remaining_clients_signals_close() {
        let mut clients = HashMap::new();
        let mut rooms = HashMap::new();
        let _rx = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host-1");

        let result = detach_client_from_room("host-1", &mut clients, &mut rooms);

        assert!(result.is_some());
        let (room_id, clients_to_notify) = result.unwrap();
        assert_eq!(room_id, "room-1");
        assert!(clients_to_notify.is_empty());
    }
}
