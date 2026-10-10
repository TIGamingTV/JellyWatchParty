//! Background loops: settings + heartbeat, the rooms long poll, and
//! keeping every room's panel message up to date (each edited at most once
//! per `MIN_EDIT_GAP`, only when what it shows changed).

use crate::api::Room;
use crate::handler::Shared;
use crate::panel::{self, PanelView};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use twilight_http::error::ErrorType;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};
use twilight_model::id::Id;

const MIN_EDIT_GAP: Duration = Duration::from_secs(2);
const CONFIG_EVERY: Duration = Duration::from_secs(30);
const RETRY_AFTER: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Msg {
    channel: u64,
    message: u64,
}

#[derive(Debug)]
struct Tracked {
    msg: Msg,
    name: String,
    last: Option<PanelView>,
    pending: Option<PanelView>,
    last_edit: Option<Instant>,
    /// The room is gone (or the panel moved): drop after the last edit.
    retire: bool,
}

/// Panel messages and what each last showed.
#[derive(Debug, Default)]
pub struct Panels {
    /// Keyed by message, so an old panel of a room can be closed while a
    /// newer one is kept up to date.
    tracked: HashMap<Msg, Tracked>,
}

fn parse_msg(channel: &str, message: &str) -> Option<Msg> {
    Some(Msg {
        channel: channel.parse().ok()?,
        message: message.parse().ok()?,
    })
}

impl Panels {
    /// A panel this bot just posted (already showing `view`).
    pub fn posted(&mut self, channel: u64, message: u64, name: &str, view: PanelView) {
        self.tracked.insert(
            Msg { channel, message },
            Tracked {
                msg: Msg { channel, message },
                name: name.to_string(),
                last: Some(view),
                pending: None,
                last_edit: Some(Instant::now()),
                retire: false,
            },
        );
    }

    /// Notes what every panel should show now.
    pub fn apply(&mut self, rooms: &[Room]) {
        let mut current: HashMap<Msg, &Room> = HashMap::new();
        for r in rooms {
            if let Some(p) = r
                .panel
                .as_ref()
                .and_then(|p| parse_msg(&p.channel_id, &p.message_id))
            {
                current.insert(p, r);
            }
        }
        for (msg, room) in &current {
            let view = panel::render(room);
            let t = self.tracked.entry(*msg).or_insert_with(|| Tracked {
                msg: *msg,
                name: room.name.clone(),
                last: None,
                pending: None,
                last_edit: None,
                retire: false,
            });
            t.name = room.name.clone();
            t.retire = false;
            if t.last.as_ref() != Some(&view) {
                t.pending = Some(view);
            } else {
                t.pending = None;
            }
        }
        for (msg, t) in self.tracked.iter_mut() {
            if !current.contains_key(msg) && !t.retire {
                let still_open = rooms.iter().any(|r| r.name == t.name && r.panel.is_some());
                let why = if still_open {
                    "This panel was replaced by a newer one."
                } else {
                    "This room is closed."
                };
                t.pending = Some(panel::render_closed(&t.name, why));
                t.retire = true;
            }
        }
    }

    /// Edits that are due now: `(channel, message, view, retiring)`. Marks
    /// them as done (see `retry` for when one fails).
    fn due(&mut self, now: Instant) -> Vec<(u64, u64, PanelView, bool)> {
        let mut out = Vec::new();
        for t in self.tracked.values_mut() {
            let ready = t
                .last_edit
                .is_none_or(|at| now.duration_since(at) >= MIN_EDIT_GAP);
            if ready {
                if let Some(v) = t.pending.take() {
                    t.last = Some(v.clone());
                    t.last_edit = Some(now);
                    out.push((t.msg.channel, t.msg.message, v, t.retire));
                }
            }
        }
        self.tracked
            .retain(|_, t| !(t.retire && t.pending.is_none()));
        out
    }

    fn forget(&mut self, channel: u64, message: u64) {
        self.tracked.remove(&Msg { channel, message });
    }

    /// An edit didn't go through: try `view` again later, unless something
    /// newer is already waiting. A final edit of a retired panel is kept
    /// until it goes through.
    fn retry(&mut self, channel: u64, message: u64, view: PanelView, retire: bool) {
        let msg = Msg { channel, message };
        let t = self.tracked.entry(msg).or_insert_with(|| Tracked {
            msg,
            name: String::new(),
            last: None,
            pending: None,
            last_edit: Some(Instant::now()),
            retire,
        });
        t.last = None;
        if t.pending.is_none() {
            t.pending = Some(view);
        }
    }
}

/// Discord says the message (or our access to it) is gone.
fn gone(e: &twilight_http::Error) -> bool {
    matches!(e.kind(), ErrorType::Response { status, .. } if matches!(status.get(), 403 | 404))
}

/// A failure that may well go away by itself (network, Discord trouble).
fn transient(e: &twilight_http::Error) -> bool {
    match e.kind() {
        ErrorType::Response { status, .. } => status.get() == 429 || status.get() >= 500,
        ErrorType::RequestError
        | ErrorType::RequestTimedOut
        | ErrorType::RequestCanceled
        | ErrorType::ServiceUnavailable { .. }
        | ErrorType::RatelimiterTicket
        | ErrorType::ChunkingResponse => true,
        _ => false,
    }
}

/// Edits due panels, every second.
pub async fn flush_loop(shared: Arc<Shared>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let due = shared.panels().due(Instant::now());
        for (channel, message, view, retiring) in due {
            let (Some(ch), Some(msg)) = (
                Id::<ChannelMarker>::new_checked(channel),
                Id::<MessageMarker>::new_checked(message),
            ) else {
                shared.panels().forget(channel, message);
                continue;
            };
            let embeds = [panel::embed(&view)];
            let components = panel::components(&view);
            let res = shared
                .http
                .update_message(ch, msg)
                .embeds(Some(&embeds))
                .components(Some(&components))
                .await;
            if let Err(e) = res {
                if gone(&e) {
                    log::info!(
                        "panel {} in {} is gone; no longer updating it",
                        message,
                        channel
                    );
                    shared.panels().forget(channel, message);
                } else if transient(&e) {
                    log::warn!("could not update panel {} (will retry): {}", message, e);
                    shared.panels().retry(channel, message, view, retiring);
                } else {
                    log::warn!("could not update panel {}: {}", message, e);
                }
            }
        }
    }
}

/// Follows the server's rooms (long poll) into the cache and the panels.
pub async fn rooms_loop(shared: Arc<Shared>) {
    let mut since = None;
    loop {
        match shared.api.rooms(since).await {
            Ok(r) => {
                since = Some(r.version);
                shared.panels().apply(&r.rooms);
                *shared.rooms.write().unwrap_or_else(|e| e.into_inner()) = r.rooms;
            }
            Err(e) => {
                log::warn!("room updates: {}", e);
                since = None;
                tokio::time::sleep(RETRY_AFTER).await;
            }
        }
    }
}

/// Fetches the settings (registering `/jwp` where they say) and checks in.
pub async fn config_loop(shared: Arc<Shared>) {
    let mut version = None;
    loop {
        match shared.api.config().await {
            Ok(c) => {
                // The version restarts at 1 with the server: compare the
                // settings too, or a change made just before a restart is
                // missed until the next one.
                if version != Some(c.version) || shared.settings() != c.settings {
                    version = Some(c.version);
                    let enabled = c.settings.as_ref().is_some_and(|s| s.enabled);
                    log::info!(
                        "settings loaded (bot {})",
                        if enabled {
                            "enabled"
                        } else {
                            "disabled in the admin UI"
                        }
                    );
                    *shared.settings.write().unwrap_or_else(|e| e.into_inner()) = c.settings;
                }
                shared.sync_commands().await;
                let name = shared
                    .bot_name
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Err(e) = shared.api.heartbeat(&name).await {
                    log::warn!("heartbeat: {}", e);
                }
                tokio::time::sleep(CONFIG_EVERY).await;
            }
            Err(e) => {
                log::warn!("settings: {}", e);
                tokio::time::sleep(RETRY_AFTER).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{PanelRef, RoomsResponse};

    fn rooms() -> Vec<Room> {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../fixtures/rooms.json")).unwrap();
        r.rooms
    }

    #[test]
    fn panels_are_edited_once_per_change_and_throttled() {
        let mut p = Panels::default();
        let rs = rooms();
        p.apply(&rs);
        let t0 = Instant::now();
        assert_eq!(p.due(t0).len(), 1, "first sight: brought up to date");
        // Same content: nothing to do.
        p.apply(&rs);
        assert!(p.due(t0 + Duration::from_secs(10)).is_empty());

        let mut changed = rooms();
        changed[0].members[1].status = "syncing".into();
        p.apply(&changed);
        assert!(
            p.due(t0 + Duration::from_millis(500)).is_empty(),
            "too soon"
        );
        assert_eq!(p.due(t0 + Duration::from_secs(3)).len(), 1);
    }

    #[test]
    fn closed_rooms_get_a_final_edit_then_are_dropped() {
        let mut p = Panels::default();
        p.apply(&rooms());
        let t0 = Instant::now();
        p.due(t0);
        p.apply(&[]);
        let due = p.due(t0 + Duration::from_secs(3));
        assert_eq!(due.len(), 1);
        assert!(due[0].2.rows.is_empty());
        assert!(due[0].2.description.contains("closed"));
        assert!(p.tracked.is_empty());
    }

    #[test]
    fn a_failed_edit_is_tried_again() {
        let mut p = Panels::default();
        p.apply(&rooms());
        let t0 = Instant::now();
        let (ch, msg, view, retiring) = p.due(t0).remove(0);
        assert!(!retiring);
        p.retry(ch, msg, view.clone(), retiring);
        assert!(
            p.due(t0 + Duration::from_millis(500)).is_empty(),
            "throttled"
        );
        let again = p.due(Instant::now() + Duration::from_secs(3));
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].2, view);

        // The final "closed" edit of a retired panel survives a failure too.
        p.apply(&[]);
        let (ch, msg, closed, retiring) = p.due(Instant::now() + Duration::from_secs(6)).remove(0);
        assert!(retiring && p.tracked.is_empty());
        p.retry(ch, msg, closed.clone(), retiring);
        let last = p.due(Instant::now() + Duration::from_secs(9));
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].2, closed);
        assert!(p.tracked.is_empty(), "dropped once sent");
    }

    #[test]
    fn a_moved_panel_retires_the_old_message() {
        let mut p = Panels::default();
        p.apply(&rooms());
        let t0 = Instant::now();
        p.due(t0);
        let mut moved = rooms();
        moved[0].panel = Some(PanelRef {
            channel_id: "200000000000000002".into(),
            message_id: "400000000000000004".into(),
        });
        p.apply(&moved);
        let due = p.due(t0 + Duration::from_secs(3));
        assert_eq!(due.len(), 2);
        let old = due.iter().find(|d| d.1 == 300000000000000003).unwrap();
        assert!(old.2.description.contains("replaced"));
        assert_eq!(p.tracked.len(), 1);
    }
}
