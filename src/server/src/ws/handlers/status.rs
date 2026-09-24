use crate::messaging::broadcast_participants;
use crate::types::{Clients, IncomingMessage, Room, Rooms};

/// Statuses a client may report for itself, as shown in the participant list.
const VALID_STATUSES: [&str; 7] = [
    "synced",
    "syncing",
    "buffering",
    "loading",
    "idle",
    "playing",
    "paused",
];

fn is_valid_status(status: &str) -> bool {
    VALID_STATUSES.contains(&status)
}

/// Stores `status` for `client_id`. Returns `false` when the client isn't in
/// the room or the status didn't change (nothing to broadcast).
fn apply_status(room: &mut Room, client_id: &str, status: &str) -> bool {
    if !room.clients.iter().any(|id| id == client_id) {
        return false;
    }
    if room.client_status.get(client_id).map(String::as_str) == Some(status) {
        return false;
    }
    room.client_status
        .insert(client_id.to_string(), status.to_string());
    true
}

/// `client_status`: a participant reports its own playback status (in sync,
/// catching up, buffering, ...). Everyone in the room gets the updated list.
pub(in crate::ws) async fn handle_client_status(
    client_id: &str,
    parsed: &IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    let Some(ref room_id) = parsed.room else {
        return;
    };
    let Some(status) = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("status"))
        .and_then(|v| v.as_str())
        .filter(|s| is_valid_status(s))
    else {
        return;
    };
    let mut locked_rooms = rooms.write().await;
    let Some(room) = locked_rooms.get_mut(room_id) else {
        return;
    };
    if apply_status(room, client_id, status) {
        let locked_clients = clients.read().await;
        broadcast_participants(room, &locked_clients);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::build_participants;
    use crate::test_helpers;
    use crate::types::ClientMessageType;

    fn status_msg(status: &str) -> IncomingMessage {
        IncomingMessage {
            msg_type: ClientMessageType::ClientStatus,
            room: Some("room-1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "status": status })),
            ts: 0,
            server_ts: None,
        }
    }

    /// Host plus one guest in "room-1"; returns (host rx, guest rx).
    async fn setup(
        clients: &Clients,
        rooms: &Rooms,
    ) -> (crate::types::ClientReceiver, crate::types::ClientReceiver) {
        let mut lc = clients.write().await;
        let mut lr = rooms.write().await;
        let host_rx = test_helpers::setup_room_with_host(&mut lc, &mut lr, "host");
        let (mut guest, guest_rx) = test_helpers::create_client_with_rx("guest", "Guest", true);
        guest.room_id = Some("room-1".to_string());
        lc.insert("guest".to_string(), guest);
        lr.get_mut("room-1")
            .unwrap()
            .clients
            .push("guest".to_string());
        (host_rx, guest_rx)
    }

    #[test]
    fn apply_status_ignores_non_members_and_repeats() {
        let mut room = test_helpers::create_room("r1", "host");
        assert!(!apply_status(&mut room, "stranger", "synced"));
        assert!(apply_status(&mut room, "host", "playing"));
        assert!(!apply_status(&mut room, "host", "playing"));
        assert!(apply_status(&mut room, "host", "paused"));
    }

    #[test]
    fn participants_list_names_host_and_statuses() {
        let mut room = test_helpers::create_room("r1", "host");
        room.clients.push("guest".to_string());
        room.clients.push("ghost".to_string());
        room.client_status
            .insert("guest".to_string(), "syncing".to_string());
        let mut clients = std::collections::HashMap::new();
        let (host, _rx1) = test_helpers::create_client_with_rx("host", "Alice", true);
        let (guest, _rx2) = test_helpers::create_client_with_rx("guest", "Bob", true);
        clients.insert("host".to_string(), host);
        clients.insert("guest".to_string(), guest);

        let list = build_participants(&room, &clients);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["name"], "Alice");
        assert_eq!(list[0]["is_host"], true);
        assert_eq!(list[0]["status"], "unknown");
        assert_eq!(list[1]["name"], "Bob");
        assert_eq!(list[1]["is_host"], false);
        assert_eq!(list[1]["status"], "syncing");
        // A member whose connection is already gone still appears.
        assert_eq!(list[2]["name"], "Someone");
    }

    #[tokio::test]
    async fn status_change_is_broadcast_to_the_room() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = setup(&clients, &rooms).await;

        handle_client_status("guest", &status_msg("buffering"), &clients, &rooms).await;

        for rx in [&mut host_rx, &mut guest_rx] {
            let msg = test_helpers::recv_msg(rx).expect("participants broadcast");
            assert_eq!(msg.msg_type, "participants");
            let list = msg.payload.unwrap()["participants"].clone();
            assert_eq!(list[1]["id"], "guest");
            assert_eq!(list[1]["status"], "buffering");
        }
    }

    #[tokio::test]
    async fn unknown_status_is_ignored() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, _guest_rx) = setup(&clients, &rooms).await;

        handle_client_status("guest", &status_msg("<script>"), &clients, &rooms).await;

        assert!(test_helpers::recv_msg(&mut host_rx).is_none());
        assert!(rooms
            .read()
            .await
            .get("room-1")
            .unwrap()
            .client_status
            .is_empty());
    }

    #[tokio::test]
    async fn repeated_status_is_not_rebroadcast() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, _guest_rx) = setup(&clients, &rooms).await;

        handle_client_status("guest", &status_msg("synced"), &clients, &rooms).await;
        handle_client_status("guest", &status_msg("synced"), &clients, &rooms).await;

        assert!(test_helpers::recv_msg(&mut host_rx).is_some());
        assert!(test_helpers::recv_msg(&mut host_rx).is_none());
    }

    #[tokio::test]
    async fn leaving_clears_the_status_and_updates_the_list() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, _guest_rx) = setup(&clients, &rooms).await;
        handle_client_status("guest", &status_msg("synced"), &clients, &rooms).await;
        let _ = test_helpers::recv_msg(&mut host_rx);

        {
            let mut lc = clients.write().await;
            let mut lr = rooms.write().await;
            crate::room::handle_leave("guest", &mut lc, &mut lr);
        }

        assert!(!rooms
            .read()
            .await
            .get("room-1")
            .unwrap()
            .client_status
            .contains_key("guest"));
        let first = test_helpers::recv_msg(&mut host_rx).unwrap();
        assert_eq!(first.msg_type, "client_left");
        let second = test_helpers::recv_msg(&mut host_rx).unwrap();
        assert_eq!(second.msg_type, "participants");
        assert_eq!(
            second.payload.unwrap()["participants"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
