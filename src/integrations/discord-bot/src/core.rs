//! What every chat platform shares: its connection to the session server,
//! the settings and rooms fetched from it, and the two loops that keep
//! them current (settings + heartbeat, and the rooms long poll).

use crate::api::{Api, Room, Settings};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

const CONFIG_EVERY: Duration = Duration::from_secs(30);
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// One platform's view of the session server.
pub struct Core {
    /// For log lines ("Discord", "Telegram").
    pub label: &'static str,
    pub api: Api,
    pub settings: RwLock<Option<Settings>>,
    pub rooms: RwLock<Vec<Room>>,
    /// The bot's name on its platform, sent with every heartbeat.
    pub bot_name: RwLock<String>,
    /// What `democratic_changed` last saw.
    democratic_seen: AtomicBool,
}

impl Core {
    pub fn new(label: &'static str, api: Api) -> Self {
        Self {
            label,
            api,
            settings: RwLock::new(None),
            rooms: RwLock::new(Vec::new()),
            bot_name: RwLock::new(String::new()),
            democratic_seen: AtomicBool::new(false),
        }
    }

    pub fn settings(&self) -> Option<Settings> {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Whether the admin turned on democratic mode commands.
    pub fn democratic(&self) -> bool {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|s| s.democratic_commands)
    }

    /// Whether `democratic()` changed since the last call (panels show it,
    /// so they need another look).
    pub fn democratic_changed(&self) -> bool {
        let now = self.democratic();
        self.democratic_seen.swap(now, Ordering::Relaxed) != now
    }

    pub fn rooms(&self) -> Vec<Room> {
        self.rooms.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn room(&self, id: &str) -> Option<Room> {
        self.rooms().into_iter().find(|r| r.id == id)
    }

    pub fn bot_name(&self) -> String {
        self.bot_name
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_bot_name(&self, name: &str) {
        *self.bot_name.write().unwrap_or_else(|e| e.into_inner()) = name.to_string();
    }
}

/// A chat platform, as seen by the shared loops.
pub trait Platform: Send + Sync + 'static {
    fn core(&self) -> &Core;

    /// The rooms changed (called after the new list is stored, so anything
    /// that reads `Core::rooms` meanwhile sees the new one).
    fn rooms_changed(&self, rooms: &[Room]);

    /// After every settings check, whether they changed or not (e.g. to
    /// register commands where the settings say).
    fn settings_checked(&self) -> impl Future<Output = ()> + Send;
}

/// Follows the server's rooms (long poll).
pub async fn rooms_loop<P: Platform>(p: Arc<P>) {
    let core = p.core();
    let mut since = None;
    loop {
        match core.api.rooms(since).await {
            Ok(r) => {
                since = Some(r.version);
                *core.rooms.write().unwrap_or_else(|e| e.into_inner()) = r.rooms.clone();
                p.rooms_changed(&r.rooms);
            }
            Err(e) => {
                log::warn!("{}: room updates: {}", core.label, e);
                since = None;
                tokio::time::sleep(RETRY_AFTER).await;
            }
        }
    }
}

/// Fetches the settings and checks in, every `CONFIG_EVERY`.
pub async fn config_loop<P: Platform>(p: Arc<P>) {
    let core = p.core();
    let mut version = None;
    loop {
        match core.api.config().await {
            Ok(c) => {
                // The version restarts at 1 with the server: compare the
                // settings too, or a change made just before a restart is
                // missed until the next one.
                if version != Some(c.version) || core.settings() != c.settings {
                    version = Some(c.version);
                    let enabled = c.settings.as_ref().is_some_and(|s| s.enabled);
                    log::info!(
                        "{}: settings loaded (bot {})",
                        core.label,
                        if enabled {
                            "enabled"
                        } else {
                            "disabled in the admin UI"
                        }
                    );
                    *core.settings.write().unwrap_or_else(|e| e.into_inner()) = c.settings;
                }
                p.settings_checked().await;
                if let Err(e) = core.api.heartbeat(&core.bot_name()).await {
                    log::warn!("{}: heartbeat: {}", core.label, e);
                }
                tokio::time::sleep(CONFIG_EVERY).await;
            }
            Err(e) => {
                log::warn!("{}: settings: {}", core.label, e);
                tokio::time::sleep(RETRY_AFTER).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_democratic_switch_is_noticed_once() {
        let core = Core::new("T", Api::new("http://127.0.0.1:9", "t", "T").unwrap());
        assert!(!core.democratic() && !core.democratic_changed());
        *core.settings.write().unwrap() = Some(Settings {
            democratic_commands: true,
            ..Settings::default()
        });
        assert!(core.democratic());
        assert!(core.democratic_changed());
        assert!(!core.democratic_changed());
        *core.settings.write().unwrap() = None;
        assert!(core.democratic_changed());
    }
}
