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
    if room.host_id == client_id {
        room.pending_play = None;
    }

    if room.clients.is_empty() || room.host_id == client_id {
        let clients_to_notify = room.clients.clone();
        Some((room_id, clients_to_notify))
    } else {
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
