//! Keeps every room's public panel message up to date: each is edited at
//! most once per `min_gap`, and only when what it shows changed. The
//! platform renders the view (`V`) and says how a panel is addressed (`K`,
//! e.g. channel and message id); this only decides when to edit what.

use crate::api::{PanelRef, Room};
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

pub const REPLACED: &str = "This panel was replaced by a newer one.";
pub const CLOSED: &str = "This room is closed.";

#[derive(Debug)]
struct Tracked<V> {
    name: String,
    last: Option<V>,
    pending: Option<V>,
    last_edit: Option<Instant>,
    /// The room is gone (or the panel moved): drop after the last edit.
    retire: bool,
}

/// An edit that is due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due<K, V> {
    pub key: K,
    pub view: V,
    /// The final edit of a panel whose room is gone (or that was replaced).
    pub retiring: bool,
}

/// Panel messages and what each last showed.
#[derive(Debug)]
pub struct Panels<K, V> {
    min_gap: Duration,
    /// Keyed by message, so an old panel of a room can be closed while a
    /// newer one is kept up to date.
    tracked: HashMap<K, Tracked<V>>,
}

impl<K: Copy + Eq + Hash, V: Clone + PartialEq> Panels<K, V> {
    pub fn new(min_gap: Duration) -> Self {
        Self {
            min_gap,
            tracked: HashMap::new(),
        }
    }

    /// A panel this bot just posted (already showing `view`).
    pub fn posted(&mut self, key: K, name: &str, view: V) {
        self.tracked.insert(
            key,
            Tracked {
                name: name.to_string(),
                last: Some(view),
                pending: None,
                last_edit: Some(Instant::now()),
                retire: false,
            },
        );
    }

    /// Notes what every panel should show now. `key` turns a room's panel
    /// reference into this platform's address (`None`: not one of ours),
    /// `closed(name, why)` renders a panel whose room is gone.
    pub fn apply(
        &mut self,
        rooms: &[Room],
        key: impl Fn(&PanelRef) -> Option<K>,
        render: impl Fn(&Room) -> V,
        closed: impl Fn(&str, &str) -> V,
    ) {
        let mut current: HashMap<K, &Room> = HashMap::new();
        for r in rooms {
            if let Some(k) = r.panel.as_ref().and_then(&key) {
                current.insert(k, r);
            }
        }
        for (k, room) in &current {
            let view = render(room);
            let t = self.tracked.entry(*k).or_insert_with(|| Tracked {
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
        for (k, t) in self.tracked.iter_mut() {
            if !current.contains_key(k) && !t.retire {
                let still_open = rooms.iter().any(|r| r.name == t.name && r.panel.is_some());
                let why = if still_open { REPLACED } else { CLOSED };
                t.pending = Some(closed(&t.name, why));
                t.retire = true;
            }
        }
    }

    /// Edits that are due now. Marks them as done (see `retry` for when
    /// one fails).
    pub fn due(&mut self, now: Instant) -> Vec<Due<K, V>> {
        let mut out = Vec::new();
        for (k, t) in self.tracked.iter_mut() {
            let ready = t
                .last_edit
                .is_none_or(|at| now.duration_since(at) >= self.min_gap);
            if ready {
                if let Some(v) = t.pending.take() {
                    t.last = Some(v.clone());
                    t.last_edit = Some(now);
                    out.push(Due {
                        key: *k,
                        view: v,
                        retiring: t.retire,
                    });
                }
            }
        }
        self.tracked
            .retain(|_, t| !(t.retire && t.pending.is_none()));
        out
    }

    /// The message is gone: stop updating it.
    pub fn forget(&mut self, key: K) {
        self.tracked.remove(&key);
    }

    /// An edit didn't go through: try `view` again later, unless something
    /// newer is already waiting. A final edit of a retired panel is kept
    /// until it goes through.
    pub fn retry(&mut self, key: K, view: V, retire: bool) {
        let t = self.tracked.entry(key).or_insert_with(|| Tracked {
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

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.tracked.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::RoomsResponse;

    fn rooms() -> Vec<Room> {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../fixtures/rooms.json")).unwrap();
        r.rooms
    }

    type P = Panels<(u64, u64), String>;

    fn key(p: &PanelRef) -> Option<(u64, u64)> {
        Some((p.channel_id.parse().ok()?, p.message_id.parse().ok()?))
    }

    fn render(r: &Room) -> String {
        let status: Vec<&str> = r.members.iter().map(|m| m.status.as_str()).collect();
        format!("{}: {}", r.name, status.join(","))
    }

    fn closed(name: &str, why: &str) -> String {
        format!("{} - {}", name, why)
    }

    fn apply(p: &mut P, rooms: &[Room]) {
        p.apply(rooms, key, render, closed);
    }

    fn panels() -> P {
        Panels::new(Duration::from_secs(2))
    }

    #[test]
    fn panels_are_edited_once_per_change_and_throttled() {
        let mut p = panels();
        let rs = rooms();
        apply(&mut p, &rs);
        let t0 = Instant::now();
        assert_eq!(p.due(t0).len(), 1, "first sight: brought up to date");
        // Same content: nothing to do.
        apply(&mut p, &rs);
        assert!(p.due(t0 + Duration::from_secs(10)).is_empty());

        let mut changed = rooms();
        changed[0].members[1].status = "syncing".into();
        apply(&mut p, &changed);
        assert!(
            p.due(t0 + Duration::from_millis(500)).is_empty(),
            "too soon"
        );
        assert_eq!(p.due(t0 + Duration::from_secs(3)).len(), 1);
    }

    #[test]
    fn closed_rooms_get_a_final_edit_then_are_dropped() {
        let mut p = panels();
        apply(&mut p, &rooms());
        let t0 = Instant::now();
        p.due(t0);
        apply(&mut p, &[]);
        let due = p.due(t0 + Duration::from_secs(3));
        assert_eq!(due.len(), 1);
        assert!(due[0].retiring);
        assert!(due[0].view.ends_with(CLOSED));
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn a_failed_edit_is_tried_again() {
        let mut p = panels();
        apply(&mut p, &rooms());
        let t0 = Instant::now();
        let d = p.due(t0).remove(0);
        assert!(!d.retiring);
        p.retry(d.key, d.view.clone(), d.retiring);
        assert!(
            p.due(t0 + Duration::from_millis(500)).is_empty(),
            "throttled"
        );
        let again = p.due(Instant::now() + Duration::from_secs(3));
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].view, d.view);

        // The final "closed" edit of a retired panel survives a failure too.
        apply(&mut p, &[]);
        let c = p.due(Instant::now() + Duration::from_secs(6)).remove(0);
        assert!(c.retiring && p.len() == 0);
        p.retry(c.key, c.view.clone(), c.retiring);
        let last = p.due(Instant::now() + Duration::from_secs(9));
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].view, c.view);
        assert_eq!(p.len(), 0, "dropped once sent");
    }

    #[test]
    fn a_moved_panel_retires_the_old_message() {
        let mut p = panels();
        apply(&mut p, &rooms());
        let t0 = Instant::now();
        p.due(t0);
        let mut moved = rooms();
        moved[0].panel = Some(PanelRef {
            channel_id: "200000000000000002".into(),
            message_id: "400000000000000004".into(),
        });
        apply(&mut p, &moved);
        let due = p.due(t0 + Duration::from_secs(3));
        assert_eq!(due.len(), 2);
        let old = due.iter().find(|d| d.key.1 == 300000000000000003).unwrap();
        assert!(old.view.ends_with(REPLACED));
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn panels_of_other_platforms_are_ignored() {
        let mut p = panels();
        let mut rs = rooms();
        rs[0].panel = Some(PanelRef {
            channel_id: "not-a-number".into(),
            message_id: "1".into(),
        });
        apply(&mut p, &rs);
        assert!(p.due(Instant::now()).is_empty());
    }
}
