use crate::messaging::send_to_client;
use crate::types::{Clients, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::time::Duration;

/// How long a disconnected client's room slot is held open, waiting for
/// them to reconnect under the same persistent client_id, before the
/// room is actually torn down / the client is actually removed.
const RECONNECT_GRACE_SECS: u64 = 90;

/// Called whenever a client's WebSocket connection ends — whether it
/// closed normally, errored, or was reaped as a zombie. Instead of
/// immediately destroying the client's room (the old behavior), this
/// waits `RECONNECT_GRACE_SECS` and only then checks whether the client
/// actually came back.
///
/// Detection works by comparing mpsc channel identity: if the client
/// reconnects with the same client_id within the window, connection.rs
/// swaps in a brand-new sender for that entry. If nobody reconnected,
/// the entry still holds the original (now-dead) sender, which we
/// captured before scheduling this check.
pub async fn schedule_disconnect(client_id: String, clients: Clients, rooms: Rooms) {
    let stale_sender = {
        let locked = clients.read().await;
        match locked.get(&client_id) {
            Some(c) => c.sender.clone(),
            None => return, // Already gone; nothing to schedule.
        }
    };

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(RECONNECT_GRACE_SECS)).await;

        let never_reconnected = {
            let locked = clients.read().await;
            match locked.get(&client_id) {
                Some(c) => c.sender.same_channel(&stale_sender),
                None => false,
            }
        };

        if never_reconnected {
            info!(
                "Client {} did not reconnect within {}s, disconnecting",
                client_id, RECONNECT_GRACE_SECS
            );
            crate::room::handle_disconnect(&client_id, &clients, &rooms).await;
        } else {
            info!(
                "Client {} reconnected within the grace period, keeping room state",
                client_id
            );
        }
    });
}

/// Sent to a client immediately after it reattaches to an existing room
/// (i.e. it reconnected with a client_id that was already a room member).
/// Mirrors the payload shape of the normal join flow so the client's
/// existing `room_state` handler (which also restores host/guest role)
/// needs no special-casing for reconnects.
pub async fn resend_room_state(client_id: &str, room_id: &str, clients: &Clients, rooms: &Rooms) {
    let locked_rooms = rooms.read().await;
    let Some(room) = locked_rooms.get(room_id) else {
        return;
    };
    let locked_clients = clients.read().await;
    send_to_client(
        client_id,
        &locked_clients,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn schedule_disconnect_noop_when_client_missing() {
        let clients: Clients = Arc::new(RwLock::new(HashMap::new()));
        let rooms: Rooms = Arc::new(RwLock::new(HashMap::new()));
        schedule_disconnect("ghost".to_string(), clients, rooms).await;
    }

    #[tokio::test]
    async fn resend_room_state_sends_to_existing_room_member() {
        let mut clients_map = HashMap::new();
        let (client, mut rx) = test_helpers::create_client_with_rx("u1", "Host", true);
        clients_map.insert("host-1".to_string(), client);
        let clients: Clients = Arc::new(RwLock::new(clients_map));

        let mut rooms_map = HashMap::new();
        rooms_map.insert(
            "room-1".to_string(),
            test_helpers::create_room("room-1", "host-1"),
        );
        let rooms: Rooms = Arc::new(RwLock::new(rooms_map));

        resend_room_state("host-1", "room-1", &clients, &rooms).await;

        let received = test_helpers::recv_msg(&mut rx);
        assert!(received.is_some());
        assert_eq!(received.unwrap().msg_type, "room_state");
    }

    #[tokio::test]
    async fn resend_room_state_noop_for_unknown_room() {
        let clients: Clients = Arc::new(RwLock::new(HashMap::new()));
        let rooms: Rooms = Arc::new(RwLock::new(HashMap::new()));
        resend_room_state("host-1", "does-not-exist", &clients, &rooms).await;
    }
}
