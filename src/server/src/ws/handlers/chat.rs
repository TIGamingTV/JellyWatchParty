use super::super::constants::MAX_CHAT_MESSAGE_LENGTH;
use super::super::dispatch::send_error;
use crate::types::{Clients, IncomingMessage, Rooms, WsMessage};
use crate::utils::now_ms;
use tokio::sync::mpsc;

fn validate_chat(text: &str) -> Result<(), &'static str> {
    if text.is_empty() {
        return Err("Chat message cannot be empty");
    }
    if text.len() > MAX_CHAT_MESSAGE_LENGTH {
        return Err("Chat message too long");
    }
    Ok(())
}

type BroadcastData = (
    Vec<mpsc::Sender<Result<warp::ws::Message, warp::Error>>>,
    String,
);

fn collect_chat_senders(
    room_id: &str,
    client_id: &str,
    username: &str,
    chat_text: &str,
    rooms: &std::collections::HashMap<String, crate::types::Room>,
    clients: &std::collections::HashMap<String, crate::types::Client>,
) -> Option<BroadcastData> {
    let room = rooms.get(room_id)?;
    if !room.clients.contains(&client_id.to_string()) {
        return None;
    }
    let msg = WsMessage {
        msg_type: "chat_message".to_string(),
        room: Some(room_id.to_string()),
        client: Some(client_id.to_string()),
        payload: Some(serde_json::json!({
            "username": username,
            "text": chat_text
        })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    let senders: Vec<_> = room
        .clients
        .iter()
        .filter_map(|id| clients.get(id).map(|c| c.sender.clone()))
        .collect();
    let json = serde_json::to_string(&msg).ok()?;
    Some((senders, json))
}

pub(in crate::ws) async fn handle_chat_message(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    let Some(ref room_id) = parsed.room else {
        send_error(client_id, clients, "Room ID required for chat").await;
        return;
    };

    let chat_text = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if let Err(msg) = validate_chat(chat_text) {
        let detail = if chat_text.len() > MAX_CHAT_MESSAGE_LENGTH {
            format!("{} (max {} characters)", msg, MAX_CHAT_MESSAGE_LENGTH)
        } else {
            msg.to_string()
        };
        send_error(client_id, clients, &detail).await;
        return;
    }

    let username = {
        let locked_clients = clients.read().await;
        locked_clients
            .get(client_id)
            .map(|c| c.user_name.clone())
            .unwrap_or_else(|| "Anonymous".to_string())
    };

    let broadcast_data = {
        let locked_rooms = rooms.read().await;
        let locked_clients = clients.read().await;
        collect_chat_senders(
            room_id,
            client_id,
            &username,
            chat_text,
            &locked_rooms,
            &locked_clients,
        )
    };

    if let Some((senders, json)) = broadcast_data {
        let warp_msg = warp::ws::Message::text(json);
        for sender in senders {
            if let Err(e) = sender.try_send(Ok(warp_msg.clone())) {
                log::warn!("Failed to send chat_message (buffer full or closed): {}", e);
            }
        }
    }
}
