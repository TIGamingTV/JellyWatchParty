//! Room operations shared by the websocket handlers and the admin API.
//!
//! Everything here works on already-locked maps. Callers must take the locks
//! in the server-wide order - `rooms` first, then `clients` - and call
//! `messaging::broadcast_room_list` after releasing them when the room list
//! changed.

use super::leave::{announce_host, handle_leave};
use crate::messaging::{
    broadcast_participants, broadcast_to_room, build_room_state_payload, send_to_client,
};
use crate::password::hash_password;
use crate::types::{Client, PlaybackState, Room, WsMessage};
use crate::utils::now_ms;
use log::info;
use std::collections::{HashMap, HashSet, VecDeque};

/// Max members per room; shared with the `join_room` handler.
pub const MAX_CLIENTS_PER_ROOM: usize = 20;
/// Max length of a room name.
pub const MAX_ROOM_NAME_LENGTH: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpError {
    RoomNotFound,
    ClientNotFound,
    NotAMember,
    RoomFull,
    InvalidName,
}

impl OpError {
    pub fn message(self) -> &'static str {
        match self {
            OpError::RoomNotFound => "Room not found",
            OpError::ClientNotFound => "Client not found",
            OpError::NotAMember => "Not a member of this room",
            OpError::RoomFull => "Room is full",
            OpError::InvalidName => "Invalid name",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AddOptions {
    /// Put there by an admin: the client is told (`admin_moved`) so it can
    /// explain why it suddenly switched rooms.
    pub by_admin: bool,
    /// Make the new member the host if the room has none.
    pub promote_if_hostless: bool,
}

type Clients = HashMap<String, Client>;
type Rooms = HashMap<String, Room>;

/// Puts `client_id` into `room_id`, first taking it out of any other room it
/// is in (with the usual leave notifications there). No password check -
/// callers decide whether one applies.
///
/// The joiner gets `room_state`; everyone else gets `participants_update`,
/// and everyone gets the new `participants` list.
pub fn add_member(
    room_id: &str,
    client_id: &str,
    rooms: &mut Rooms,
    clients: &mut Clients,
    opts: AddOptions,
) -> Result<(), OpError> {
    let room = rooms.get(room_id).ok_or(OpError::RoomNotFound)?;
    let client = clients.get(client_id).ok_or(OpError::ClientNotFound)?;
    let already_member = room.clients.iter().any(|id| id == client_id);

    if already_member && opts.by_admin {
        return Ok(());
    }
    if !already_member && room.clients.len() >= MAX_CLIENTS_PER_ROOM {
        return Err(OpError::RoomFull);
    }
    if client.room_id.as_deref().is_some_and(|r| r != room_id) {
        handle_leave(client_id, clients, rooms);
    }

    let room = rooms.get_mut(room_id).ok_or(OpError::RoomNotFound)?;
    if !already_member {
        room.clients.push(client_id.to_string());
    }
    room.ready_clients.remove(client_id);
    if let Some(client) = clients.get_mut(client_id) {
        client.room_id = Some(room_id.to_string());
    }

    let promoted = opts.promote_if_hostless && room.is_hostless();
    if promoted {
        room.host_id = client_id.to_string();
        room.ready_clients.insert(client_id.to_string());
    }

    info!(
        "Client {} {} room {}{}",
        client_id,
        if opts.by_admin {
            "added by an admin to"
        } else {
            "joining"
        },
        room_id,
        if promoted { " as host" } else { "" }
    );

    let mut payload = build_room_state_payload(room, room.clients.len());
    if opts.by_admin {
        payload["admin_moved"] = serde_json::json!(true);
    }
    send_to_client(
        client_id,
        clients,
        &WsMessage {
            msg_type: "room_state".to_string(),
            room: Some(room_id.to_string()),
            client: Some(client_id.to_string()),
            payload: Some(payload),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
    if promoted && room.clients.len() > 1 {
        // Members that were already waiting in a hostless room learn who
        // drives playback now.
        announce_host(room_id, room, clients, Some(client_id));
    }
    broadcast_to_room(
        room,
        clients,
        &WsMessage {
            msg_type: "participants_update".to_string(),
            room: Some(room_id.to_string()),
            client: None,
            payload: Some(serde_json::json!({ "participant_count": room.clients.len() })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
        Some(client_id),
    );
    broadcast_participants(room, clients);
    Ok(())
}

/// Removes a member on an admin's behalf. The removed client is told with a
/// `room_closed` carrying `reason`; the rest of the room sees a normal leave
/// (including host promotion, or the room closing if it is now empty).
pub fn kick_member(
    room_id: &str,
    client_id: &str,
    reason: &str,
    rooms: &mut Rooms,
    clients: &mut Clients,
) -> Result<(), OpError> {
    let room = rooms.get(room_id).ok_or(OpError::RoomNotFound)?;
    if !room.clients.iter().any(|id| id == client_id) {
        return Err(OpError::NotAMember);
    }
    info!(
        "Removing client {} from room {} ({})",
        client_id, room_id, reason
    );
    handle_leave(client_id, clients, rooms);
    send_to_client(
        client_id,
        clients,
        &WsMessage {
            msg_type: "room_closed".to_string(),
            room: Some(room_id.to_string()),
            client: None,
            payload: Some(serde_json::json!({ "reason": reason, "removed": true })),
            ts: now_ms(),
            server_ts: Some(now_ms()),
        },
    );
    Ok(())
}

/// Hands the host role to `client_id`, who must already be a member.
/// Everyone gets `host_changed` and the updated participant list. A pending
/// (not yet started) play from the old host is dropped.
pub fn set_host(
    room_id: &str,
    client_id: &str,
    rooms: &mut Rooms,
    clients: &Clients,
) -> Result<(), OpError> {
    let room = rooms.get_mut(room_id).ok_or(OpError::RoomNotFound)?;
    if !room.clients.iter().any(|id| id == client_id) {
        return Err(OpError::NotAMember);
    }
    if room.host_id == client_id {
        return Ok(());
    }
    info!(
        "Host of room {} changed from {} to {}",
        room_id, room.host_id, client_id
    );
    room.host_id = client_id.to_string();
    room.pending_play = None;
    announce_host(room_id, room, clients, None);
    broadcast_participants(room, clients);
    Ok(())
}

/// Closes a room, telling every member why. Returns the ids that were in it.
pub fn close_room(
    room_id: &str,
    reason: &str,
    rooms: &mut Rooms,
    clients: &mut Clients,
) -> Result<Vec<String>, OpError> {
    let room = rooms.remove(room_id).ok_or(OpError::RoomNotFound)?;
    info!("Closing room {} ({})", room_id, reason);
    let msg = WsMessage {
        msg_type: "room_closed".to_string(),
        room: Some(room_id.to_string()),
        client: None,
        payload: Some(serde_json::json!({ "reason": reason })),
        ts: now_ms(),
        server_ts: Some(now_ms()),
    };
    for cid in &room.clients {
        send_to_client(cid, clients, &msg);
        if let Some(client) = clients.get_mut(cid) {
            if client.room_id.as_deref() == Some(room_id) {
                client.room_id = None;
            }
        }
    }
    Ok(room.clients)
}

/// Trims and validates a room name.
pub fn clean_room_name(raw: &str) -> Result<String, OpError> {
    let name: String = raw
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ROOM_NAME_LENGTH)
        .collect();
    if name.is_empty() {
        Err(OpError::InvalidName)
    } else {
        Ok(name)
    }
}

/// Creates an empty, hostless group from the admin panel. Its first member
/// becomes host; an empty password means anyone may join.
pub fn create_group(
    name: &str,
    password: Option<&str>,
    rooms: &mut Rooms,
) -> Result<String, OpError> {
    let name = clean_room_name(name)?;
    let room_id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let room = Room {
        room_id: room_id.clone(),
        name,
        host_id: String::new(),
        media_id: None,
        clients: Vec::new(),
        ready_clients: HashSet::new(),
        pending_play: None,
        state: PlaybackState {
            position: 0.0,
            play_state: "paused".to_string(),
        },
        last_state_ts: now,
        last_command_ts: 0,
        chat_history: VecDeque::new(),
        password_hash: password.filter(|p| !p.is_empty()).map(hash_password),
        client_status: HashMap::new(),
        failed_joins: HashMap::new(),
        // Like the native Host Bridge, members of admin groups may not all
        // understand the start countdown, so plays go out straight away.
        started: true,
        admin_created: true,
        created_at: now,
    };
    info!(
        "Admin created group '{}' ({}) (has_password: {})",
        room.name,
        room_id,
        room.password_hash.is_some()
    );
    rooms.insert(room_id.clone(), room);
    Ok(room_id)
}

/// Renames a room and/or sets (`Some(Some(pw))`), clears (`Some(None)` or
/// an empty password) or keeps (`None`) its password. Existing members are
/// unaffected by a password change.
pub fn update_room(
    room_id: &str,
    name: Option<&str>,
    password: Option<Option<&str>>,
    rooms: &mut Rooms,
) -> Result<(), OpError> {
    let new_name = name.map(clean_room_name).transpose()?;
    let room = rooms.get_mut(room_id).ok_or(OpError::RoomNotFound)?;
    if let Some(n) = new_name {
        room.name = n;
    }
    if let Some(pw) = password {
        room.password_hash = pw.filter(|p| !p.is_empty()).map(hash_password);
        room.failed_joins.clear();
    }
    info!(
        "Admin updated room {} (has_password: {})",
        room_id,
        room.password_hash.is_some()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{self, recv_msg};
    use crate::types::ClientReceiver;

    fn drain(rx: &mut ClientReceiver) -> Vec<WsMessage> {
        std::iter::from_fn(|| recv_msg(rx)).collect()
    }

    fn types(rx: &mut ClientReceiver) -> Vec<String> {
        drain(rx).into_iter().map(|m| m.msg_type).collect()
    }

    fn add_client(clients: &mut Clients, id: &str) -> ClientReceiver {
        let (c, rx) = test_helpers::create_client_with_rx(id, id, true);
        clients.insert(id.to_string(), c);
        rx
    }

    #[test]
    fn first_member_of_a_hostless_group_becomes_host() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let mut rx = add_client(&mut clients, "a");
        let id = create_group("Movie night", Some("pw"), &mut rooms).unwrap();
        assert!(rooms[&id].is_hostless());
        assert!(rooms[&id].admin_created);

        let opts = AddOptions {
            by_admin: true,
            promote_if_hostless: true,
        };
        add_member(&id, "a", &mut rooms, &mut clients, opts).unwrap();

        assert_eq!(rooms[&id].host_id, "a");
        assert_eq!(clients["a"].room_id.as_deref(), Some(id.as_str()));
        let msgs = drain(&mut rx);
        assert_eq!(msgs[0].msg_type, "room_state");
        let p = msgs[0].payload.as_ref().unwrap();
        assert_eq!(p["admin_moved"], true);
        assert_eq!(p["host_id"], "a");
    }

    #[test]
    fn member_added_without_promotion_leaves_room_hostless_until_one_joins() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let _ra = add_client(&mut clients, "a");
        let mut rb = add_client(&mut clients, "b");
        let id = create_group("G", None, &mut rooms).unwrap();

        add_member(&id, "b", &mut rooms, &mut clients, AddOptions::default()).unwrap();
        assert!(rooms[&id].is_hostless());
        drain(&mut rb);

        let opts = AddOptions {
            by_admin: false,
            promote_if_hostless: true,
        };
        add_member(&id, "a", &mut rooms, &mut clients, opts).unwrap();
        assert_eq!(rooms[&id].host_id, "a");
        // The member that was already waiting is told who the host is now.
        assert!(types(&mut rb).contains(&"host_changed".to_string()));
    }

    #[test]
    fn moving_a_member_leaves_its_old_room_properly() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let _rh = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host");
        let mut rg = add_client(&mut clients, "guest");
        let opts = AddOptions {
            by_admin: false,
            promote_if_hostless: true,
        };
        add_member("room-1", "guest", &mut rooms, &mut clients, opts).unwrap();
        let target = create_group("Other", None, &mut rooms).unwrap();
        drain(&mut rg);

        let opts = AddOptions {
            by_admin: true,
            promote_if_hostless: true,
        };
        add_member(&target, "host", &mut rooms, &mut clients, opts).unwrap();

        // The old room no longer lists the mover and promoted the guest.
        assert_eq!(rooms["room-1"].clients, vec!["guest".to_string()]);
        assert_eq!(rooms["room-1"].host_id, "guest");
        assert_eq!(rooms[&target].host_id, "host");
        assert!(types(&mut rg).contains(&"host_changed".to_string()));
    }

    #[test]
    fn add_member_respects_the_room_cap() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let id = create_group("Full", None, &mut rooms).unwrap();
        rooms.get_mut(&id).unwrap().clients =
            (0..MAX_CLIENTS_PER_ROOM).map(|i| format!("x{i}")).collect();
        let _r = add_client(&mut clients, "late");
        assert_eq!(
            add_member(&id, "late", &mut rooms, &mut clients, AddOptions::default()),
            Err(OpError::RoomFull)
        );
    }

    #[test]
    fn add_member_errors() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let _r = add_client(&mut clients, "a");
        assert_eq!(
            add_member("nope", "a", &mut rooms, &mut clients, AddOptions::default()),
            Err(OpError::RoomNotFound)
        );
        let id = create_group("G", None, &mut rooms).unwrap();
        assert_eq!(
            add_member(
                &id,
                "ghost",
                &mut rooms,
                &mut clients,
                AddOptions::default()
            ),
            Err(OpError::ClientNotFound)
        );
    }

    #[test]
    fn set_host_moves_the_role_and_tells_everyone() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let mut rh = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host");
        let mut rg = add_client(&mut clients, "guest");
        add_member(
            "room-1",
            "guest",
            &mut rooms,
            &mut clients,
            AddOptions::default(),
        )
        .unwrap();
        rooms.get_mut("room-1").unwrap().pending_play = Some(crate::types::PendingPlay {
            position: 1.0,
            created_at: 1,
        });
        drain(&mut rh);
        drain(&mut rg);

        set_host("room-1", "guest", &mut rooms, &clients).unwrap();

        assert_eq!(rooms["room-1"].host_id, "guest");
        assert!(rooms["room-1"].pending_play.is_none());
        for rx in [&mut rh, &mut rg] {
            let msgs = drain(rx);
            assert_eq!(msgs[0].msg_type, "host_changed");
            assert_eq!(msgs[0].payload.as_ref().unwrap()["host_id"], "guest");
            assert_eq!(msgs[1].msg_type, "participants");
        }
        assert_eq!(
            set_host("room-1", "stranger", &mut rooms, &clients),
            Err(OpError::NotAMember)
        );
    }

    #[test]
    fn kick_tells_the_member_and_promotes_a_new_host() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let mut rh = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host");
        let mut rg = add_client(&mut clients, "guest");
        add_member(
            "room-1",
            "guest",
            &mut rooms,
            &mut clients,
            AddOptions::default(),
        )
        .unwrap();
        drain(&mut rh);
        drain(&mut rg);

        kick_member(
            "room-1",
            "host",
            "Removed by an admin",
            &mut rooms,
            &mut clients,
        )
        .unwrap();

        let last = drain(&mut rh).pop().unwrap();
        assert_eq!(last.msg_type, "room_closed");
        assert_eq!(last.payload.unwrap()["reason"], "Removed by an admin");
        assert!(clients["host"].room_id.is_none());
        assert_eq!(rooms["room-1"].host_id, "guest");
        assert_eq!(
            kick_member("room-1", "host", "x", &mut rooms, &mut clients),
            Err(OpError::NotAMember)
        );
    }

    #[test]
    fn kicking_the_last_member_closes_the_room() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let _rh = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host");
        kick_member("room-1", "host", "bye", &mut rooms, &mut clients).unwrap();
        assert!(!rooms.contains_key("room-1"));
    }

    #[test]
    fn close_room_notifies_and_detaches_members() {
        let mut rooms = HashMap::new();
        let mut clients = HashMap::new();
        let mut rh = test_helpers::setup_room_with_host(&mut clients, &mut rooms, "host");
        let ids = close_room("room-1", "Closed by an admin", &mut rooms, &mut clients).unwrap();
        assert_eq!(ids, vec!["host".to_string()]);
        assert!(rooms.is_empty());
        assert!(clients["host"].room_id.is_none());
        let msg = recv_msg(&mut rh).unwrap();
        assert_eq!(msg.msg_type, "room_closed");
        assert_eq!(msg.payload.unwrap()["reason"], "Closed by an admin");
        assert_eq!(
            close_room("room-1", "x", &mut rooms, &mut clients),
            Err(OpError::RoomNotFound)
        );
    }

    #[test]
    fn update_room_renames_and_changes_password() {
        let mut rooms = HashMap::new();
        let id = create_group("Old", Some("a"), &mut rooms).unwrap();
        rooms
            .get_mut(&id)
            .unwrap()
            .failed_joins
            .insert("u".into(), (5, 1));

        update_room(&id, Some("  New  "), Some(Some("b")), &mut rooms).unwrap();
        let room = &rooms[&id];
        assert_eq!(room.name, "New");
        let (salt, hash) = room.password_hash.clone().unwrap();
        assert!(crate::password::verify_password("b", &salt, &hash));
        assert!(room.failed_joins.is_empty());

        update_room(&id, None, Some(None), &mut rooms).unwrap();
        assert!(rooms[&id].password_hash.is_none());
        update_room(&id, None, Some(Some("")), &mut rooms).unwrap();
        assert!(rooms[&id].password_hash.is_none());

        assert_eq!(
            update_room(&id, Some("   "), None, &mut rooms),
            Err(OpError::InvalidName)
        );
    }

    #[test]
    fn clean_room_name_strips_controls_and_truncates() {
        assert_eq!(clean_room_name(" a\u{7}b ").unwrap(), "ab");
        assert_eq!(
            clean_room_name(&"x".repeat(500)).unwrap().len(),
            MAX_ROOM_NAME_LENGTH
        );
        assert_eq!(clean_room_name(""), Err(OpError::InvalidName));
    }
}
