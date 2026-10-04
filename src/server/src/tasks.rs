use crate::types::{ClientKind, Clients, Room, Rooms};
use crate::utils::now_ms;
use log::{info, warn};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::watch;

const ZOMBIE_CHECK_INTERVAL_SECS: u64 = 30;
const ZOMBIE_TIMEOUT_MS: u64 = crate::room::STALE_AFTER_MS;

pub fn spawn_zombie_cleanup(clients: Clients, rooms: Rooms) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(ZOMBIE_CHECK_INTERVAL_SECS)).await;
            let now = now_ms();
            let zombies: Vec<(String, u64)> = {
                let locked_clients = clients.read().await;
                locked_clients
                    .iter()
                    // Ended connections already have a disconnect scheduled.
                    .filter(|(_, c)| c.connected)
                    // Bridged devices have no socket to go quiet; their own
                    // task watches the Jellyfin session instead.
                    .filter(|(_, c)| c.kind == ClientKind::Web)
                    // last_seen can be a hair newer than `now` (taken before
                    // the lock), or the clock may step back: never wrap.
                    .filter(|(_, c)| now.saturating_sub(c.last_seen) > ZOMBIE_TIMEOUT_MS)
                    .map(|(id, c)| (id.clone(), c.conn_id))
                    .collect()
            };
            for (id, conn_id) in zombies {
                warn!(
                    "Zombie connection detected, starting reconnect grace period: {}",
                    id
                );
                crate::room::schedule_disconnect(id, conn_id, clients.clone(), rooms.clone()).await;
            }
        }
    });
}

const EMPTY_GROUP_CHECK_INTERVAL_SECS: u64 = 30;

/// Ids of admin-created groups that nobody has joined within `ttl_ms` of
/// their creation. (Rooms close as soon as their last member leaves, so an
/// empty room is always one that was never joined.)
pub fn expired_empty_groups(rooms: &HashMap<String, Room>, now: u64, ttl_ms: u64) -> Vec<String> {
    rooms
        .values()
        .filter(|r| {
            r.admin_created && r.clients.is_empty() && now.saturating_sub(r.created_at) > ttl_ms
        })
        .map(|r| r.room_id.clone())
        .collect()
}

/// Removes admin-created groups nobody joined within `ttl_ms`.
pub fn spawn_empty_group_reaper(clients: Clients, rooms: Rooms, ttl_ms: u64) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(EMPTY_GROUP_CHECK_INTERVAL_SECS)).await;
            let removed = {
                let mut locked_rooms = rooms.write().await;
                let expired = expired_empty_groups(&locked_rooms, now_ms(), ttl_ms);
                for id in &expired {
                    info!("Removing admin group {} (nobody joined it)", id);
                    locked_rooms.remove(id);
                }
                !expired.is_empty()
            };
            if removed {
                crate::messaging::broadcast_room_list(&clients, &rooms).await;
            }
        }
    });
}

/// Resolves once SIGTERM/SIGINT has been received. Each listener (the
/// websocket server and the admin panel) waits on its own clone.
pub async fn wait_for_shutdown(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|stop| *stop).await;
}

pub fn setup_shutdown_signal() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
            tokio::select! {
                _ = sigterm.recv() => info!("Received SIGTERM, initiating graceful shutdown..."),
                _ = sigint.recv() => info!("Received SIGINT, initiating graceful shutdown..."),
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for Ctrl+C");
            info!("Received Ctrl+C, initiating graceful shutdown...");
        }
        let _ = tx.send(true);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::ops::create_group;

    #[test]
    fn only_old_empty_admin_groups_expire() {
        let mut rooms = HashMap::new();
        let fresh = create_group("fresh", None, &mut rooms).unwrap();
        let old = create_group("old", None, &mut rooms).unwrap();
        let joined = create_group("joined", None, &mut rooms).unwrap();
        rooms.get_mut(&old).unwrap().created_at = 0;
        let j = rooms.get_mut(&joined).unwrap();
        j.created_at = 0;
        j.clients.push("someone".into());
        let mut user_room = crate::test_helpers::create_room("user", "h");
        user_room.clients.clear();
        rooms.insert("user".into(), user_room);

        let expired = expired_empty_groups(&rooms, now_ms(), 600_000);
        assert_eq!(expired, vec![old]);
        assert!(!expired.contains(&fresh));
    }
}
