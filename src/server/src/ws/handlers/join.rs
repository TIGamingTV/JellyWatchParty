use super::super::constants::MAX_CLIENTS_PER_ROOM;
use super::super::dispatch::{is_authenticated, send_error};
use super::super::validation::sanitize_name;
use crate::messaging::{broadcast_to_room, send_to_client};
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
            payload: Some(serde_json::json!({
                "name": room.name,
                "host_id": room.host_id,
                "state": room.state,
                "participant_count": room.clients.len(),
                "media_id": room.media_id
            })),
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

    if !room.clients.contains(&client_id.to_string()) && room.clients.len() >= MAX_CLIENTS_PER_ROOM
    {
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

    info!("Client {} joining room {}", client_id, room_id);
    add_client_to_room(client_id, room, &mut locked_clients, &payload_name);
    notify_join(client_id, room, &locked_clients);
}
