use super::super::dispatch::send_error;
use super::super::validation::sanitize_name;
use crate::auth::JwtConfig;
use crate::messaging::send_to_client;
use crate::types::{Clients, IncomingMessage, WsMessage};
use crate::utils::now_ms;
use log::{info, warn};
use std::sync::Arc;

async fn handle_jwt_auth(
    client_id: &str,
    token: &str,
    clients: &Clients,
    jwt_config: &Arc<JwtConfig>,
) -> bool {
    match jwt_config.validate_token(token) {
        Ok(claims) => {
            let mut locked = clients.write().await;
            if let Some(client) = locked.get_mut(client_id) {
                client.authenticated = true;
                client.user_id = claims.sub;
                client.user_name = claims.name.clone();
                info!("Client {} authenticated as {}", client_id, claims.name);
            }
            drop(locked);
            let locked_clients = clients.read().await;
            send_to_client(
                client_id,
                &locked_clients,
                &WsMessage {
                    msg_type: "auth_success".to_string(),
                    room: None,
                    client: Some(client_id.to_string()),
                    payload: Some(serde_json::json!({ "user_name": claims.name })),
                    ts: now_ms(),
                    server_ts: Some(now_ms()),
                },
            );
            true
        }
        Err(e) => {
            warn!("Auth failed for {}: {}", client_id, e);
            false
        }
    }
}

async fn handle_identity(client_id: &str, payload: &serde_json::Value, clients: &Clients) {
    let user_name = payload
        .get("user_name")
        .and_then(|v| v.as_str())
        .and_then(sanitize_name);
    let user_id = payload.get("user_id").and_then(|v| v.as_str());
    if let Some(name) = user_name {
        let mut locked = clients.write().await;
        if let Some(client) = locked.get_mut(client_id) {
            client.user_name = name.clone();
            if let Some(uid) = user_id {
                client.user_id = uid.to_string();
            }
            info!("Client {} identified as {}", client_id, name);
        }
    }
}

pub(in crate::ws) async fn handle_auth(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    jwt_config: &Arc<JwtConfig>,
) {
    if let Some(payload) = &parsed.payload {
        if let Some(token) = payload.get("token").and_then(|v| v.as_str()) {
            if handle_jwt_auth(client_id, token, clients, jwt_config).await {
                return;
            }
            send_error(client_id, clients, "Authentication failed").await;
            return;
        }
        if !jwt_config.enabled {
            handle_identity(client_id, payload, clients).await;
        } else {
            warn!(
                "Client {} sent auth without token but JWT is required",
                client_id
            );
            send_error(
                client_id,
                clients,
                "Authentication required: no token provided",
            )
            .await;
        }
    } else if jwt_config.enabled {
        warn!(
            "Client {} sent auth with no payload but JWT is required",
            client_id
        );
        send_error(
            client_id,
            clients,
            "Authentication required: no token provided",
        )
        .await;
    }
}
