//! A server-wide "something about the rooms changed" counter.
//!
//! Bumped wherever the room list or a room's participants are broadcast, so
//! out-of-process watchers (the chat integration's long-poll) learn about
//! changes without being room members themselves.

use std::sync::LazyLock;
use tokio::sync::watch;

static ROOMS_VERSION: LazyLock<watch::Sender<u64>> = LazyLock::new(|| watch::channel(1).0);

/// Marks the rooms as changed.
pub fn bump() {
    ROOMS_VERSION.send_modify(|v| *v = v.wrapping_add(1));
}

/// The current version.
pub fn version() -> u64 {
    *ROOMS_VERSION.borrow()
}

/// A receiver that wakes on every bump.
pub fn subscribe() -> watch::Receiver<u64> {
    ROOMS_VERSION.subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bump_wakes_subscribers() {
        let mut rx = subscribe();
        rx.borrow_and_update();
        let before = version();
        bump();
        assert!(version() != before);
        assert!(rx.has_changed().unwrap());
    }
}
