use super::super::dispatch::send_error;
use super::super::validation::{is_valid_media_id, is_valid_position};
use crate::messaging::{broadcast_room_list, broadcast_to_room};
use crate::types::{Clients, IncomingMessage, PlaybackState, Rooms, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::collections::HashSet;

/// Host-only: changes which item a room is watching, either because the
/// host started something after creating an empty room, or switched to a
/// different item mid-session (issue #71). Resets playback state and
/// `ready_clients` to just the host, so the host's next `play` waits (up to
/// `MAX_READY_WAIT_MS`, same as any other pending play) for guests to load
/// the new item before playback is scheduled for everyone.
pub(in crate::ws) async fn handle_set_media(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    let Some(ref room_id) = parsed.room else {
        return;
    };

    let Some(media_id) = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("media_id"))
        .and_then(|v| v.as_str())
    else {
        send_error(client_id, clients, "media_id is required").await;
        return;
    };
    if !is_valid_media_id(media_id) {
        send_error(client_id, clients, "Invalid media_id").await;
        return;
    }
    let position = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("position"))
        .and_then(|v| v.as_f64())
        .filter(|pos| is_valid_position(*pos))
        .unwrap_or(0.0);

    let changed = {
        let mut locked_rooms = rooms.write().await;
        let locked_clients = clients.read().await;

        let Some(room) = locked_rooms.get_mut(room_id) else {
            return;
        };
        if room.host_id != client_id {
            return;
        }
        if room.media_id.as_deref() == Some(media_id) {
            // No-op: already watching this item, don't reset ready state or
            // interrupt in-flight sync for nothing.
            return;
        }

        info!(
            "Room {} media changed by host {} to {}",
            room_id, client_id, media_id
        );

        room.media_id = Some(media_id.to_string());
        room.state = PlaybackState {
            position,
            play_state: "paused".to_string(),
        };
        room.pending_play = None;
        room.ready_clients = HashSet::from([client_id.to_string()]);
        room.last_state_ts = now_ms();
        room.last_command_ts = 0;

        let msg = WsMessage {
            msg_type: "media_changed".to_string(),
            room: Some(room_id.clone()),
            client: None,
            payload: Some(serde_json::json!({
                "media_id": media_id,
                "position": position,
            })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        };
        broadcast_to_room(room, &locked_clients, &msg, Some(client_id));
        true
    };

    if changed {
        broadcast_room_list(clients, rooms).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    fn set_media_msg(room: &str, media_id: &str, position: f64) -> IncomingMessage {
        IncomingMessage {
            msg_type: crate::types::ClientMessageType::SetMedia,
            room: Some(room.to_string()),
            client: None,
            payload: Some(serde_json::json!({ "media_id": media_id, "position": position })),
            ts: 0,
            server_ts: None,
        }
    }

    const MEDIA_A: &str = "550e8400e29b41d4a716446655440000";
    const MEDIA_B: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[tokio::test]
    async fn host_can_change_media() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, mut rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.clients = vec!["host".to_string(), "guest".to_string()];
            room.ready_clients = HashSet::from(["host".to_string(), "guest".to_string()]);
            room.media_id = Some(MEDIA_A.to_string());
            lr.insert("room-1".to_string(), room);
        }

        let parsed = set_media_msg("room-1", MEDIA_B, 42.0);
        handle_set_media("host", &parsed, &clients, &rooms).await;

        let lr = rooms.read().await;
        let room = lr.get("room-1").unwrap();
        assert_eq!(room.media_id, Some(MEDIA_B.to_string()));
        assert!((room.state.position - 42.0).abs() < f64::EPSILON);
        assert_eq!(room.state.play_state, "paused");
        assert!(room.pending_play.is_none());
        assert_eq!(room.ready_clients, HashSet::from(["host".to_string()]));
        drop(lr);

        // The host isn't sent media_changed (it already knows) - only the
        // refreshed room_list every connected client gets.
        let msg_h = test_helpers::recv_msg(&mut rx_h).unwrap();
        assert_eq!(msg_h.msg_type, "room_list");
        assert!(rx_h.try_recv().is_err());

        // The guest gets media_changed first, then room_list.
        let msg_g = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg_g.msg_type, "media_changed");
        let payload = msg_g.payload.unwrap();
        assert_eq!(payload.get("media_id").unwrap(), MEDIA_B);
        assert_eq!(payload.get("position").unwrap(), 42.0);
        let msg_g2 = test_helpers::recv_msg(&mut rx_g).unwrap();
        assert_eq!(msg_g2.msg_type, "room_list");
    }

    #[tokio::test]
    async fn non_host_is_ignored() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, _rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        let (guest, mut rx_g) = test_helpers::create_client_with_rx("ug", "Guest", true);
        {
            let mut lc = clients.write().await;
            lc.insert("host".to_string(), host);
            lc.insert("guest".to_string(), guest);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.clients = vec!["host".to_string(), "guest".to_string()];
            lr.insert("room-1".to_string(), room);
        }

        let parsed = set_media_msg("room-1", MEDIA_B, 0.0);
        handle_set_media("guest", &parsed, &clients, &rooms).await;

        let lr = rooms.read().await;
        assert_eq!(lr.get("room-1").unwrap().media_id, None);
        drop(lr);
        assert!(rx_g.try_recv().is_err());
    }

    #[tokio::test]
    async fn invalid_media_id_is_rejected() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, mut rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        {
            clients.write().await.insert("host".to_string(), host);
        }
        {
            let mut lr = rooms.write().await;
            lr.insert(
                "room-1".to_string(),
                test_helpers::create_room("room-1", "host"),
            );
        }

        let parsed = set_media_msg("room-1", "not-valid-hex", 0.0);
        handle_set_media("host", &parsed, &clients, &rooms).await;

        let lr = rooms.read().await;
        assert_eq!(lr.get("room-1").unwrap().media_id, None);
        drop(lr);
        let msg = test_helpers::recv_msg(&mut rx_h).unwrap();
        assert_eq!(msg.msg_type, "error");
    }

    #[tokio::test]
    async fn same_media_id_is_a_no_op() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, mut rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        {
            clients.write().await.insert("host".to_string(), host);
        }
        {
            let mut lr = rooms.write().await;
            let mut room = test_helpers::create_room("room-1", "host");
            room.media_id = Some(MEDIA_A.to_string());
            room.ready_clients = HashSet::from(["host".to_string(), "someone-else".to_string()]);
            lr.insert("room-1".to_string(), room);
        }

        let parsed = set_media_msg("room-1", MEDIA_A, 10.0);
        handle_set_media("host", &parsed, &clients, &rooms).await;

        let lr = rooms.read().await;
        let room = lr.get("room-1").unwrap();
        // ready_clients untouched: it wasn't actually a media change.
        assert_eq!(
            room.ready_clients,
            HashSet::from(["host".to_string(), "someone-else".to_string()])
        );
        drop(lr);
        assert!(rx_h.try_recv().is_err());
    }

    #[tokio::test]
    async fn empty_room_can_get_its_first_media() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (host, mut rx_h) = test_helpers::create_client_with_rx("uh", "Host", true);
        {
            clients.write().await.insert("host".to_string(), host);
        }
        {
            let mut lr = rooms.write().await;
            lr.insert(
                "room-1".to_string(),
                test_helpers::create_room("room-1", "host"),
            );
        }

        let parsed = set_media_msg("room-1", MEDIA_A, 0.0);
        handle_set_media("host", &parsed, &clients, &rooms).await;

        let lr = rooms.read().await;
        assert_eq!(
            lr.get("room-1").unwrap().media_id,
            Some(MEDIA_A.to_string())
        );
        drop(lr);
        // Only member is the host, and hosts don't receive their own
        // media_changed broadcast - just the refreshed room_list.
        let msg_h = test_helpers::recv_msg(&mut rx_h).unwrap();
        assert_eq!(msg_h.msg_type, "room_list");
        assert!(rx_h.try_recv().is_err());
    }

    #[tokio::test]
    async fn unknown_room_is_ignored() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let parsed = set_media_msg("nope", MEDIA_A, 0.0);
        // Should not panic.
        handle_set_media("host", &parsed, &clients, &rooms).await;
    }
}
