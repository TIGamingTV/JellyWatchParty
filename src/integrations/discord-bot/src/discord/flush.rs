//! Edits due panel messages on Discord (see `crate::panels` for when).

use super::handler::Shared;
use super::panel::{self, PanelView};
use crate::api::{PanelRef, Room};
use crate::panels::{Due, Panels};
use std::sync::Arc;
use std::time::{Duration, Instant};
use twilight_http::error::ErrorType;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};
use twilight_model::id::Id;

pub const MIN_EDIT_GAP: Duration = Duration::from_secs(2);

/// A panel on Discord: (channel, message).
pub type Key = (u64, u64);

pub type DiscordPanels = Panels<Key, PanelView>;

pub fn new_panels() -> DiscordPanels {
    Panels::new(MIN_EDIT_GAP)
}

fn key(p: &PanelRef) -> Option<Key> {
    Some((p.channel_id.parse().ok()?, p.message_id.parse().ok()?))
}

/// Notes what every panel should show now.
pub fn apply(panels: &mut DiscordPanels, rooms: &[Room]) {
    panels.apply(rooms, key, panel::render, panel::render_closed);
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
        for Due {
            key: (channel, message),
            view,
            retiring,
        } in due
        {
            let (Some(ch), Some(msg)) = (
                Id::<ChannelMarker>::new_checked(channel),
                Id::<MessageMarker>::new_checked(message),
            ) else {
                shared.panels().forget((channel, message));
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
                    shared.panels().forget((channel, message));
                } else if transient(&e) {
                    log::warn!("could not update panel {} (will retry): {}", message, e);
                    shared.panels().retry((channel, message), view, retiring);
                } else {
                    log::warn!("could not update panel {}: {}", message, e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::RoomsResponse;

    #[test]
    fn discord_panels_render_and_close() {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../../fixtures/rooms.json")).unwrap();
        let mut p = new_panels();
        apply(&mut p, &r.rooms);
        let t0 = Instant::now();
        let due = p.due(t0);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].key, (200000000000000002, 300000000000000003));
        assert!(!due[0].view.rows.is_empty());
        apply(&mut p, &[]);
        let due = p.due(t0 + Duration::from_secs(3));
        assert!(due[0].view.rows.is_empty());
        assert!(due[0].view.description.contains("closed"));
    }
}
