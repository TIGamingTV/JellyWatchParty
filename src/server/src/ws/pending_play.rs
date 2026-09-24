use super::constants::{PLAY_SCHEDULE_MS, START_COUNTDOWN_MS};
use crate::messaging::broadcast_to_room;
use crate::types::{Client, Clients, Room, Rooms, WsMessage};
use crate::utils::now_ms;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

pub(super) fn all_ready(room: &Room) -> bool {
    room.ready_clients.len() >= room.clients.len()
}

/// Tells everyone in the room (host included) to start playing `position` at
/// a shared server time. The room's first play, with others in the room,
/// gets a countdown: clients show it and all start together when it ends.
pub(super) fn start_scheduled_play(
    room: &mut Room,
    clients: &HashMap<String, Client>,
    position: f64,
    current_ts: u64,
) {
    let countdown = !room.started && room.clients.len() > 1;
    let target_server_ts = current_ts
        + if countdown {
            START_COUNTDOWN_MS
        } else {
            PLAY_SCHEDULE_MS
        };
    room.started = true;
    room.pending_play = None;
    room.state.position = position;
    room.state.play_state = "playing".to_string();
    let msg = WsMessage {
        msg_type: "player_event".to_string(),
        room: Some(room.room_id.clone()),
        client: None,
        payload: Some(serde_json::json!({
            "action": "play",
            "position": position,
            "target_server_ts": target_server_ts,
            "countdown": countdown
        })),
        ts: current_ts,
        server_ts: Some(target_server_ts),
    };
    broadcast_to_room(room, clients, &msg, None);
}

pub(super) async fn broadcast_scheduled_play(room: &mut Room, clients: &Clients, position: f64) {
    let locked_clients = clients.read().await;
    start_scheduled_play(room, &locked_clients, position, now_ms());
}

/// The room's first play is on hold until everyone is ready: tell everyone
/// (so they can show "waiting for everyone") and for at most how long.
pub(super) fn broadcast_start_pending(
    room: &Room,
    clients: &HashMap<String, Client>,
    wait_ms: u64,
) {
    let msg = WsMessage {
        msg_type: "start_pending".to_string(),
        room: Some(room.room_id.clone()),
        client: None,
        payload: Some(serde_json::json!({ "timeout_ms": wait_ms })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    broadcast_to_room(room, clients, &msg, None);
}

pub(super) fn schedule_pending_play(
    room_id: String,
    created_at: u64,
    wait_ms: u64,
    clients: Clients,
    rooms: Rooms,
) {
    tokio::spawn(async move {
        sleep(Duration::from_millis(wait_ms)).await;
        let mut locked_rooms = rooms.write().await;
        if let Some(room) = locked_rooms.get_mut(&room_id) {
            let pending = match room.pending_play.clone() {
                Some(pending) if pending.created_at == created_at => pending,
                _ => return,
            };
            broadcast_scheduled_play(room, &clients, pending.position).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;
    use std::collections::HashSet;

    #[test]
    fn all_ready_true() {
        let mut room = test_helpers::create_room("r1", "host");
        room.clients = vec!["host".to_string(), "guest".to_string()];
        room.ready_clients = HashSet::from(["host".to_string(), "guest".to_string()]);
        assert!(all_ready(&room));
    }

    #[test]
    fn all_ready_false() {
        let mut room = test_helpers::create_room("r1", "host");
        room.clients = vec!["host".to_string(), "guest".to_string()];
        room.ready_clients = HashSet::from(["host".to_string()]);
        assert!(!all_ready(&room));
    }

    fn guest_rx(
        clients: &mut HashMap<String, Client>,
        room: &mut Room,
    ) -> crate::types::ClientReceiver {
        let (guest, rx) = test_helpers::create_client_with_rx("guest", "Guest", true);
        clients.insert("guest".to_string(), guest);
        room.clients.push("guest".to_string());
        rx
    }

    #[test]
    fn first_play_with_others_counts_down_for_everyone() {
        let mut clients = HashMap::new();
        let (host, mut host_rx) = test_helpers::create_client_with_rx("host", "Host", true);
        clients.insert("host".to_string(), host);
        let mut room = test_helpers::create_room("r1", "host");
        room.started = false;
        let mut g_rx = guest_rx(&mut clients, &mut room);

        start_scheduled_play(&mut room, &clients, 12.0, 1_000);

        assert!(room.started);
        assert_eq!(room.state.play_state, "playing");
        for rx in [&mut host_rx, &mut g_rx] {
            let msg = test_helpers::recv_msg(rx).expect("host and guest get the play");
            let p = msg.payload.unwrap();
            assert_eq!(p["action"], "play");
            assert_eq!(p["countdown"], true);
            assert_eq!(p["position"], 12.0);
            assert_eq!(p["target_server_ts"], 1_000 + START_COUNTDOWN_MS);
        }
    }

    #[test]
    fn later_plays_and_solo_rooms_skip_the_countdown() {
        let mut clients = HashMap::new();
        let (host, mut host_rx) = test_helpers::create_client_with_rx("host", "Host", true);
        clients.insert("host".to_string(), host);

        // Alone in a fresh room: starts without a countdown.
        let mut solo = test_helpers::create_room("r1", "host");
        solo.started = false;
        start_scheduled_play(&mut solo, &clients, 0.0, 1_000);
        let p = test_helpers::recv_msg(&mut host_rx)
            .unwrap()
            .payload
            .unwrap();
        assert_eq!(p["countdown"], false);
        assert_eq!(p["target_server_ts"], 1_000 + PLAY_SCHEDULE_MS);
        assert!(solo.started);

        // Already started, with a guest: a normal scheduled play.
        let mut room = test_helpers::create_room("r2", "host");
        let _g = guest_rx(&mut clients, &mut room);
        start_scheduled_play(&mut room, &clients, 5.0, 2_000);
        let p = test_helpers::recv_msg(&mut host_rx)
            .unwrap()
            .payload
            .unwrap();
        assert_eq!(p["countdown"], false);
        assert_eq!(p["target_server_ts"], 2_000 + PLAY_SCHEDULE_MS);
    }

    #[test]
    fn start_pending_reports_the_wait() {
        let mut clients = HashMap::new();
        let (host, mut host_rx) = test_helpers::create_client_with_rx("host", "Host", true);
        clients.insert("host".to_string(), host);
        let room = test_helpers::create_room("r1", "host");
        broadcast_start_pending(&room, &clients, 10_000);
        let msg = test_helpers::recv_msg(&mut host_rx).unwrap();
        assert_eq!(msg.msg_type, "start_pending");
        assert_eq!(msg.payload.unwrap()["timeout_ms"], 10_000);
    }

    #[test]
    fn all_ready_empty_room() {
        let mut room = test_helpers::create_room("r1", "host");
        room.clients.clear();
        room.ready_clients.clear();
        assert!(all_ready(&room));
    }
}
