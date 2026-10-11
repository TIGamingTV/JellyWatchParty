//! Edits due panel messages on Telegram (see `crate::panels` for when),
//! within Telegram's limits: about 20 messages a minute per group, so at
//! most one panel edit every `CHAT_GAP` in each chat.

use super::client::TgError;
use super::panel::{self, View};
use super::Tg;
use crate::api::{PanelRef, Room};
use crate::panels::{Due, Panels};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const MIN_EDIT_GAP: Duration = Duration::from_secs(5);
const CHAT_GAP: Duration = Duration::from_secs(3);

/// A panel on Telegram: (chat, message).
pub type Key = (i64, i64);

pub type TgPanels = Panels<Key, View>;

type Batch = Vec<Due<Key, View>>;

pub fn new_panels() -> TgPanels {
    Panels::new(MIN_EDIT_GAP)
}

fn key(p: &PanelRef) -> Option<Key> {
    Some((p.channel_id.parse().ok()?, p.message_id.parse().ok()?))
}

pub fn apply(panels: &mut TgPanels, rooms: &[Room]) {
    panels.apply(rooms, key, panel::render, panel::render_closed);
}

/// Which due edits may go out now; the rest wait for their chat's turn.
fn split_by_chat(due: Batch, next_ok: &mut HashMap<i64, Instant>, now: Instant) -> (Batch, Batch) {
    let mut send = Vec::new();
    let mut wait = Vec::new();
    for d in due {
        let chat = d.key.0;
        if next_ok.get(&chat).is_some_and(|t| now < *t) {
            wait.push(d);
        } else {
            next_ok.insert(chat, now + CHAT_GAP);
            send.push(d);
        }
    }
    next_ok.retain(|_, t| now < *t);
    (send, wait)
}

pub async fn flush_loop(tg: Arc<Tg>) {
    let mut next_ok: HashMap<i64, Instant> = HashMap::new();
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if tg.paused() {
            continue;
        }
        let now = Instant::now();
        let due = tg.panels().due(now);
        let (send, wait) = split_by_chat(due, &mut next_ok, now);
        for d in wait {
            tg.panels().retry(d.key, d.view, d.retiring);
        }
        for Due {
            key: (chat, message),
            view,
            retiring,
        } in send
        {
            let res = tg
                .client
                .edit_message(chat, message, &view.text, panel::keyboard(&view.rows))
                .await;
            match res {
                Ok(()) => {}
                Err(TgError::Gone(why)) => {
                    log::info!(
                        "Telegram: panel {} in {} is gone ({}); no longer updating it",
                        message,
                        chat,
                        why
                    );
                    tg.panels().forget((chat, message));
                }
                Err(TgError::Migrated(to)) => {
                    log::warn!(
                        "Telegram: group {} became a supergroup with ID {}: put the new ID in the admin panel",
                        chat,
                        to
                    );
                    tg.panels().forget((chat, message));
                }
                Err(TgError::RetryAfter(s)) => {
                    log::warn!("Telegram: slowing down for {} s", s);
                    tg.pause(s);
                    tg.panels().retry((chat, message), view, retiring);
                }
                Err(e @ TgError::Transient(_)) => {
                    log::warn!(
                        "Telegram: could not update panel {} (will retry): {}",
                        message,
                        e
                    );
                    tg.panels().retry((chat, message), view, retiring);
                }
                Err(e) => log::warn!("Telegram: could not update panel {}: {}", message, e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::RoomsResponse;

    fn view(n: usize) -> View {
        View {
            text: n.to_string(),
            rows: Vec::new(),
        }
    }

    fn due(chat: i64, msg: i64) -> Due<Key, View> {
        Due {
            key: (chat, msg),
            view: view(msg as usize),
            retiring: false,
        }
    }

    #[test]
    fn one_edit_per_chat_at_a_time() {
        let mut next_ok = HashMap::new();
        let t0 = Instant::now();
        let (send, wait) =
            split_by_chat(vec![due(-1, 1), due(-1, 2), due(-2, 3)], &mut next_ok, t0);
        assert_eq!(send.len(), 2);
        assert_eq!(wait.len(), 1);
        let (send, _) = split_by_chat(vec![due(-1, 2)], &mut next_ok, t0 + Duration::from_secs(1));
        assert!(send.is_empty());
        let (send, _) = split_by_chat(vec![due(-1, 2)], &mut next_ok, t0 + CHAT_GAP);
        assert_eq!(send.len(), 1);
    }

    #[test]
    fn telegram_panels_take_negative_chat_ids() {
        let mut r: RoomsResponse =
            serde_json::from_str(include_str!("../../../fixtures/rooms.json")).unwrap();
        r.rooms[0].panel = Some(PanelRef {
            channel_id: "-1001234567890".into(),
            message_id: "17".into(),
        });
        let mut p = new_panels();
        apply(&mut p, &r.rooms);
        let d = p.due(Instant::now());
        assert_eq!(d[0].key, (-1001234567890, 17));
        assert!(d[0].view.text.contains("Friday movie night"));
    }
}
