use super::super::dispatch::{is_authenticated, send_error};
use super::super::validation::{is_valid_media_id, is_valid_position, sanitize_name};
use crate::messaging::{broadcast_room_list, send_to_client};
use crate::room::close_room;
use crate::types::{Clients, IncomingMessage, PlaybackState, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::collections::HashSet;

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
    let room_name = format!("Room de {}", host_name);

    info!(
        "Creating room '{}' ({}) for {}",
        room_name, room_id, client_id
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
    }
}

fn insert_and_notify(
    client_id: &str,
    room: Room,
    payload_name: &Option<String>,
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
    }
    send_to_client(
        client_id,
        locked_clients,
        &WsMessage {
            msg_type: "room_state".to_string(),
            room: Some(room_id),
            client: Some(client_id.to_string()),
            payload: Some(serde_json::json!({
                "name": room.name,
                "host_id": room.host_id,
                "state": room.state,
                "participant_count": 1,
                "media_id": room.media_id
            })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
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

    let existing_room_id = {
        let locked_rooms = rooms.read().await;
        locked_rooms
            .values()
            .find(|r| r.host_id == client_id)
            .map(|r| r.room_id.clone())
    };
    if let Some(room_id) = existing_room_id {
        close_room(&room_id, clients, rooms).await;
    }

    info!("create_room payload: {:?}", parsed.payload);

    let payload_ref = parsed.payload.as_ref();
    let (host_name, payload_name) = {
        let locked_clients = clients.read().await;
        resolve_host_name(payload_ref, &locked_clients, client_id)
    };
    let room = build_room(client_id, &host_name, payload_ref);

    {
        let mut locked_rooms = rooms.write().await;
        let mut locked_clients = clients.write().await;
        insert_and_notify(
            client_id,
            room,
            &payload_name,
            &mut locked_clients,
            &mut locked_rooms,
        );
    }

    broadcast_room_list(clients, rooms).await;
}
