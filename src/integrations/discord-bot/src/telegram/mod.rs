//! The Telegram bot: commands and buttons in one configured group, private
//! chats for everything secret (link codes, room passwords) and personal
//! (device lists), and one live panel message per room in the group.
//!
//! Telegram has no private replies in groups and no forms, so anything
//! that needs one continues in the user's private chat with the bot. If
//! the bot may not write there first (the user never pressed Start), the
//! button opens that chat with a `/start` payload that picks up where the
//! user left off.

mod client;
mod flush;
mod handler;
mod ids;
mod panel;
mod text;
mod types;

use crate::api::{Api, Room};
use crate::core::{self, Core, Platform};
use client::{Client, TgError};
use flush::TgPanels;
use ids::Picks;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use types::{ChatMember, ChatMemberUpdated, Update};

/// Long poll length for `getUpdates`, in seconds.
const POLL_SECS: u64 = 50;
const CONV_TTL: Duration = Duration::from_secs(10 * 60);
const MEMBER_TTL: Duration = Duration::from_secs(60);
const RETRY_AFTER: Duration = Duration::from_secs(5);

/// The command menus.
const PRIVATE_COMMANDS: &[(&str, &str)] = &[
    (
        "rooms",
        "The rooms, with buttons to join and add your devices",
    ),
    ("newroom", "Create a room"),
    (
        "link",
        "Link your Jellyfin account (with the code from your admin)",
    ),
    ("whoami", "Your linked Jellyfin account and rooms"),
    ("unlink", "Unlink your Telegram account"),
    ("cancel", "Stop what you're doing"),
    ("help", "What I can do"),
];
const GROUP_COMMANDS: &[(&str, &str)] = &[
    ("newroom", "Create a room (its panel is posted here)"),
    ("rooms", "List the rooms"),
    ("link", "Link your Jellyfin account (in a private chat)"),
    ("groupid", "This group's ID, for the admin panel"),
    ("help", "What I can do"),
];
const OTHER_GROUP_COMMANDS: &[(&str, &str)] = &[
    ("groupid", "This group's ID, for the admin panel"),
    ("help", "What I can do"),
];

/// A private conversation: what the bot asked for and is waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conv {
    LinkName,
    LinkCode { username: String },
    NewName { topic: Option<i64> },
    NewPassword { name: String, topic: Option<i64> },
    JoinPassword { room: String },
    Rename { room: String },
    SetPassword { room: String },
}

/// The Telegram platform's state. Derefs to `Core` (API, settings, rooms).
pub struct Tg {
    core: Core,
    client: Client,
    /// The bot's @username, for links and `/cmd@bot`.
    username: String,
    panels: Mutex<TgPanels>,
    convs: Mutex<HashMap<i64, (Instant, Conv)>>,
    picks: Mutex<Picks>,
    members: Mutex<HashMap<(i64, i64), (Instant, ChatMember)>>,
    /// `Some(group)` once the command menus are set (for that group).
    registered: tokio::sync::Mutex<Option<Option<i64>>>,
    /// Telegram asked to slow down until then.
    paused_until: Mutex<Option<Instant>>,
    /// One update at a time per user, so a conversation stays in order.
    user_locks: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
}

impl std::ops::Deref for Tg {
    type Target = Core;

    fn deref(&self) -> &Core {
        &self.core
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Platform for Tg {
    fn core(&self) -> &Core {
        &self.core
    }

    fn rooms_changed(&self, rooms: &[Room]) {
        flush::apply(&mut self.panels(), rooms);
    }

    async fn settings_checked(&self) {
        self.sync_commands().await;
    }
}

impl Tg {
    fn new(api: Api, client: Client, username: String) -> Self {
        Self {
            core: Core::new("Telegram", api),
            client,
            username,
            panels: Mutex::new(flush::new_panels()),
            convs: Mutex::new(HashMap::new()),
            picks: Mutex::new(Picks::default()),
            members: Mutex::new(HashMap::new()),
            registered: tokio::sync::Mutex::new(None),
            paused_until: Mutex::new(None),
            user_locks: Mutex::new(HashMap::new()),
        }
    }

    fn panels(&self) -> MutexGuard<'_, TgPanels> {
        lock(&self.panels)
    }

    /// The configured group, if any.
    fn group_id(&self) -> Option<i64> {
        self.settings()
            .and_then(|s| s.guild_id.parse::<i64>().ok())
            .filter(|g| *g < 0)
    }

    fn set_conv(&self, user: i64, conv: Conv) {
        let mut c = lock(&self.convs);
        c.retain(|_, (at, _)| at.elapsed() < CONV_TTL);
        c.insert(user, (Instant::now(), conv));
    }

    fn take_conv(&self, user: i64) -> Option<Conv> {
        lock(&self.convs)
            .remove(&user)
            .filter(|(at, _)| at.elapsed() < CONV_TTL)
            .map(|(_, c)| c)
    }

    fn pause(&self, secs: u64) {
        *lock(&self.paused_until) = Some(Instant::now() + Duration::from_secs(secs));
    }

    fn paused(&self) -> bool {
        lock(&self.paused_until).is_some_and(|t| Instant::now() < t)
    }

    /// Whether `user` is in `chat` (briefly cached).
    async fn member(&self, chat: i64, user: i64) -> Result<ChatMember, TgError> {
        if let Some((at, m)) = lock(&self.members).get(&(chat, user)) {
            if at.elapsed() < MEMBER_TTL {
                return Ok(m.clone());
            }
        }
        let m = self.client.get_chat_member(chat, user).await?;
        let mut cache = lock(&self.members);
        cache.retain(|_, (at, _)| at.elapsed() < MEMBER_TTL);
        cache.insert((chat, user), (Instant::now(), m.clone()));
        Ok(m)
    }

    fn user_lock(&self, user: i64) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = lock(&self.user_locks);
        if locks.len() > 1000 {
            locks.retain(|_, l| Arc::strong_count(l) > 1);
        }
        locks.entry(user).or_default().clone()
    }

    /// Sets the command menus: for private chats and other groups once,
    /// and the full one in the configured group (moved when it changes).
    async fn sync_commands(&self) {
        let want = self.group_id();
        let mut reg = self.registered.lock().await;
        if *reg == Some(want) {
            return;
        }
        if reg.is_none() {
            let global = async {
                self.client
                    .set_commands(PRIVATE_COMMANDS, json!({ "type": "all_private_chats" }))
                    .await?;
                self.client
                    .set_commands(OTHER_GROUP_COMMANDS, json!({ "type": "all_group_chats" }))
                    .await
            };
            if let Err(e) = global.await {
                log::warn!("Telegram: could not set the command menus: {}", e);
                return;
            }
        }
        if let Some(Some(old)) = *reg {
            if Some(old) != want {
                let _ = self
                    .client
                    .delete_commands(json!({ "type": "chat", "chat_id": old }))
                    .await;
            }
        }
        *reg = Some(None);
        if let Some(g) = want {
            match self
                .client
                .set_commands(GROUP_COMMANDS, json!({ "type": "chat", "chat_id": g }))
                .await
            {
                Ok(()) => {
                    log::info!("Telegram: commands set up in group {}", g);
                    *reg = Some(Some(g));
                }
                Err(e) => log::error!(
                    "Telegram: could not set the commands in group {} (is the bot in that group?): {}",
                    g,
                    e
                ),
            }
        }
    }

    async fn handle(self: Arc<Self>, u: Update) {
        if let Some(m) = u.my_chat_member {
            self.on_membership(m);
            return;
        }
        let lock = u.user_id().map(|id| self.user_lock(id));
        let _guard = match &lock {
            Some(l) => Some(l.lock().await),
            None => None,
        };
        if let Some(m) = u.message {
            self.on_message(m).await;
        } else if let Some(c) = u.callback_query {
            self.on_callback(c).await;
        }
    }

    fn on_membership(&self, m: ChatMemberUpdated) {
        if !m.chat.is_group() {
            return;
        }
        let title = m.chat.title.unwrap_or_default();
        if !m.new_chat_member.in_chat() {
            log::warn!("Telegram: removed from group \"{}\" ({})", title, m.chat.id);
        } else if Some(m.chat.id) == self.group_id() {
            log::info!("Telegram: added to the watch party group \"{}\"", title);
        } else {
            log::info!(
                "Telegram: added to group \"{}\". To run watch parties there, put its ID {} in the admin panel under Telegram bot > Group ID",
                title,
                m.chat.id
            );
        }
    }
}

/// Runs the Telegram bot until `stop` turns true (`Ok`), or until Telegram
/// refuses its token (`Err`).
pub async fn run(
    token: String,
    telegram_api: String,
    api: Api,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let client = Client::new(&telegram_api, &token)?;
    let me = loop {
        match client.get_me().await {
            Ok(me) => break me,
            Err(TgError::Unauthorized) => return Err("Telegram refused TELEGRAM_BOT_TOKEN".into()),
            Err(e) => log::warn!("Telegram not reachable yet: {}", e),
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            _ = stop.wait_for(|s| *s) => return Ok(()),
        }
    };
    let username = me
        .username
        .clone()
        .ok_or("Telegram says this bot has no username")?;
    log::info!("Connected to Telegram as @{}", username);
    if let Err(e) = client.delete_webhook().await {
        log::warn!(
            "Telegram: could not remove a webhook (updates may not arrive): {}",
            e
        );
    }

    let tg = Arc::new(Tg::new(api, client, username.clone()));
    tg.set_bot_name(&format!("@{}", username));
    let tasks = [
        tokio::spawn(core::config_loop(tg.clone())),
        tokio::spawn(core::rooms_loop(tg.clone())),
        tokio::spawn(flush::flush_loop(tg.clone())),
    ];

    let mut offset: Option<i64> = None;
    let result = loop {
        let res = tokio::select! {
            r = tg.client.get_updates(offset, POLL_SECS) => r,
            _ = stop.wait_for(|s| *s) => break Ok(()),
        };
        match res {
            Ok(updates) => {
                for u in updates {
                    offset = Some(u.update_id + 1);
                    tokio::spawn(tg.clone().handle(u));
                }
            }
            Err(TgError::Unauthorized) => {
                break Err("Telegram refused TELEGRAM_BOT_TOKEN".to_string())
            }
            Err(TgError::RetryAfter(s)) => tokio::time::sleep(Duration::from_secs(s)).await,
            Err(e) => {
                if e.to_string().contains("Conflict") {
                    log::warn!(
                        "Telegram: another program is fetching this bot's updates (a second bot container with the same token?): {}",
                        e
                    );
                } else {
                    log::warn!("Telegram updates: {}", e);
                }
                tokio::time::sleep(RETRY_AFTER).await;
            }
        }
    };
    // Confirm the updates already handled, so a restart doesn't repeat them.
    if offset.is_some() {
        let _ = tg.client.get_updates(offset, 0).await;
    }
    for t in tasks {
        t.abort();
    }
    result
}
