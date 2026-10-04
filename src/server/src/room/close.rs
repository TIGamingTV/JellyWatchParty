use crate::messaging::broadcast_room_list;
use crate::types::{Clients, Rooms};

/// Closes `room_id` (if it still exists), tells its members why, and
/// broadcasts the new room list. Takes the locks in the usual rooms ->
/// clients order.
pub async fn close_room(room_id: &str, reason: &str, clients: &Clients, rooms: &Rooms) {
    let closed = {
        let mut locked_rooms = rooms.write().await;
        let mut locked_clients = clients.write().await;
        super::ops::close_room(room_id, reason, &mut locked_rooms, &mut locked_clients).is_ok()
    };
    if closed {
        broadcast_room_list(clients, rooms).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[tokio::test]
    async fn close_room_clears_members_and_ignores_other_rooms() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        {
            let mut lc = clients.write().await;
            let mut lr = rooms.write().await;
            let _rx = test_helpers::setup_room_with_host(&mut lc, &mut lr, "host");
            let (mut other, _rx2) = test_helpers::create_client_with_rx("u2", "B", true);
            other.room_id = Some("room-2".to_string());
            lc.insert("other".to_string(), other);
        }

        close_room("room-1", "Host started a new room", &clients, &rooms).await;

        let lc = clients.read().await;
        assert!(lc.get("host").unwrap().room_id.is_none());
        assert_eq!(lc.get("other").unwrap().room_id.as_deref(), Some("room-2"));
        assert!(rooms.read().await.is_empty());
    }

    #[tokio::test]
    async fn close_room_unknown_is_a_noop() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        close_room("missing", "x", &clients, &rooms).await;
    }
}
