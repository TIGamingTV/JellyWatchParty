use crate::messaging::{broadcast_room_list, send_to_client};
use crate::types::{Client, Clients, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::collections::HashMap;

fn notify_room_closed(
    room_id: &str,
    clients_list: &[String],
    locked_clients: &HashMap<String, Client>,
) {
    let msg = WsMessage {
        msg_type: "room_closed".to_string(),
        room: Some(room_id.to_string()),
        client: None,
        payload: Some(serde_json::json!({ "reason": "Host started a new room" })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    for cid in clients_list {
        send_to_client(cid, locked_clients, &msg);
    }
}

fn clear_room_from_clients(
    room_id: &str,
    client_ids: &[String],
    locked_clients: &mut HashMap<String, Client>,
) {
    for cid in client_ids {
        if let Some(client) = locked_clients.get_mut(cid) {
            if client.room_id.as_deref() == Some(room_id) {
                client.room_id = None;
            }
        }
    }
}

pub async fn close_room(room_id: &str, clients: &Clients, rooms: &Rooms) {
    let clients_to_notify: Vec<String>;

    {
        let mut locked_rooms = rooms.write().await;
        let locked_clients = clients.read().await;

        let Some(room) = locked_rooms.remove(room_id) else {
            return;
        };
        info!("Closing room {} (host creating new room)", room_id);
        clients_to_notify = room.clients.clone();

        notify_room_closed(room_id, &clients_to_notify, &locked_clients);

        drop(locked_clients);
        let mut locked_clients = clients.write().await;
        clear_room_from_clients(room_id, &clients_to_notify, &mut locked_clients);
    }

    broadcast_room_list(clients, rooms).await;
}
