//! Pure decision logic for a bridged Jellyfin device. No I/O here: the
//! bridge task feeds in what it saw (room + device) and carries out what
//! comes back, which keeps all of the timing rules unit-testable.

use super::api::JfSession;
use super::time::parse_utc_ms;

pub const TICKS_PER_SEC: f64 = 10_000_000.0;
/// Further apart than this (seconds), a receiver is seeked.
pub const DRIFT_THRESHOLD_SECS: f64 = 2.0;
/// Minimum time between seeks sent to one device.
pub const SEEK_COOLDOWN_MS: u64 = 4_000;
/// Seek this far ahead of the room while playing, to cover the time the
/// device takes to act on the seek.
pub const SEEK_LEAD_SECS: f64 = 1.0;
/// Minimum time between pause/unpause commands to one device.
pub const PLAYPAUSE_COOLDOWN_MS: u64 = 2_500;
/// Minimum time between "play this item" commands to one device.
pub const PLAY_NOW_COOLDOWN_MS: u64 = 15_000;
/// Never extrapolate a playing device's position further than this past
/// its last progress report (the device may have stalled).
pub const MAX_EXTRAPOLATION_SECS: f64 = 30.0;

/// What the bridge knows about a device at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceView {
    pub item_id: Option<String>,
    /// Estimated current position (seconds).
    pub position: f64,
    pub paused: bool,
    /// When the device last reported progress (ms since epoch, 0 if never).
    pub checkin_ms: u64,
}

/// Jellyfin only learns a device's position when the device reports
/// progress (every few seconds). Extrapolate from that report.
pub fn device_view(s: &JfSession, now_ms: u64) -> DeviceView {
    let ps = s.play_state.clone().unwrap_or_default();
    let reported = ps.position_ticks.unwrap_or(0).max(0) as f64 / TICKS_PER_SEC;
    let checkin_ms = s
        .last_playback_check_in
        .as_deref()
        .and_then(parse_utc_ms)
        .unwrap_or(0);
    let item_id = s.item_id();
    let position = if !ps.is_paused && item_id.is_some() && checkin_ms > 0 {
        let elapsed = now_ms.saturating_sub(checkin_ms) as f64 / 1000.0;
        reported + elapsed.min(MAX_EXTRAPOLATION_SECS)
    } else {
        reported
    };
    DeviceView {
        item_id,
        position,
        paused: ps.is_paused,
        checkin_ms,
    }
}

/// What the room wants, from the bridge's point of view.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomView {
    pub media_id: Option<String>,
    pub playing: bool,
    /// Where the room is right now (seconds).
    pub expected: f64,
}

// --- receiver --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    PlayNow { item_id: String, start_secs: f64 },
    Seek { to_secs: f64 },
    Pause,
    Unpause,
}

/// What a receiver bridge sent recently. Until the device reports progress
/// again, the bridge assumes its last command took effect, so it doesn't
/// repeat a seek just because Jellyfin still shows the old position.
#[derive(Debug, Clone, Default)]
pub struct FollowerMemory {
    pub seek_at: u64,
    pub seek_target: f64,
    pub pp_at: u64,
    pub pp_paused: bool,
    pub play_now_at: u64,
    pub play_now_item: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub commands: Vec<Command>,
    /// One of the participant statuses (`synced`, `syncing`, `loading`,
    /// `idle`, ...), or `offline` when the device isn't there.
    pub status: &'static str,
    /// Device minus room position, when on the room's item.
    pub drift: Option<f64>,
    /// True when the device is on the room's item (or the room has none),
    /// i.e. it can count as `ready`.
    pub on_item: bool,
}

pub fn follower_step(
    room: &RoomView,
    device: Option<&DeviceView>,
    mem: &mut FollowerMemory,
    now: u64,
) -> Step {
    let mut commands = Vec::new();
    let Some(d) = device else {
        return Step {
            commands,
            status: "offline",
            drift: None,
            on_item: false,
        };
    };
    let Some(media) = room.media_id.as_ref() else {
        return Step {
            commands,
            status: "idle",
            drift: None,
            on_item: true,
        };
    };

    if d.item_id.as_ref() != Some(media) {
        let retry = mem.play_now_item.as_ref() != Some(media)
            || now.saturating_sub(mem.play_now_at) >= PLAY_NOW_COOLDOWN_MS;
        if retry {
            commands.push(Command::PlayNow {
                item_id: media.clone(),
                start_secs: room.expected.max(0.0),
            });
            mem.play_now_at = now;
            mem.play_now_item = Some(media.clone());
            // Playback starts playing; a paused room gets a pause once the
            // device is on the item.
            mem.pp_at = now;
            mem.pp_paused = false;
        }
        return Step {
            commands,
            status: "loading",
            drift: None,
            on_item: false,
        };
    }

    let paused = if mem.pp_at > d.checkin_ms {
        mem.pp_paused
    } else {
        d.paused
    };
    let position = if mem.seek_at > d.checkin_ms {
        let since = if paused {
            0.0
        } else {
            now.saturating_sub(mem.seek_at) as f64 / 1000.0
        };
        mem.seek_target + since
    } else {
        d.position
    };

    let want_paused = !room.playing;
    let state_ok = paused == want_paused;
    if !state_ok && now.saturating_sub(mem.pp_at) >= PLAYPAUSE_COOLDOWN_MS {
        commands.push(if want_paused {
            Command::Pause
        } else {
            Command::Unpause
        });
        mem.pp_at = now;
        mem.pp_paused = want_paused;
    }

    let drift = position - room.expected;
    let in_sync = drift.abs() <= DRIFT_THRESHOLD_SECS;
    if !in_sync && now.saturating_sub(mem.seek_at) >= SEEK_COOLDOWN_MS {
        let target = (room.expected + if room.playing { SEEK_LEAD_SECS } else { 0.0 }).max(0.0);
        commands.push(Command::Seek { to_secs: target });
        mem.seek_at = now;
        mem.seek_target = target;
    }

    Step {
        commands,
        status: if in_sync && state_ok {
            "synced"
        } else {
            "syncing"
        },
        drift: Some(drift),
        on_item: true,
    }
}

// --- host ------------------------------------------------------------------

/// Messages a host bridge sends into its room, like a web host would.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    SetMedia { media_id: String, position: f64 },
    Play { position: f64 },
    Pause { position: f64 },
    State { position: f64, playing: bool },
}

#[derive(Debug, Clone, Default)]
pub struct HostMemory {
    /// Last play/pause state reported to the room; `None` until the first.
    pub last_paused: Option<bool>,
}

pub fn host_step(
    room: &RoomView,
    device: Option<&DeviceView>,
    mem: &mut HostMemory,
) -> (Vec<HostEvent>, &'static str) {
    let mut events = Vec::new();
    let item = device.and_then(|d| d.item_id.clone());
    let (Some(d), Some(item)) = (device, item) else {
        // Stopped, or the device went away: the room pauses where it is.
        if mem.last_paused == Some(false) {
            events.push(HostEvent::Pause {
                position: room.expected,
            });
        }
        mem.last_paused = Some(true);
        return (events, if device.is_some() { "idle" } else { "offline" });
    };

    if room.media_id.as_deref() != Some(item.as_str()) {
        events.push(HostEvent::SetMedia {
            media_id: item,
            position: d.position,
        });
        // set_media leaves the room paused; report the device state afresh.
        mem.last_paused = None;
    }

    match mem.last_paused {
        Some(p) if p == d.paused => events.push(HostEvent::State {
            position: d.position,
            playing: !d.paused,
        }),
        _ if d.paused => {
            if mem.last_paused.is_some() || room.playing {
                events.push(HostEvent::Pause {
                    position: d.position,
                });
            } else {
                events.push(HostEvent::State {
                    position: d.position,
                    playing: false,
                });
            }
        }
        _ => events.push(HostEvent::Play {
            position: d.position,
        }),
    }
    mem.last_paused = Some(d.paused);
    (events, if d.paused { "paused" } else { "playing" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jellyfin::api::{JfItem, JfPlayState};

    const ITEM: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "ffffffffffffffffffffffffffffffff";

    fn room(playing: bool, expected: f64) -> RoomView {
        RoomView {
            media_id: Some(ITEM.into()),
            playing,
            expected,
        }
    }

    fn dev(item: Option<&str>, position: f64, paused: bool, checkin_ms: u64) -> DeviceView {
        DeviceView {
            item_id: item.map(String::from),
            position,
            paused,
            checkin_ms,
        }
    }

    #[test]
    fn device_view_extrapolates_while_playing() {
        let s = JfSession {
            now_playing_item: Some(JfItem {
                id: Some(ITEM.to_uppercase()),
                ..Default::default()
            }),
            play_state: Some(JfPlayState {
                position_ticks: Some(100 * 10_000_000),
                is_paused: false,
            }),
            last_playback_check_in: Some("1970-01-01T00:00:10Z".into()),
            ..Default::default()
        };
        let v = device_view(&s, 13_000);
        assert_eq!(v.item_id.as_deref(), Some(ITEM));
        assert!((v.position - 103.0).abs() < 1e-9);
        assert_eq!(v.checkin_ms, 10_000);
        // Capped for a device that stopped reporting.
        assert!((device_view(&s, 1_000_000).position - 130.0).abs() < 1e-9);
        // Paused: no extrapolation.
        let mut paused = s.clone();
        paused.play_state.as_mut().unwrap().is_paused = true;
        assert!((device_view(&paused, 13_000).position - 100.0).abs() < 1e-9);
    }

    #[test]
    fn receiver_offline_and_idle() {
        let mut m = FollowerMemory::default();
        assert_eq!(
            follower_step(&room(true, 5.0), None, &mut m, 0).status,
            "offline"
        );
        let no_media = RoomView {
            media_id: None,
            playing: false,
            expected: 0.0,
        };
        let s = follower_step(&no_media, Some(&dev(None, 0.0, true, 0)), &mut m, 0);
        assert_eq!(s.status, "idle");
        assert!(s.on_item && s.commands.is_empty());
    }

    #[test]
    fn receiver_on_wrong_item_is_told_to_play_it_once_per_cooldown() {
        let mut m = FollowerMemory::default();
        let d = dev(Some(OTHER), 0.0, false, 0);
        let s = follower_step(&room(true, 42.0), Some(&d), &mut m, 1_000);
        assert_eq!(
            s.commands,
            vec![Command::PlayNow {
                item_id: ITEM.into(),
                start_secs: 42.0
            }]
        );
        assert_eq!(s.status, "loading");
        assert!(!s.on_item);
        let s = follower_step(&room(true, 43.0), Some(&d), &mut m, 2_000);
        assert!(s.commands.is_empty());
        let s = follower_step(
            &room(true, 60.0),
            Some(&d),
            &mut m,
            1_000 + PLAY_NOW_COOLDOWN_MS,
        );
        assert_eq!(s.commands.len(), 1);
        // Nothing playing at all: same.
        let mut m = FollowerMemory::default();
        let s = follower_step(&room(false, 0.0), Some(&dev(None, 0.0, true, 0)), &mut m, 0);
        assert!(matches!(s.commands[0], Command::PlayNow { .. }));
    }

    #[test]
    fn receiver_in_sync_does_nothing() {
        let mut m = FollowerMemory::default();
        let s = follower_step(
            &room(true, 100.0),
            Some(&dev(Some(ITEM), 101.5, false, 1)),
            &mut m,
            10_000,
        );
        assert!(s.commands.is_empty());
        assert_eq!(s.status, "synced");
        assert!((s.drift.unwrap() - 1.5).abs() < 1e-9);
        assert!(s.on_item);
    }

    #[test]
    fn receiver_seeks_with_lead_and_cooldown_and_trusts_its_seek() {
        let mut m = FollowerMemory::default();
        let d = dev(Some(ITEM), 50.0, false, 1);
        let s = follower_step(&room(true, 100.0), Some(&d), &mut m, 10_000);
        assert_eq!(
            s.commands,
            vec![Command::Seek {
                to_secs: 100.0 + SEEK_LEAD_SECS
            }]
        );
        assert_eq!(s.status, "syncing");

        // Jellyfin still reports the old position (no check-in since the
        // seek): the bridge assumes the seek worked and stays quiet.
        let s = follower_step(&room(true, 102.0), Some(&d), &mut m, 12_000);
        assert!(s.commands.is_empty(), "{:?}", s.commands);
        assert_eq!(s.status, "synced");

        // A fresh report that is still far off: seek again after cooldown.
        let fresh = dev(Some(ITEM), 60.0, false, 13_000);
        let s = follower_step(&room(true, 103.0), Some(&fresh), &mut m, 13_500);
        assert!(s.commands.is_empty(), "cooldown");
        let s = follower_step(&room(true, 104.0), Some(&fresh), &mut m, 14_000);
        assert_eq!(s.commands.len(), 1);
    }

    #[test]
    fn receiver_pauses_and_unpauses_with_the_room() {
        let mut m = FollowerMemory::default();
        let playing_dev = dev(Some(ITEM), 10.0, false, 1);
        let s = follower_step(&room(false, 10.0), Some(&playing_dev), &mut m, 10_000);
        assert_eq!(s.commands, vec![Command::Pause]);
        assert_eq!(s.status, "syncing");
        // Not repeated while the device hasn't reported back.
        let s = follower_step(&room(false, 10.0), Some(&playing_dev), &mut m, 11_000);
        assert!(s.commands.is_empty());
        assert_eq!(s.status, "synced");

        let paused_dev = dev(Some(ITEM), 10.0, true, 20_000);
        let s = follower_step(&room(true, 10.0), Some(&paused_dev), &mut m, 21_000);
        assert_eq!(s.commands, vec![Command::Unpause]);
    }

    #[test]
    fn paused_room_seek_has_no_lead() {
        let mut m = FollowerMemory::default();
        let s = follower_step(
            &room(false, 30.0),
            Some(&dev(Some(ITEM), 0.0, true, 1)),
            &mut m,
            10_000,
        );
        assert_eq!(s.commands, vec![Command::Seek { to_secs: 30.0 }]);
    }

    #[test]
    fn host_first_sight_and_changes() {
        let mut m = HostMemory::default();
        let r = room(false, 0.0);
        // Already playing when it becomes host: the room plays.
        let (ev, st) = host_step(&r, Some(&dev(Some(ITEM), 5.0, false, 1)), &mut m);
        assert_eq!(ev, vec![HostEvent::Play { position: 5.0 }]);
        assert_eq!(st, "playing");
        // Steady state: position updates.
        let (ev, _) = host_step(
            &room(true, 5.0),
            Some(&dev(Some(ITEM), 6.0, false, 1)),
            &mut m,
        );
        assert_eq!(
            ev,
            vec![HostEvent::State {
                position: 6.0,
                playing: true
            }]
        );
        // Paused on the device.
        let (ev, st) = host_step(
            &room(true, 6.0),
            Some(&dev(Some(ITEM), 7.0, true, 1)),
            &mut m,
        );
        assert_eq!(ev, vec![HostEvent::Pause { position: 7.0 }]);
        assert_eq!(st, "paused");
    }

    #[test]
    fn host_paused_at_first_sight_reports_state_only() {
        let mut m = HostMemory::default();
        let (ev, _) = host_step(
            &room(false, 0.0),
            Some(&dev(Some(ITEM), 5.0, true, 1)),
            &mut m,
        );
        assert_eq!(
            ev,
            vec![HostEvent::State {
                position: 5.0,
                playing: false
            }]
        );
        // ...but a room that is playing gets paused.
        let mut m = HostMemory::default();
        let (ev, _) = host_step(
            &room(true, 0.0),
            Some(&dev(Some(ITEM), 5.0, true, 1)),
            &mut m,
        );
        assert_eq!(ev, vec![HostEvent::Pause { position: 5.0 }]);
    }

    #[test]
    fn host_switching_items_sets_media_then_plays() {
        let mut m = HostMemory {
            last_paused: Some(false),
        };
        let (ev, _) = host_step(
            &room(true, 50.0),
            Some(&dev(Some(OTHER), 1.0, false, 1)),
            &mut m,
        );
        assert_eq!(
            ev,
            vec![
                HostEvent::SetMedia {
                    media_id: OTHER.into(),
                    position: 1.0
                },
                HostEvent::Play { position: 1.0 }
            ]
        );
    }

    #[test]
    fn host_stopping_pauses_the_room_once() {
        let mut m = HostMemory {
            last_paused: Some(false),
        };
        let (ev, st) = host_step(&room(true, 50.0), Some(&dev(None, 0.0, true, 1)), &mut m);
        assert_eq!(ev, vec![HostEvent::Pause { position: 50.0 }]);
        assert_eq!(st, "idle");
        let (ev, st) = host_step(&room(false, 50.0), None, &mut m);
        assert!(ev.is_empty());
        assert_eq!(st, "offline");
    }
}
