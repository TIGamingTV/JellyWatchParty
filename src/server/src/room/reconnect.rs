use crate::messaging::{build_participants_msg, build_room_state_payload, send_to_client};
use crate::types::{Clients, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::time::Duration;

/// How long a disconnected client's room slot is held open, waiting for
/// them to reconnect under the same persistent client_id, before the
/// room is actually torn down / the client is actually removed.
const RECONNECT_GRACE_SECS: u64 = 90;

/// A client that hasn't sent anything for this long counts as gone even if
/// its socket still looks open (matches the zombie reaper's timeout).
pub const STALE_AFTER_MS: u64 = 60_000;

/// Whether connection `conn_id` of `client` should be removed now that its
/// grace period is over: it must still be the attached connection (nobody
/// reconnected), and either it ended or it has been silent too long.
pub(crate) fn should_evict(client: &crate::types::Client, conn_id: u64, now: u64) -> bool {
    client.conn_id == conn_id
        && (!client.connected || now.saturating_sub(client.last_seen) > STALE_AFTER_MS)
}

/// Called when connection `conn_id` of a client ends - closed normally,
/// errored, or reaped as a zombie. Instead of destroying the client's room
/// slot at once, waits `RECONNECT_GRACE_SECS` and only then checks whether
/// the client came back.
///
/// Connections are told apart by `conn_id`: a reconnect (connection.rs)
/// attaches a new id, so a timer for an older connection finds a different
/// id and leaves the entry alone - even if that older socket only noticed
/// it was dead long after the client had moved on.
pub async fn schedule_disconnect(client_id: String, conn_id: u64, clients: Clients, rooms: Rooms) {
    {
        let locked = clients.read().await;
        match locked.get(&client_id) {
            Some(c) if c.conn_id == conn_id => {}
            _ => return, // Gone, or another connection owns it now.
        }
    }

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(RECONNECT_GRACE_SECS)).await;

        let evict = {
            let locked = clients.read().await;
            locked
                .get(&client_id)
                .is_some_and(|c| should_evict(c, conn_id, now_ms()))
        };

        if evict {
            info!(
                "Client {} did not reconnect within {}s, disconnecting",
                client_id, RECONNECT_GRACE_SECS
            );
            crate::room::handle_disconnect(&client_id, &clients, &rooms).await;
        } else {
            info!(
                "Client {} reconnected or is active again, keeping room state",
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
            payload: Some(build_room_state_payload(room, room.clients.len())),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
    send_to_client(
        client_id,
        &locked_clients,
        &build_participants_msg(room, &locked_clients),
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
        schedule_disconnect("ghost".to_string(), 1, clients, rooms).await;
    }

    #[test]
    fn only_the_attached_ended_or_silent_connection_is_evicted() {
        let (mut c, _rx) = test_helpers::create_client_with_rx("u", "U", true);
        let now = now_ms();
        c.conn_id = 7;
        c.connected = true;
        c.last_seen = now;
        assert!(!should_evict(&c, 7, now), "alive and attached");
        c.connected = false;
        assert!(should_evict(&c, 7, now), "ended, nobody came back");
        assert!(!should_evict(&c, 6, now), "an older connection's timer");
        c.conn_id = 8;
        c.connected = true;
        assert!(
            !should_evict(&c, 7, now),
            "reconnected under a new connection"
        );
        c.last_seen = now - STALE_AFTER_MS - 1;
        assert!(should_evict(&c, 8, now), "socket open but silent (zombie)");
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
