use super::super::constants::{MAX_CHAT_HISTORY, MAX_CHAT_MESSAGE_LENGTH};
use super::super::dispatch::send_error;
use crate::types::{ChatHistoryEntry, Clients, IncomingMessage, Rooms, WsMessage};
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
    rooms: &mut std::collections::HashMap<String, crate::types::Room>,
    clients: &std::collections::HashMap<String, crate::types::Client>,
) -> Option<BroadcastData> {
    let room = rooms.get_mut(room_id)?;
    if !room.clients.contains(&client_id.to_string()) {
        return None;
    }
    let server_ts = now_ms();

    room.chat_history.push_back(ChatHistoryEntry {
        client_id: client_id.to_string(),
        username: username.to_string(),
        text: chat_text.to_string(),
        server_ts,
    });
    if room.chat_history.len() > MAX_CHAT_HISTORY {
        room.chat_history.pop_front();
    }

    let msg = WsMessage {
        msg_type: "chat_message".to_string(),
        room: Some(room_id.to_string()),
        client: Some(client_id.to_string()),
        payload: Some(serde_json::json!({
            "username": username,
            "text": chat_text
        })),
        ts: server_ts,
        server_ts: Some(server_ts),
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
        let mut locked_rooms = rooms.write().await;
        let locked_clients = clients.read().await;
        collect_chat_senders(
            room_id,
            client_id,
            &username,
            chat_text,
            &mut locked_rooms,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_chat_valid() {
        assert!(validate_chat("Hello world").is_ok());
    }

    #[test]
    fn validate_chat_empty() {
        assert!(validate_chat("").is_err());
    }

    #[test]
    fn validate_chat_too_long() {
        let long = "a".repeat(MAX_CHAT_MESSAGE_LENGTH + 1);
        assert!(validate_chat(&long).is_err());
    }

    #[test]
    fn validate_chat_at_limit() {
        let exact = "a".repeat(MAX_CHAT_MESSAGE_LENGTH);
        assert!(validate_chat(&exact).is_ok());
    }

    #[tokio::test]
    async fn handle_chat_message_appends_to_history() {
        let clients = crate::test_helpers::create_clients();
        let rooms = crate::test_helpers::create_rooms();
        let (host, mut rx_h) = crate::test_helpers::create_client_with_rx("uh", "Host", true);
        clients.write().await.insert("host".to_string(), host);
        rooms.write().await.insert(
            "room-1".to_string(),
            crate::test_helpers::create_room("room-1", "host"),
        );

        let parsed = IncomingMessage {
            msg_type: crate::types::ClientMessageType::ChatMessage,
            room: Some("room-1".to_string()),
            client: Some("host".to_string()),
            payload: Some(serde_json::json!({ "text": "hello" })),
            ts: 0,
            server_ts: None,
        };
        handle_chat_message("host", &parsed, &clients, &rooms).await;

        let _ = crate::test_helpers::recv_msg(&mut rx_h); // drain the live broadcast
        let rooms_locked = rooms.read().await;
        let history = &rooms_locked.get("room-1").unwrap().chat_history;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "hello");
        assert_eq!(history[0].username, "Host");
    }

    #[tokio::test]
    async fn handle_chat_message_caps_history_at_max() {
        let clients = crate::test_helpers::create_clients();
        let rooms = crate::test_helpers::create_rooms();
        let (host, _rx_h) = crate::test_helpers::create_client_with_rx("uh", "Host", true);
        clients.write().await.insert("host".to_string(), host);
        rooms.write().await.insert(
            "room-1".to_string(),
            crate::test_helpers::create_room("room-1", "host"),
        );

        for i in 0..(MAX_CHAT_HISTORY + 5) {
            let parsed = IncomingMessage {
                msg_type: crate::types::ClientMessageType::ChatMessage,
                room: Some("room-1".to_string()),
                client: Some("host".to_string()),
                payload: Some(serde_json::json!({ "text": format!("msg {}", i) })),
                ts: 0,
                server_ts: None,
            };
            handle_chat_message("host", &parsed, &clients, &rooms).await;
        }

        let rooms_locked = rooms.read().await;
        let history = &rooms_locked.get("room-1").unwrap().chat_history;
        assert_eq!(history.len(), MAX_CHAT_HISTORY);
        // Oldest messages should have been evicted — the front should no
        // longer be "msg 0".
        assert_ne!(history.front().unwrap().text, "msg 0");
    }
}
