use super::constants::MAX_MESSAGE_SIZE;
use super::handlers::{
    handle_auth, handle_chat_message, handle_client_log, handle_create_room, handle_join_room,
    handle_leave_room, handle_ping, handle_playback, handle_ready, handle_unknown,
};
use crate::auth::JwtConfig;
use crate::messaging::{send_room_list, send_to_client};
use crate::types::{ClientMessageType, Clients, IncomingMessage, Rooms, WsMessage};
use crate::utils::now_ms;
use log::{debug, warn};
use std::sync::Arc;

pub(super) async fn check_rate_limit(client_id: &str, clients: &Clients) -> bool {
    use super::constants::RATE_LIMIT_MESSAGES;
    use super::constants::RATE_LIMIT_WINDOW_MS;
    let mut locked_clients = clients.write().await;
    if let Some(client) = locked_clients.get_mut(client_id) {
        let now = now_ms();
        client.last_seen = now;
        if now - client.last_reset > RATE_LIMIT_WINDOW_MS {
            client.message_count = 0;
            client.last_reset = now;
        }
        client.message_count += 1;
        if client.message_count > RATE_LIMIT_MESSAGES {
            return true;
        }
    }
    false
}

pub(super) async fn send_error(client_id: &str, clients: &Clients, message: &str) {
    let locked_clients = clients.read().await;
    send_to_client(
        client_id,
        &locked_clients,
        &WsMessage {
            msg_type: "error".to_string(),
            room: None,
            client: Some(client_id.to_string()),
            payload: Some(serde_json::json!({ "message": message })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
}

pub(super) async fn is_authenticated(client_id: &str, clients: &Clients) -> bool {
    let locked = clients.read().await;
    locked
        .get(client_id)
        .map(|c| c.authenticated)
        .unwrap_or(false)
}

pub(super) async fn client_msg(
    client_id: &str,
    msg: warp::ws::Message,
    clients: &Clients,
    rooms: &Rooms,
    jwt_config: &Arc<JwtConfig>,
) {
    if check_rate_limit(client_id, clients).await {
        warn!("Rate limited client: {}", client_id);
        send_error(client_id, clients, "Rate limit exceeded").await;
        return;
    }

    if msg.as_bytes().len() > MAX_MESSAGE_SIZE {
        warn!(
            "Message too large from client {}: {} bytes",
            client_id,
            msg.as_bytes().len()
        );
        send_error(client_id, clients, "Message too large").await;
        return;
    }

    let msg_str = if let Ok(s) = msg.to_str() { s } else { return };

    let parsed: IncomingMessage = match serde_json::from_str(msg_str) {
        Ok(v) => v,
        Err(e) => {
            warn!("JSON parse error from {}: {}", client_id, e);
            send_error(client_id, clients, "Invalid message format").await;
            return;
        }
    };

    debug!("Message from {}: {:?}", client_id, parsed.msg_type);

    match parsed.msg_type {
        ClientMessageType::Auth => handle_auth(client_id, &parsed, clients, jwt_config).await,
        ClientMessageType::ListRooms => send_room_list(client_id, clients, rooms).await,
        ClientMessageType::CreateRoom => {
            handle_create_room(client_id, &parsed, clients, rooms).await
        }
        ClientMessageType::JoinRoom => handle_join_room(client_id, &parsed, clients, rooms).await,
        ClientMessageType::Ready => handle_ready(client_id, &parsed, clients, rooms).await,
        ClientMessageType::LeaveRoom => handle_leave_room(client_id, clients, rooms).await,
        ClientMessageType::PlayerEvent | ClientMessageType::StateUpdate => {
            handle_playback(client_id, parsed, clients, rooms).await
        }
        ClientMessageType::Ping => handle_ping(client_id, &parsed, clients).await,
        ClientMessageType::ClientLog => handle_client_log(client_id, &parsed),
        ClientMessageType::ChatMessage => {
            handle_chat_message(client_id, &parsed, clients, rooms).await
        }
        ClientMessageType::Unknown => handle_unknown(client_id, clients).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[tokio::test]
    async fn check_rate_limit_under() {
        let clients = test_helpers::create_clients();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "User", true);
        clients.write().await.insert("c1".to_string(), client);
        let limited = check_rate_limit("c1", &clients).await;
        assert!(!limited);
    }

    #[tokio::test]
    async fn check_rate_limit_at_limit() {
        use super::super::constants::RATE_LIMIT_MESSAGES;
        let clients = test_helpers::create_clients();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "User", true);
        clients.write().await.insert("c1".to_string(), client);
        for _ in 0..RATE_LIMIT_MESSAGES {
            check_rate_limit("c1", &clients).await;
        }
        // Next message should be rate limited
        let limited = check_rate_limit("c1", &clients).await;
        assert!(limited);
    }

    #[tokio::test]
    async fn is_authenticated_true() {
        let clients = test_helpers::create_clients();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "User", true);
        clients.write().await.insert("c1".to_string(), client);
        assert!(is_authenticated("c1", &clients).await);
    }

    #[tokio::test]
    async fn is_authenticated_false() {
        let clients = test_helpers::create_clients();
        let (client, _rx) = test_helpers::create_client_with_rx("u1", "User", false);
        clients.write().await.insert("c1".to_string(), client);
        assert!(!is_authenticated("c1", &clients).await);
    }

    #[tokio::test]
    async fn is_authenticated_not_found() {
        let clients = test_helpers::create_clients();
        assert!(!is_authenticated("nonexistent", &clients).await);
    }
}
