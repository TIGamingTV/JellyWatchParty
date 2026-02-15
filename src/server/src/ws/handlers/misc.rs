use super::super::constants::PLAY_SCHEDULE_MS;
use super::super::dispatch::send_error;
use super::super::pending_play::{all_ready, broadcast_scheduled_play};
use crate::messaging::{broadcast_room_list, send_to_client};
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

pub(in crate::ws) async fn handle_leave_room(client_id: &str, clients: &Clients, rooms: &Rooms) {
    info!("Client {} leaving room", client_id);
    {
        let mut locked_clients = clients.write().await;
        let mut locked_rooms = rooms.write().await;
        handle_leave(client_id, &mut locked_clients, &mut locked_rooms);
    }
    broadcast_room_list(clients, rooms).await;
}
