//! Chat integrations: lets people run third-party-client watch parties
//! from a chat platform (Discord, Telegram) without an admin, through a bot
//! that runs as a separate sidecar.
//!
//! - The sidecar only reports who is asking (their chat account, the
//!   server or group, channel and roles the request came from) over a
//!   token-protected API on its own port (`api.rs`). Every decision is
//!   made here.
//! - A chat account acts as a Jellyfin user once linked with a 4-digit code
//!   an admin assigned to that user in the admin UI (`store.rs`).
//! - Rooms created from a chat have an owner and participants (`actions.rs`,
//!   `types::ChatRoom`). Participants may only bridge their *own* Jellyfin
//!   devices; the owner (or an admin) controls the room.

mod actions;
pub mod api;
pub mod config;
pub mod store;
mod view;

#[cfg(test)]
mod tests;

pub use actions::{DeviceRole, KickTarget, RoomUpdate};

use crate::jellyfin::api::JfUser;
use crate::jellyfin::Bridges;
use crate::types::{Clients, Rooms};
use crate::utils::now_ms;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use config::IntegrationConfig;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use store::Store;

/// The Jellyfin user list is re-fetched at most this often...
const USERS_TTL_MS: u64 = 60_000;
/// ...and a stale copy is used for this long while Jellyfin is unreachable.
const USERS_STALE_OK_MS: u64 = 10 * 60_000;
const AUDIT_LEN: usize = 500;

/// Wrong link attempts per chat account before it must wait.
pub const LINK_FAILS_PER_ACCOUNT: u32 = 5;
pub const LINK_FAIL_WINDOW_MS: u64 = 15 * 60_000;
/// Wrong link attempts from everyone together before linking pauses for a
/// while: bounds how fast many accounts together can guess.
pub const LINK_FAILS_GLOBAL: u32 = 50;
pub const LINK_GLOBAL_WINDOW_MS: u64 = 10 * 60_000;
/// Requests per chat account per minute.
pub const REQUESTS_PER_ACTOR: u32 = 30;
const REQUEST_WINDOW_MS: u64 = 60_000;
/// A sidecar that hasn't checked in for this long is shown as offline.
const SIDECAR_STALE_MS: u64 = 90_000;
const REAPER_INTERVAL_SECS: u64 = 30;

/// An integration API failure: `{error, reason, retry_after_ms?}`.
#[derive(Debug)]
pub struct IntError {
    pub status: StatusCode,
    pub reason: &'static str,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl IntError {
    pub fn new(status: StatusCode, reason: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            reason,
            message: message.into(),
            retry_after_ms: None,
        }
    }

    fn forbidden(reason: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, reason, message)
    }

    fn conflict(reason: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, reason, message)
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid", message)
    }

    fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "That room doesn't exist anymore",
        )
    }

    fn wait(reason: &'static str, message: impl Into<String>, retry_after_ms: u64) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            reason,
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
        }
    }

    fn jellyfin(e: String) -> Self {
        warn!("chat integration: Jellyfin unavailable: {}", e);
        Self::new(
            StatusCode::BAD_GATEWAY,
            "jellyfin_unavailable",
            "Jellyfin can't be reached right now; try again in a minute",
        )
    }

    pub fn body(&self) -> serde_json::Value {
        let mut v = serde_json::json!({ "error": self.message, "reason": self.reason });
        if let Some(ms) = self.retry_after_ms {
            v["retry_after_ms"] = ms.into();
        }
        v
    }
}

impl IntoResponse for IntError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body())).into_response()
    }
}

pub type IntResult = Result<serde_json::Value, IntError>;

/// Who is asking, as the sidecar saw it. Only facts the platform told the
/// sidecar; the server decides what they may do.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Actor {
    /// The chat account id (Discord: user snowflake; Telegram: user id).
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// The server (Discord guild) or group (Telegram chat id) the request
    /// is about.
    #[serde(default)]
    pub guild_id: String,
    #[serde(default)]
    pub channel_id: String,
    /// Role ids the account has there (Telegram: `admin` for group
    /// administrators).
    #[serde(default)]
    pub roles: Vec<String>,
}

/// A chat account id: Discord snowflakes and Telegram user ids are
/// positive numbers.
pub fn valid_external_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

/// A chat (channel) id: like an account id, but Telegram groups are
/// negative.
pub fn valid_chat_id(id: &str) -> bool {
    valid_external_id(id.strip_prefix('-').unwrap_or(id))
}

/// Strips control characters and caps the length of a display name.
pub fn clean_display_name(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub ts: u64,
    pub kind: &'static str,
    /// `admin`, or `<provider>:<id> (<name>)`.
    pub actor: String,
    pub detail: String,
    pub warn: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Heartbeat {
    pub seen_at: u64,
    pub bot_name: String,
}

#[derive(Default)]
struct Guards {
    /// provider:account -> (count, window start)
    link_fails: HashMap<String, (u32, u64)>,
    link_fails_global: (u32, u64),
    requests: HashMap<String, (u32, u64)>,
}

fn window_left(entry: Option<&(u32, u64)>, limit: u32, window: u64, now: u64) -> Option<u64> {
    let &(count, start) = entry?;
    let elapsed = now.saturating_sub(start);
    (count >= limit && elapsed < window).then(|| window - elapsed)
}

fn bump_window(entry: &mut (u32, u64), window: u64, now: u64) {
    if now.saturating_sub(entry.1) >= window {
        *entry = (0, now);
    }
    entry.0 = entry.0.saturating_add(1);
}

impl Guards {
    fn link_blocked(&self, key: &str, now: u64) -> Option<u64> {
        let account = window_left(
            self.link_fails.get(key),
            LINK_FAILS_PER_ACCOUNT,
            LINK_FAIL_WINDOW_MS,
            now,
        );
        let global = window_left(
            Some(&self.link_fails_global),
            LINK_FAILS_GLOBAL,
            LINK_GLOBAL_WINDOW_MS,
            now,
        );
        account.max(global)
    }

    fn record_link_fail(&mut self, key: &str, now: u64) {
        self.link_fails
            .retain(|_, (_, start)| now.saturating_sub(*start) < LINK_FAIL_WINDOW_MS);
        let e = self.link_fails.entry(key.to_string()).or_insert((0, now));
        bump_window(e, LINK_FAIL_WINDOW_MS, now);
        bump_window(&mut self.link_fails_global, LINK_GLOBAL_WINDOW_MS, now);
    }

    fn clear_link_fails(&mut self, key: &str) {
        self.link_fails.remove(key);
    }

    /// Counts a request; returns how long to wait if over the limit.
    fn rate_limited(&mut self, key: &str, now: u64) -> Option<u64> {
        if self.requests.len() > 10_000 {
            self.requests
                .retain(|_, (_, start)| now.saturating_sub(*start) < REQUEST_WINDOW_MS);
        }
        let e = self.requests.entry(key.to_string()).or_insert((0, now));
        bump_window(e, REQUEST_WINDOW_MS, now);
        window_left(Some(e), REQUESTS_PER_ACTOR + 1, REQUEST_WINDOW_MS, now)
    }
}

struct UserCache {
    fetched_at: u64,
    users: Arc<Vec<JfUser>>,
}

struct Inner {
    store: Store,
    bridges: Bridges,
    clients: Clients,
    rooms: Rooms,
    /// Providers whose sidecar token is configured.
    providers: Vec<String>,
    listen_addr: Option<std::net::SocketAddr>,
    users: tokio::sync::Mutex<Option<UserCache>>,
    audit: Mutex<VecDeque<AuditEntry>>,
    guards: Mutex<Guards>,
    sidecars: Mutex<HashMap<String, Heartbeat>>,
    settings_version: AtomicU64,
}

/// The chat integration, shared by its API listener and the admin panel.
#[derive(Clone)]
pub struct Integration(Arc<Inner>);

/// Whether chat integrations can be used, for the admin panel.
#[derive(Clone)]
pub enum IntegrationStatus {
    Enabled(Integration),
    /// Why not (shown in the UI).
    Unavailable(String),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Integration {
    pub fn new(
        cfg: &IntegrationConfig,
        bridges: Bridges,
        clients: Clients,
        rooms: Rooms,
    ) -> Result<Self, String> {
        let store = Store::open(&cfg.data_dir)?;
        Ok(Self(Arc::new(Inner {
            store,
            bridges,
            clients,
            rooms,
            providers: cfg.tokens.iter().map(|(p, _)| p.clone()).collect(),
            listen_addr: (!cfg.tokens.is_empty()).then_some(cfg.addr),
            users: tokio::sync::Mutex::new(None),
            audit: Mutex::new(VecDeque::new()),
            guards: Mutex::new(Guards::default()),
            sidecars: Mutex::new(HashMap::new()),
            settings_version: AtomicU64::new(1),
        })))
    }

    pub fn store(&self) -> &Store {
        &self.0.store
    }

    pub fn bridges(&self) -> &Bridges {
        &self.0.bridges
    }

    pub fn has_sidecar(&self, provider: &str) -> bool {
        self.0.providers.iter().any(|p| p == provider)
    }

    pub fn settings_version(&self) -> u64 {
        self.0.settings_version.load(Ordering::Relaxed)
    }

    pub fn settings_changed(&self) {
        self.0.settings_version.fetch_add(1, Ordering::Relaxed);
    }

    /// Every Jellyfin user, cached for a minute.
    pub async fn users(&self) -> Result<Arc<Vec<JfUser>>, String> {
        self.fetch_users(false).await
    }

    /// Like `users`, but asks Jellyfin now (after an admin assigns a code,
    /// so a just-created user is found). Falls back to the cached list.
    pub async fn fresh_users(&self) -> Result<Arc<Vec<JfUser>>, String> {
        self.fetch_users(true).await
    }

    async fn fetch_users(&self, force: bool) -> Result<Arc<Vec<JfUser>>, String> {
        let mut cache = self.0.users.lock().await;
        let now = now_ms();
        if let Some(c) = cache.as_ref() {
            if !force && now.saturating_sub(c.fetched_at) < USERS_TTL_MS {
                return Ok(c.users.clone());
            }
        }
        match self.0.bridges.api().users().await {
            Ok(users) => {
                let users = Arc::new(users);
                *cache = Some(UserCache {
                    fetched_at: now,
                    users: users.clone(),
                });
                Ok(users)
            }
            Err(e) => match cache.as_ref() {
                Some(c) if now.saturating_sub(c.fetched_at) < USERS_STALE_OK_MS => {
                    warn!("Jellyfin users not refreshed ({}); using the last list", e);
                    Ok(c.users.clone())
                }
                _ => Err(e),
            },
        }
    }

    #[cfg(test)]
    pub async fn set_users(&self, users: Vec<JfUser>) {
        *self.0.users.lock().await = Some(UserCache {
            fetched_at: now_ms(),
            users: Arc::new(users),
        });
    }

    pub fn audit(&self, kind: &'static str, actor: String, detail: String, warn: bool) {
        if warn {
            warn!("chat integration: {} by {}: {}", kind, actor, detail);
        } else {
            info!("chat integration: {} by {}: {}", kind, actor, detail);
        }
        let mut log = lock(&self.0.audit);
        log.push_back(AuditEntry {
            ts: now_ms(),
            kind,
            actor,
            detail,
            warn,
        });
        while log.len() > AUDIT_LEN {
            log.pop_front();
        }
    }

    /// Newest first.
    pub fn audit_log(&self) -> Vec<AuditEntry> {
        lock(&self.0.audit).iter().rev().cloned().collect()
    }

    pub fn heartbeat(&self, provider: &str, bot_name: String) {
        lock(&self.0.sidecars).insert(
            provider.to_string(),
            Heartbeat {
                seen_at: now_ms(),
                bot_name,
            },
        );
    }

    /// Status per provider, for the admin panel.
    pub fn status_json(&self) -> serde_json::Value {
        let now = now_ms();
        let sidecars = lock(&self.0.sidecars).clone();
        let settings = self.0.store.read(|d| d.settings.clone());
        let providers: Vec<_> = config::PROVIDERS
            .iter()
            .map(|(p, var)| {
                let hb = sidecars.get(*p);
                serde_json::json!({
                    "provider": p,
                    "token_var": var,
                    "token_set": self.has_sidecar(p),
                    "sidecar": hb.map(|h| serde_json::json!({
                        "seen_at": h.seen_at,
                        "bot_name": h.bot_name,
                        "online": now.saturating_sub(h.seen_at) < SIDECAR_STALE_MS,
                    })),
                    "settings": settings.for_provider(p),
                })
            })
            .collect();
        serde_json::json!({
            "available": true,
            "listening": self.0.listen_addr.map(|a| a.to_string()),
            "providers": providers,
        })
    }

    fn guards(&self) -> MutexGuard<'_, Guards> {
        lock(&self.0.guards)
    }

    /// Closes chat rooms that have had no members for longer than their
    /// platform's `empty_room_minutes`.
    pub fn spawn_reaper(&self) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(REAPER_INTERVAL_SECS)).await;
                me.close_idle_rooms(now_ms()).await;
            }
        });
    }

    pub async fn close_idle_rooms(&self, now: u64) -> Vec<String> {
        let settings = self.0.store.read(|d| d.settings.clone());
        let closed = {
            let mut rooms = self.0.rooms.write().await;
            let mut clients = self.0.clients.write().await;
            let idle: Vec<String> = rooms
                .values()
                .filter(|r| r.clients.is_empty())
                .filter_map(|r| {
                    let chat = r.chat.as_ref()?;
                    let ttl =
                        settings.for_provider(&chat.provider)?.empty_room_minutes as u64 * 60_000;
                    let since = chat.empty_since.unwrap_or(r.created_at);
                    (now.saturating_sub(since) > ttl).then(|| r.room_id.clone())
                })
                .collect();
            for id in &idle {
                info!("Closing chat room {} (nobody in it for a while)", id);
                let _ = crate::room::ops::close_room(
                    id,
                    "Closed: nobody was watching",
                    &mut rooms,
                    &mut clients,
                );
            }
            idle
        };
        if !closed.is_empty() {
            crate::messaging::broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        }
        closed
    }
}

/// Sets up the chat integration (needs `DATA_DIR` and the Jellyfin device
/// bridge) and starts its API listener when a sidecar token is configured.
pub fn start(
    clients: &Clients,
    rooms: &Rooms,
    bridges: Option<&Bridges>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> IntegrationStatus {
    use config::IntegrationSetup;
    let cfg = match IntegrationSetup::from_env() {
        IntegrationSetup::NotConfigured(reason) => {
            info!("Chat integrations: off ({})", reason);
            return IntegrationStatus::Unavailable(reason);
        }
        IntegrationSetup::Misconfigured(reason) => {
            warn!("Chat integrations: NOT started - {}", reason);
            return IntegrationStatus::Unavailable(reason);
        }
        IntegrationSetup::Enabled(cfg) => cfg,
    };
    let Some(bridges) = bridges else {
        let reason = "Chat integrations need Jellyfin devices (JELLYFIN_URL and JELLYFIN_API_KEY)";
        warn!("Chat integrations: NOT started - {}", reason);
        return IntegrationStatus::Unavailable(reason.into());
    };
    let hub = match Integration::new(&cfg, bridges.clone(), clients.clone(), rooms.clone()) {
        Ok(h) => h,
        Err(e) => {
            warn!("Chat integrations: NOT started - {}", e);
            return IntegrationStatus::Unavailable(e);
        }
    };
    hub.spawn_reaper();
    if cfg.tokens.is_empty() {
        let vars: Vec<&str> = config::PROVIDERS.iter().map(|(_, v)| *v).collect();
        info!(
            "Chat integrations: data in {}; no sidecar token set ({}), so the integration API is not started",
            cfg.data_dir.display(),
            vars.join(" / ")
        );
    } else {
        let state = api::ApiState::new(hub.clone(), &cfg.tokens);
        tokio::spawn(api::serve(state, cfg.addr, shutdown));
    }
    IntegrationStatus::Enabled(hub)
}
