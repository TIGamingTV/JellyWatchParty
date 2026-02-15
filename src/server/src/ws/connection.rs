use super::constants::CLIENT_CHANNEL_BUFFER;
use super::dispatch::client_msg;
use crate::auth::JwtConfig;
use crate::messaging::{send_room_list, send_to_client};
use crate::types::{Clients, Rooms, WsMessage};
use crate::utils::now_ms;
use futures::StreamExt;
use log::info;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

fn register_client(
    client_sender: mpsc::Sender<Result<warp::ws::Message, warp::Error>>,
    jwt_config: &Arc<JwtConfig>,
) -> crate::types::Client {
    let now = now_ms();
    let authenticated = !jwt_config.enabled;
    let (user_id, user_name) = if authenticated {
        ("anonymous".to_string(), "Anonymous".to_string())
    } else {
        ("".to_string(), "".to_string())
    };

    crate::types::Client {
        sender: client_sender,
        room_id: None,
        user_id,
        user_name,
        authenticated,
        message_count: 0,
        last_reset: now,
        last_seen: now,
    }
}

fn send_client_hello(
    client_id: &str,
    locked_clients: &std::collections::HashMap<String, crate::types::Client>,
) {
    send_to_client(
        client_id,
        locked_clients,
        &WsMessage {
            msg_type: "client_hello".to_string(),
            room: None,
            client: Some(client_id.to_string()),
            payload: Some(serde_json::json!({ "client_id": client_id })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
}

pub async fn client_connection(
    ws: warp::ws::WebSocket,
    clients: Clients,
    rooms: Rooms,
    jwt_config: Arc<JwtConfig>,
) {
    let (client_ws_sender, mut client_ws_rcv) = ws.split();
    let (client_sender, client_rcv) = mpsc::channel(CLIENT_CHANNEL_BUFFER);
    let client_rcv = ReceiverStream::new(client_rcv);

    tokio::task::spawn(async move {
        let _ = client_rcv.forward(client_ws_sender).await;
    });

    let temp_id = uuid::Uuid::new_v4().to_string();
    info!(
        "Client connected: {} (auth_required: {})",
        temp_id, jwt_config.enabled
    );

    let client = register_client(client_sender, &jwt_config);
    clients.write().await.insert(temp_id.clone(), client);

    {
        let locked_clients = clients.read().await;
        send_client_hello(&temp_id, &locked_clients);
    }

    send_room_list(&temp_id, &clients, &rooms).await;

    while let Some(result) = client_ws_rcv.next().await {
        if let Ok(msg) = result {
            client_msg(&temp_id, msg, &clients, &rooms, &jwt_config).await;
        }
    }

    crate::room::handle_disconnect(&temp_id, &clients, &rooms).await;
}
