use super::super::constants::MAX_POSITION_SECONDS;
use super::super::constants::{
    COMMAND_COOLDOWN_MS, CONTROL_SCHEDULE_MS, MAX_READY_WAIT_MS, MIN_STATE_UPDATE_INTERVAL_MS,
    PLAY_SCHEDULE_MS, POSITION_JITTER_THRESHOLD, START_READY_WAIT_MS,
};
use super::super::pending_play::{
    all_ready, broadcast_start_pending, schedule_pending_play, start_scheduled_play,
};
use super::super::validation::{is_valid_play_state, is_valid_position};
use crate::types::{
    Client, ClientKind, ClientMessageType, ClientSender, Clients, IncomingMessage, OutboundMessage,
    PendingPlay, Room, Rooms,
};
use crate::utils::now_ms;
use std::collections::HashMap;

/// A room position change bigger than this (seconds) counts as a change
/// (`Room::last_change_ts`), not as playing on.
const POSITION_CHANGE_SECS: f64 = 2.0;

/// Whether `client_id` may send this playback message to `room`.
///
/// The host may send anything. In a democratic room that has started, any
/// other member may send discrete `play`, `pause` and `seek` events if it
/// is a client that supports democratic mode (the injected web client), or
/// a bridged Jellyfin device while the room also allows device control.
/// Periodic `state_update`s, `buffering`, `set_media` and the room's first
/// play stay with the host: several sources of those would fight.
pub(crate) fn may_control(
    room: &Room,
    client_id: &str,
    client: Option<&Client>,
    msg_type: &ClientMessageType,
    action: Option<&str>,
) -> bool {
    if room.host_id == client_id {
        return true;
    }
    if !room.democratic || !room.started {
        return false;
    }
    if *msg_type != ClientMessageType::PlayerEvent
        || !matches!(action, Some("play" | "pause" | "seek"))
    {
        return false;
    }
    if !room.clients.iter().any(|id| id == client_id) {
        return false;
    }
    match client {
        Some(c) if c.kind == ClientKind::Bridge => room.device_control,
        // Plugin bridges stand in for a device and only follow or host.
        Some(c) => c.democratic_capable && c.bridge_device.is_none(),
        None => false,
    }
}

fn handle_play_not_ready(room: &mut Room, position: f64, current_ts: u64) -> Option<(String, u64)> {
    room.state.position = position;
    if let Some(pending) = room.pending_play.as_mut() {
        pending.position = position;
        None
    } else {
        room.pending_play = Some(PendingPlay {
            position,
            created_at: current_ts,
        });
        room.last_state_ts = current_ts;
        Some((room.room_id.clone(), current_ts))
    }
}

fn absorb_during_pending(
    room: &mut Room,
    parsed: &IncomingMessage,
    action: Option<&str>,
    current_ts: u64,
) -> bool {
    if room.pending_play.is_none() {
        return false;
    }
    let is_player_event = parsed.msg_type == ClientMessageType::PlayerEvent;

    if is_player_event && action == Some("pause") {
        return false;
    }

    let position = parsed
        .payload
        .as_ref()
        .and_then(|p| p.get("position"))
        .and_then(|v| v.as_f64())
        .filter(|pos| is_valid_position(*pos))
        .unwrap_or(room.state.position);
    room.state.position = position;
    if let Some(pending) = room.pending_play.as_mut() {
        pending.position = position;
    }
    room.last_state_ts = current_ts;
    true
}

fn should_process_state_update(
    room: &Room,
    payload: &serde_json::Value,
    current_ts: u64,
    sender: &str,
) -> bool {
    let in_command_cooldown =
        room.last_command_ts > 0 && current_ts - room.last_command_ts < COMMAND_COOLDOWN_MS;
    // Someone else just played, paused or seeked (democratic mode, or a chat
    // bot user): the host's player is still carrying it out, and its
    // updates from before that would undo it.
    if in_command_cooldown && room.last_command_by != sender {
        return false;
    }
    let new_pos = payload
        .get("position")
        .and_then(|v| v.as_f64())
        .unwrap_or(room.state.position);
    let new_play_state = payload
        .get("play_state")
        .and_then(|v| v.as_str())
        .unwrap_or(&room.state.play_state);

    if new_play_state != room.state.play_state {
        return true;
    }

    let pos_diff = new_pos - room.state.position;
    let too_frequent = current_ts - room.last_state_ts < MIN_STATE_UPDATE_INTERVAL_MS;
    let small_backward_jitter = (-2.0..-POSITION_JITTER_THRESHOLD).contains(&pos_diff);
    let small_forward_jitter = (0.0..POSITION_JITTER_THRESHOLD).contains(&pos_diff);

    !(in_command_cooldown || too_frequent || small_backward_jitter || small_forward_jitter)
}

fn apply_state_changes(
    room: &mut Room,
    parsed: &mut IncomingMessage,
    action: Option<&str>,
    current_ts: u64,
    sender: &str,
) {
    let was_playing = room.state.play_state.clone();
    let expected = room.expected_position(current_ts);
    if let Some(payload) = &parsed.payload {
        if let Some(pos) = payload.get("position").and_then(|v| v.as_f64()) {
            if is_valid_position(pos) {
                room.state.position = pos;
            }
        }
        if let Some(st) = payload.get("play_state").and_then(|v| v.as_str()) {
            if is_valid_play_state(st) {
                room.state.play_state = st.to_string();
            }
        }
        if parsed.msg_type == ClientMessageType::PlayerEvent {
            if let Some(action) = action {
                if action == "play" {
                    room.state.play_state = "playing".to_string();
                }
                if action == "pause" || action == "buffering" {
                    room.state.play_state = "paused".to_string();
                }
            }
        }
    }
    room.last_state_ts = current_ts;
    if parsed.msg_type == ClientMessageType::PlayerEvent
        || room.state.play_state != was_playing
        || (room.state.position - expected).abs() > POSITION_CHANGE_SECS
    {
        room.last_change_ts = current_ts;
    }

    if parsed.msg_type == ClientMessageType::PlayerEvent {
        room.last_command_ts = current_ts;
        room.last_command_by = sender.to_string();
        let schedule_delay = if action == Some("play") {
            PLAY_SCHEDULE_MS
        } else {
            CONTROL_SCHEDULE_MS
        };
        let target_server_ts = current_ts + schedule_delay;
        if let Some(payload) = parsed.payload.as_mut() {
            payload["target_server_ts"] = serde_json::json!(target_server_ts);
        }
        parsed.server_ts = Some(target_server_ts);
    } else {
        parsed.server_ts = Some(current_ts);
    }
}

pub(in crate::ws) async fn handle_playback(
    client_id: &str,
    mut parsed: IncomingMessage,
    clients: &Clients,
    rooms: &Rooms,
) {
    let Some(ref room_id) = parsed.room else {
        return;
    };

    let mut pending_schedule: Option<(String, u64, u64)> = None;
    let broadcast_data: Option<(Vec<ClientSender>, String)> = 'broadcast: {
        let mut locked_rooms = rooms.write().await;
        let locked_clients = clients.read().await;

        let Some(room) = locked_rooms.get_mut(room_id) else {
            break 'broadcast None;
        };

        let current_ts = now_ms();
        let is_player_event = parsed.msg_type == ClientMessageType::PlayerEvent;
        let action: Option<String> = if is_player_event {
            parsed
                .payload
                .as_ref()
                .and_then(|p| p.get("action"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };
        let sender = locked_clients.get(client_id);
        if !may_control(room, client_id, sender, &parsed.msg_type, action.as_deref()) {
            break 'broadcast None;
        }
        let by_name = sender.map(|c| c.user_name.clone()).unwrap_or_default();

        if is_player_event && action.as_deref() == Some("pause") {
            room.pending_play = None;
        }

        let play_position = || {
            parsed
                .payload
                .as_ref()
                .and_then(|p| p.get("position"))
                .and_then(|v| v.as_f64())
                .filter(|pos| is_valid_position(*pos))
        };

        // The room's first play: wait (longer) for everyone to be ready, then
        // everyone, host included, starts together after a countdown.
        if is_player_event && action.as_deref() == Some("play") && !room.started {
            let position = play_position().unwrap_or(room.state.position);
            if all_ready(room) {
                start_scheduled_play(room, &locked_clients, position, current_ts);
            } else if let Some((id, ts)) = handle_play_not_ready(room, position, current_ts) {
                broadcast_start_pending(room, &locked_clients, START_READY_WAIT_MS);
                pending_schedule = Some((id, ts, START_READY_WAIT_MS));
            }
            break 'broadcast None;
        }

        if is_player_event && action.as_deref() == Some("play") && !all_ready(room) {
            let position = play_position().unwrap_or(room.state.position);
            pending_schedule = handle_play_not_ready(room, position, current_ts)
                .map(|(id, ts)| (id, ts, MAX_READY_WAIT_MS));
            break 'broadcast None;
        }

        if absorb_during_pending(room, &parsed, action.as_deref(), current_ts) {
            break 'broadcast None;
        }

        // The host was already playing (e.g. created the room mid-movie): the
        // room has started, so a later pause/play gets no countdown.
        if parsed.msg_type == ClientMessageType::StateUpdate
            && parsed
                .payload
                .as_ref()
                .and_then(|p| p.get("play_state"))
                .and_then(|v| v.as_str())
                == Some("playing")
        {
            room.started = true;
        }

        if parsed.msg_type == ClientMessageType::StateUpdate {
            let should_process = parsed
                .payload
                .as_ref()
                .map(|p| should_process_state_update(room, p, current_ts, client_id))
                .unwrap_or(true);
            if !should_process {
                break 'broadcast None;
            }
        }

        apply_state_changes(room, &mut parsed, action.as_deref(), current_ts, client_id);
        if is_player_event {
            // Who did it, so everyone can be told ("Alice paused").
            if let Some(payload) = parsed.payload.as_mut() {
                payload["by"] = serde_json::json!(client_id);
                payload["by_name"] = serde_json::json!(by_name);
            }
        }

        let senders: Vec<_> = room
            .clients
            .iter()
            .filter(|id| *id != client_id)
            .filter_map(|id| locked_clients.get(id).map(|c| c.sender.clone()))
            .collect();

        match serde_json::to_string(&parsed) {
            Ok(json) => break 'broadcast Some((senders, json)),
            Err(e) => {
                log::error!("Failed to serialize message: {}", e);
                break 'broadcast None;
            }
        }
    };

    if let Some((senders, json)) = broadcast_data {
        let outbound = OutboundMessage::text(json);
        for sender in senders {
            if let Err(e) = sender.try_send(outbound.clone()) {
                log::warn!("Failed to send player event (buffer full or closed): {}", e);
            }
        }
    }
    if let Some((room_id, created_at, wait_ms)) = pending_schedule {
        schedule_pending_play(room_id, created_at, wait_ms, clients.clone(), rooms.clone());
    }
}

/// A play, pause or seek from outside the room's members.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoomCommand {
    Play,
    Pause,
    Seek(f64),
}

impl RoomCommand {
    pub fn action(self) -> &'static str {
        match self {
            RoomCommand::Play => "play",
            RoomCommand::Pause => "pause",
            RoomCommand::Seek(_) => "seek",
        }
    }
}

/// Carries out a command from outside the room (a chat bot user in a
/// democratic room) as if a member had sent it. It goes to every member,
/// host included, since none of them sent it. The caller checks that the
/// room is democratic and has started. Returns where playback goes.
pub fn server_command(
    room: &mut Room,
    clients: &HashMap<String, Client>,
    cmd: RoomCommand,
    by_name: &str,
    now: u64,
) -> f64 {
    let position = match cmd {
        RoomCommand::Seek(p) if p.is_nan() => 0.0,
        RoomCommand::Seek(p) => p.clamp(0.0, MAX_POSITION_SECONDS),
        _ => room.expected_position(now),
    };
    if cmd == RoomCommand::Pause {
        room.pending_play = None;
    }
    let mut parsed = IncomingMessage {
        msg_type: ClientMessageType::PlayerEvent,
        room: Some(room.room_id.clone()),
        client: None,
        payload: Some(serde_json::json!({
            "action": cmd.action(),
            "position": position,
        })),
        ts: now,
        server_ts: None,
    };
    apply_state_changes(room, &mut parsed, Some(cmd.action()), now, "");
    if let Some(payload) = parsed.payload.as_mut() {
        payload["by"] = serde_json::json!("");
        payload["by_name"] = serde_json::json!(by_name);
    }
    match serde_json::to_string(&parsed) {
        Ok(json) => {
            let outbound = OutboundMessage::text(json);
            for c in room.clients.iter().filter_map(|id| clients.get(id)) {
                if let Err(e) = c.sender.try_send(outbound.clone()) {
                    log::warn!("Failed to send player event (buffer full or closed): {}", e);
                }
            }
        }
        Err(e) => log::error!("Failed to serialize message: {}", e),
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    fn play_msg(position: f64) -> IncomingMessage {
        IncomingMessage {
            msg_type: ClientMessageType::PlayerEvent,
            room: Some("room-1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "action": "play", "position": position })),
            ts: 0,
            server_ts: None,
        }
    }

    /// Fresh (not started) room-1 with a host and a guest; returns their rx.
    async fn fresh_room_with_guest(
        clients: &Clients,
        rooms: &Rooms,
        guest_ready: bool,
    ) -> (crate::types::ClientReceiver, crate::types::ClientReceiver) {
        let mut lc = clients.write().await;
        let mut lr = rooms.write().await;
        let host_rx = test_helpers::setup_room_with_host(&mut lc, &mut lr, "host");
        let (mut guest, guest_rx) = test_helpers::create_client_with_rx("guest", "Guest", true);
        guest.room_id = Some("room-1".to_string());
        lc.insert("guest".to_string(), guest);
        let room = lr.get_mut("room-1").unwrap();
        room.started = false;
        room.clients.push("guest".to_string());
        if guest_ready {
            room.ready_clients.insert("guest".to_string());
        }
        (host_rx, guest_rx)
    }

    #[tokio::test]
    async fn first_play_waits_for_everyone_then_counts_down() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = fresh_room_with_guest(&clients, &rooms, false).await;

        handle_playback("host", play_msg(3.0), &clients, &rooms).await;

        // Guest not ready yet: nothing plays, everyone is told to wait.
        for rx in [&mut host_rx, &mut guest_rx] {
            let msg = test_helpers::recv_msg(rx).expect("start_pending");
            assert_eq!(msg.msg_type, "start_pending");
        }
        {
            let lr = rooms.read().await;
            let room = lr.get("room-1").unwrap();
            assert!(!room.started);
            assert_eq!(room.pending_play.as_ref().unwrap().position, 3.0);
        }

        // The guest reports ready: the countdown goes out to both.
        let ready = IncomingMessage {
            msg_type: ClientMessageType::Ready,
            room: Some("room-1".to_string()),
            client: None,
            payload: None,
            ts: 0,
            server_ts: None,
        };
        crate::ws::handlers::handle_ready("guest", &ready, &clients, &rooms).await;
        for rx in [&mut host_rx, &mut guest_rx] {
            let p = test_helpers::recv_msg(rx)
                .expect("countdown play")
                .payload
                .unwrap();
            assert_eq!(p["action"], "play");
            assert_eq!(p["countdown"], true);
            assert_eq!(p["position"], 3.0);
        }
        assert!(rooms.read().await.get("room-1").unwrap().started);
    }

    #[tokio::test]
    async fn first_play_with_everyone_ready_counts_down_at_once() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = fresh_room_with_guest(&clients, &rooms, true).await;

        handle_playback("host", play_msg(0.0), &clients, &rooms).await;

        for rx in [&mut host_rx, &mut guest_rx] {
            let p = test_helpers::recv_msg(rx)
                .expect("countdown play")
                .payload
                .unwrap();
            assert_eq!(p["countdown"], true);
        }
    }

    #[tokio::test]
    async fn a_host_already_playing_marks_the_room_started() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (_host_rx, _guest_rx) = fresh_room_with_guest(&clients, &rooms, true).await;
        let update = IncomingMessage {
            msg_type: ClientMessageType::StateUpdate,
            room: Some("room-1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "position": 600.0, "play_state": "playing" })),
            ts: 0,
            server_ts: None,
        };

        handle_playback("host", update, &clients, &rooms).await;

        assert!(rooms.read().await.get("room-1").unwrap().started);
    }

    #[tokio::test]
    async fn plays_after_the_start_behave_as_before() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = fresh_room_with_guest(&clients, &rooms, true).await;
        rooms.write().await.get_mut("room-1").unwrap().started = true;

        handle_playback("host", play_msg(8.0), &clients, &rooms).await;

        // Relayed to the guest only, with no countdown flag.
        let p = test_helpers::recv_msg(&mut guest_rx)
            .unwrap()
            .payload
            .unwrap();
        assert_eq!(p["action"], "play");
        assert!(p.get("countdown").is_none());
        assert!(test_helpers::recv_msg(&mut host_rx).is_none());
    }

    #[test]
    fn should_process_state_update_play_state_change() {
        let room = test_helpers::create_room("r1", "host");
        let payload = serde_json::json!({ "position": 0.0, "play_state": "playing" });
        assert!(should_process_state_update(
            &room,
            &payload,
            now_ms(),
            "host"
        ));
    }

    #[test]
    fn should_process_state_update_during_cooldown() {
        let mut room = test_helpers::create_room("r1", "host");
        let now = now_ms();
        room.last_command_ts = now; // Just issued a command
        room.last_state_ts = now;
        let payload = serde_json::json!({ "position": 0.1, "play_state": "paused" });
        assert!(!should_process_state_update(
            &room,
            &payload,
            now + 100,
            "host"
        ));
    }

    #[test]
    fn should_process_state_update_jitter() {
        let mut room = test_helpers::create_room("r1", "host");
        room.state.position = 10.0;
        room.last_state_ts = 0;
        let payload = serde_json::json!({ "position": 10.2, "play_state": "paused" });
        // 0.2 < POSITION_JITTER_THRESHOLD (0.5), should be filtered
        assert!(!should_process_state_update(
            &room,
            &payload,
            now_ms(),
            "host"
        ));
    }

    #[test]
    fn should_process_state_update_significant_move() {
        let mut room = test_helpers::create_room("r1", "host");
        room.state.position = 10.0;
        room.last_state_ts = 0;
        let payload = serde_json::json!({ "position": 15.0, "play_state": "paused" });
        assert!(should_process_state_update(
            &room,
            &payload,
            now_ms(),
            "host"
        ));
    }

    #[test]
    fn absorb_during_pending_state_update() {
        let mut room = test_helpers::create_room("r1", "host");
        room.pending_play = Some(PendingPlay {
            position: 5.0,
            created_at: now_ms(),
        });
        let parsed = IncomingMessage {
            msg_type: ClientMessageType::StateUpdate,
            room: Some("r1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "position": 6.0 })),
            ts: 0,
            server_ts: None,
        };
        assert!(absorb_during_pending(&mut room, &parsed, None, now_ms()));
    }

    #[test]
    fn absorb_during_pending_pause_not_absorbed() {
        let mut room = test_helpers::create_room("r1", "host");
        room.pending_play = Some(PendingPlay {
            position: 5.0,
            created_at: now_ms(),
        });
        let parsed = IncomingMessage {
            msg_type: ClientMessageType::PlayerEvent,
            room: Some("r1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "action": "pause" })),
            ts: 0,
            server_ts: None,
        };
        assert!(!absorb_during_pending(
            &mut room,
            &parsed,
            Some("pause"),
            now_ms()
        ));
    }

    #[test]
    fn absorb_no_pending() {
        let mut room = test_helpers::create_room("r1", "host");
        let parsed = IncomingMessage {
            msg_type: ClientMessageType::StateUpdate,
            room: Some("r1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "position": 6.0 })),
            ts: 0,
            server_ts: None,
        };
        assert!(!absorb_during_pending(&mut room, &parsed, None, now_ms()));
    }

    #[test]
    fn handle_play_not_ready_creates_pending() {
        let mut room = test_helpers::create_room("r1", "host");
        assert!(room.pending_play.is_none());
        let result = handle_play_not_ready(&mut room, 10.0, now_ms());
        assert!(result.is_some());
        assert!(room.pending_play.is_some());
        assert!((room.pending_play.as_ref().unwrap().position - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn handle_play_not_ready_existing_pending() {
        let mut room = test_helpers::create_room("r1", "host");
        room.pending_play = Some(PendingPlay {
            position: 5.0,
            created_at: now_ms(),
        });
        let result = handle_play_not_ready(&mut room, 15.0, now_ms());
        assert!(result.is_none()); // Returns None when pending already exists
        assert!((room.pending_play.as_ref().unwrap().position - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_state_changes_updates_room() {
        let mut room = test_helpers::create_room("r1", "host");
        let mut parsed = IncomingMessage {
            msg_type: ClientMessageType::StateUpdate,
            room: Some("r1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "position": 42.0, "play_state": "playing" })),
            ts: 0,
            server_ts: None,
        };
        let now = now_ms();
        apply_state_changes(&mut room, &mut parsed, None, now, "host");
        assert!((room.state.position - 42.0).abs() < f64::EPSILON);
        assert_eq!(room.state.play_state, "playing");
        assert_eq!(room.last_state_ts, now);
    }

    #[test]
    fn apply_state_changes_player_event_play() {
        let mut room = test_helpers::create_room("r1", "host");
        let mut parsed = IncomingMessage {
            msg_type: ClientMessageType::PlayerEvent,
            room: Some("r1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "action": "play", "position": 10.0 })),
            ts: 0,
            server_ts: None,
        };
        let now = now_ms();
        apply_state_changes(&mut room, &mut parsed, Some("play"), now, "host");
        assert_eq!(room.state.play_state, "playing");
        assert_eq!(room.last_command_ts, now);
        // server_ts should be set to target_server_ts
        assert!(parsed.server_ts.is_some());
        assert!(parsed.server_ts.unwrap() > now);
    }

    // --- democratic mode ---------------------------------------------------

    fn event(action: &str, position: f64) -> IncomingMessage {
        IncomingMessage {
            msg_type: ClientMessageType::PlayerEvent,
            room: Some("room-1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "action": action, "position": position })),
            ts: 0,
            server_ts: None,
        }
    }

    fn state(play_state: &str, position: f64) -> IncomingMessage {
        IncomingMessage {
            msg_type: ClientMessageType::StateUpdate,
            room: Some("room-1".to_string()),
            client: None,
            payload: Some(serde_json::json!({ "position": position, "play_state": play_state })),
            ts: 0,
            server_ts: None,
        }
    }

    /// Started democratic room-1: a host and a guest whose client supports
    /// democratic mode.
    async fn democratic_room(
        clients: &Clients,
        rooms: &Rooms,
    ) -> (crate::types::ClientReceiver, crate::types::ClientReceiver) {
        let (host_rx, guest_rx) = fresh_room_with_guest(clients, rooms, true).await;
        let mut lr = rooms.write().await;
        let room = lr.get_mut("room-1").unwrap();
        room.started = true;
        room.democratic = true;
        room.state.play_state = "playing".into();
        room.last_state_ts = now_ms();
        clients
            .write()
            .await
            .get_mut("guest")
            .unwrap()
            .democratic_capable = true;
        (host_rx, guest_rx)
    }

    #[tokio::test]
    async fn guests_control_a_democratic_room() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = democratic_room(&clients, &rooms).await;

        handle_playback("guest", event("pause", 42.0), &clients, &rooms).await;

        let msg = test_helpers::recv_msg(&mut host_rx).expect("the host follows the guest");
        let p = msg.payload.unwrap();
        assert_eq!(p["action"], "pause");
        assert_eq!(p["by"], "guest");
        assert_eq!(p["by_name"], "Guest");
        assert!(test_helpers::recv_msg(&mut guest_rx).is_none(), "no echo");
        {
            let lr = rooms.read().await;
            let room = &lr["room-1"];
            assert_eq!(room.state.play_state, "paused");
            assert_eq!(room.state.position, 42.0);
            assert_eq!(room.last_command_by, "guest");
        }

        // The host's update from before it paused doesn't undo the pause...
        handle_playback("host", state("playing", 41.0), &clients, &rooms).await;
        assert_eq!(rooms.read().await["room-1"].state.play_state, "paused");
        assert!(test_helpers::recv_msg(&mut guest_rx).is_none());
        // ...but the host can still pause, play and seek itself.
        handle_playback("host", event("play", 42.0), &clients, &rooms).await;
        assert_eq!(rooms.read().await["room-1"].state.play_state, "playing");
        assert_eq!(
            test_helpers::recv_msg(&mut guest_rx)
                .unwrap()
                .payload
                .unwrap()["by"],
            "host"
        );
    }

    #[tokio::test]
    async fn guests_only_send_discrete_commands() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, _guest_rx) = democratic_room(&clients, &rooms).await;

        for msg in [
            state("paused", 10.0),
            event("buffering", 10.0),
            event("rewind", 10.0),
        ] {
            handle_playback("guest", msg, &clients, &rooms).await;
            assert!(test_helpers::recv_msg(&mut host_rx).is_none());
        }
        handle_playback("guest", event("seek", 600.0), &clients, &rooms).await;
        assert_eq!(
            test_helpers::recv_msg(&mut host_rx)
                .unwrap()
                .payload
                .unwrap()["action"],
            "seek"
        );
    }

    #[tokio::test]
    async fn who_may_control_a_democratic_room() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (_h, _g) = democratic_room(&clients, &rooms).await;
        let lr = rooms.read().await;
        let lc = clients.read().await;
        let room = &lr["room-1"];
        let pause = ClientMessageType::PlayerEvent;
        let guest = lc.get("guest").unwrap();
        assert!(may_control(
            room,
            "guest",
            Some(guest),
            &pause,
            Some("pause")
        ));

        // A client that didn't announce support, a plugin bridge, and a
        // device bridge without device control may not.
        let mut old = guest.clone();
        old.democratic_capable = false;
        assert!(!may_control(
            room,
            "guest",
            Some(&old),
            &pause,
            Some("pause")
        ));
        let mut plugin = guest.clone();
        plugin.bridge_device = Some("dev".into());
        assert!(!may_control(
            room,
            "guest",
            Some(&plugin),
            &pause,
            Some("pause")
        ));
        let mut device = guest.clone();
        device.kind = ClientKind::Bridge;
        assert!(!may_control(
            room,
            "guest",
            Some(&device),
            &pause,
            Some("pause")
        ));
        let mut with_devices = room.clone();
        with_devices.device_control = true;
        assert!(may_control(
            &with_devices,
            "guest",
            Some(&device),
            &pause,
            Some("pause")
        ));

        // Not a member, or the room hasn't started, or it isn't democratic.
        assert!(!may_control(
            room,
            "stranger",
            Some(guest),
            &pause,
            Some("pause")
        ));
        let mut fresh = room.clone();
        fresh.started = false;
        assert!(!may_control(
            &fresh,
            "guest",
            Some(guest),
            &pause,
            Some("pause")
        ));
        let mut host_only = room.clone();
        host_only.democratic = false;
        assert!(!may_control(
            &host_only,
            "guest",
            Some(guest),
            &pause,
            Some("pause")
        ));
        // The host may do anything.
        assert!(may_control(
            &host_only,
            "host",
            None,
            &ClientMessageType::StateUpdate,
            None
        ));
    }

    #[tokio::test]
    async fn server_commands_reach_everyone_and_hold_off_the_host() {
        let clients = test_helpers::create_clients();
        let rooms = test_helpers::create_rooms();
        let (mut host_rx, mut guest_rx) = democratic_room(&clients, &rooms).await;
        {
            let mut lr = rooms.write().await;
            let lc = clients.read().await;
            let room = lr.get_mut("room-1").unwrap();
            // One clock reading for both commands: the room is playing, so a
            // later `now` would advance the expected position past 0.
            let now = now_ms();
            let pos = server_command(room, &lc, RoomCommand::Seek(-5.0), "Bob", now);
            assert_eq!(pos, 0.0, "clamped");
            let pos = server_command(room, &lc, RoomCommand::Pause, "Bob", now);
            assert_eq!(pos, 0.0);
            assert_eq!(room.state.play_state, "paused");
            assert_eq!(room.last_command_by, "");
        }
        for rx in [&mut host_rx, &mut guest_rx] {
            let seek = test_helpers::recv_msg(rx).unwrap().payload.unwrap();
            assert_eq!(seek["action"], "seek");
            let pause = test_helpers::recv_msg(rx).unwrap().payload.unwrap();
            assert_eq!(pause["action"], "pause");
            assert_eq!(pause["by_name"], "Bob");
            assert!(pause["target_server_ts"].is_u64());
        }
        handle_playback("host", state("playing", 30.0), &clients, &rooms).await;
        assert_eq!(rooms.read().await["room-1"].state.play_state, "paused");
    }
}
