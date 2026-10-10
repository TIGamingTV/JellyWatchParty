//! What chat users can do, and who may do it.
//!
//! Every action first passes `caller` (platform enabled, right server or
//! group and channel, required role, per-account rate limit), then - except
//! linking - `me` (the chat account is linked to an existing, enabled
//! Jellyfin user).
//!
//! Permissions in a chat room:
//! - participants (joined through the chat, with the password if any) may
//!   add and remove their *own* Jellyfin devices, as receiver, or as host
//!   while the room has none;
//! - the owner and admins (Jellyfin administrators, or the optional admin
//!   role) may also pick the host, remove anyone, change the name and
//!   password, hand the room over and close it.

use super::store::{clean_code, ChatSettings, CodeCheck, Link};
use super::{
    clean_display_name, valid_chat_id, valid_external_id, Actor, IntError, IntResult, Integration,
};
use crate::jellyfin::api::normalize_id;
use crate::jellyfin::bridge::{AddError, Role};
use crate::messaging::broadcast_room_list;
use crate::password::verify_password;
use crate::room::ops::{self, OpError};
use crate::types::{ChatParticipant, ChatRoom, ClientKind, PanelRef, Room};
use crate::utils::now_ms;
use axum::http::StatusCode;
use std::collections::HashMap;

/// Most participants a chat room takes.
pub const MAX_PARTICIPANTS: usize = 50;
const MAX_PASSWORD_LEN: usize = 200;
const MAX_USERNAME_LEN: usize = 128;

/// The checked request context.
pub struct Caller {
    pub provider: String,
    pub actor: Actor,
    pub settings: ChatSettings,
    role_admin: bool,
}

impl Caller {
    fn label(&self) -> String {
        format!("{}:{} ({})", self.provider, self.actor.id, self.actor.name)
    }
}

/// A caller linked to a Jellyfin user.
pub struct Me {
    pub caller: Caller,
    pub user_id: String,
    pub user_name: String,
    pub is_admin: bool,
}

impl Me {
    fn label(&self) -> String {
        format!("{} = {}", self.caller.label(), self.user_name)
    }

    fn manages(&self, chat: &ChatRoom) -> bool {
        self.is_admin || chat.owner == self.user_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
    Host,
    Receiver,
}

pub enum KickTarget {
    /// A room member (client id): a device or a web client.
    Member(String),
    /// A participant, by chat account id.
    User(String),
}

pub struct RoomUpdate {
    pub name: Option<String>,
    /// `Some(None)` / `Some(Some(""))` removes the password.
    pub password: Option<Option<String>>,
}

fn not_owner() -> IntError {
    IntError::forbidden(
        "not_owner",
        "Only the room's owner (or an admin) can do that",
    )
}

fn not_participant() -> IntError {
    IntError::forbidden("not_participant", "Join the room first")
}

fn op_error(e: OpError) -> IntError {
    match e {
        OpError::RoomNotFound => IntError::not_found(),
        OpError::RoomFull => IntError::conflict("room_full", "The room is full"),
        OpError::InvalidName => IntError::invalid("Give the room a name"),
        OpError::ClientNotFound | OpError::NotAMember => IntError::new(
            StatusCode::NOT_FOUND,
            "member_not_found",
            "That member isn't in the room anymore",
        ),
    }
}

fn add_error(e: AddError) -> IntError {
    match e {
        AddError::Unavailable(m) => IntError::jellyfin(m),
        AddError::SessionNotFound => IntError::new(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "That device isn't active in Jellyfin anymore; open the app and try again",
        ),
        AddError::RunsWebClient => IntError::conflict(
            "runs_web_client",
            "That app shows the Watch Party panel itself: join the room from there",
        ),
        AddError::NoRemoteControl => IntError::conflict(
            "no_remote_control",
            "That app can't be remote-controlled, so it can only be the host",
        ),
        AddError::AlreadyBridged => IntError::conflict(
            "device_already_bridged",
            "That device is already in a watch party",
        ),
        AddError::NotYourDevice => IntError::forbidden(
            "not_your_device",
            "That device is signed in as someone else",
        ),
        AddError::Op(op) => op_error(op),
    }
}

fn chat_room<'a>(
    rooms: &'a mut HashMap<String, Room>,
    room_id: &str,
    provider: &str,
) -> Result<&'a mut Room, IntError> {
    rooms
        .get_mut(room_id)
        .filter(|r| r.chat.as_ref().is_some_and(|c| c.provider == provider))
        .ok_or_else(IntError::not_found)
}

fn chat(room: &Room) -> &ChatRoom {
    room.chat.as_ref().expect("checked by chat_room")
}

fn chat_mut(room: &mut Room) -> &mut ChatRoom {
    room.chat.as_mut().expect("checked by chat_room")
}

fn clean_password(p: Option<&str>) -> Result<Option<String>, IntError> {
    match p.map(str::trim).filter(|p| !p.is_empty()) {
        None => Ok(None),
        Some(p) if p.chars().count() > MAX_PASSWORD_LEN => Err(IntError::invalid(format!(
            "Passwords can be at most {} characters",
            MAX_PASSWORD_LEN
        ))),
        Some(p) => Ok(Some(p.to_string())),
    }
}

/// Outcome of a link attempt, decided under the store lock.
enum LinkOutcome {
    Linked,
    Taken,
    Wrong { frozen_now: bool },
    Refused,
}

impl Integration {
    /// Checks the platform-side policy for a request.
    pub fn caller(&self, provider: &str, mut actor: Actor) -> Result<Caller, IntError> {
        if !valid_external_id(&actor.id) {
            return Err(IntError::invalid("Missing or invalid account id"));
        }
        actor.name = clean_display_name(&actor.name);
        if let Some(wait) = self
            .guards()
            .rate_limited(&format!("{}:{}", provider, actor.id), now_ms())
        {
            return Err(IntError::wait(
                "rate_limited",
                "Slow down a little and try again in a moment",
                wait,
            ));
        }
        let settings = self
            .store()
            .read(|d| d.settings.for_provider(provider).cloned())
            .ok_or_else(|| IntError::forbidden("not_configured", "Unknown platform"))?;
        if !settings.enabled {
            return Err(IntError::forbidden(
                "disabled",
                "Watch parties from chat are turned off on this server",
            ));
        }
        if settings.guild_id.is_empty() || settings.guild_id != actor.guild_id {
            return Err(IntError::forbidden(
                "wrong_guild",
                "This bot only works in its own server or group",
            ));
        }
        if !settings.channel_ids.is_empty() && !settings.channel_ids.contains(&actor.channel_id) {
            return Err(IntError::forbidden(
                "channel_not_allowed",
                "Use the watch party channel for this",
            ));
        }
        if !settings.required_role_id.is_empty()
            && !actor.roles.contains(&settings.required_role_id)
        {
            return Err(IntError::forbidden(
                "missing_role",
                "You need the watch party role to use this",
            ));
        }
        let role_admin =
            !settings.admin_role_id.is_empty() && actor.roles.contains(&settings.admin_role_id);
        Ok(Caller {
            provider: provider.to_string(),
            actor,
            settings,
            role_admin,
        })
    }

    /// The linked, still existing and enabled Jellyfin user behind a caller.
    pub async fn me(&self, caller: Caller) -> Result<Me, IntError> {
        let (user_id, stored_name) = self
            .store()
            .read(|d| {
                d.linked_user(&caller.provider, &caller.actor.id)
                    .map(|(id, r)| (id.to_string(), r.name.clone()))
            })
            .ok_or_else(|| {
                IntError::forbidden(
                    "not_linked",
                    "Link your Jellyfin account first: use the link command with the code from your admin",
                )
            })?;
        let users = self.users().await.map_err(IntError::jellyfin)?;
        let user = users
            .iter()
            .find(|u| u.id == user_id && !u.is_disabled())
            .ok_or_else(|| {
                IntError::forbidden(
                    "account_disabled",
                    "Your Jellyfin account is disabled or gone; ask an admin",
                )
            })?;
        if user.name != stored_name {
            self.store().update(|d| {
                if let Some(r) = d.users.get_mut(&user_id) {
                    r.name = user.name.clone();
                }
            });
        }
        Ok(Me {
            is_admin: user.is_admin() || caller.role_admin,
            user_id,
            user_name: user.name.clone(),
            caller,
        })
    }

    pub async fn me_for(&self, provider: &str, actor: Actor) -> Result<Me, IntError> {
        let caller = self.caller(provider, actor)?;
        self.me(caller).await
    }

    // --- linking -----------------------------------------------------------

    pub async fn link(
        &self,
        provider: &str,
        actor: Actor,
        username: &str,
        code: &str,
    ) -> IntResult {
        let caller = self.caller(provider, actor)?;
        let key = format!("{}:{}", provider, caller.actor.id);
        if let Some(wait) = self.guards().link_blocked(&key, now_ms()) {
            self.audit(
                "link_blocked",
                caller.label(),
                "link attempt while locked out".into(),
                true,
            );
            return Err(IntError::wait(
                "locked_out",
                format!(
                    "Too many wrong attempts. Try again in {} minutes",
                    wait.div_ceil(60_000)
                ),
                wait,
            ));
        }
        let users = self.users().await.map_err(IntError::jellyfin)?;
        let wanted = username.trim().to_lowercase();
        let target = (!wanted.is_empty() && wanted.chars().count() <= MAX_USERNAME_LEN)
            .then(|| {
                users
                    .iter()
                    .find(|u| u.name.to_lowercase() == wanted && !u.is_disabled())
            })
            .flatten();
        let code = clean_code(code);

        let outcome = match (target, code.as_deref()) {
            (Some(user), Some(code)) => {
                let mac = self.store().code_mac(&user.id, code);
                let link = Link {
                    external_id: caller.actor.id.clone(),
                    display_name: caller.actor.name.clone(),
                    linked_at: now_ms(),
                };
                self.store()
                    .update(|d| match d.check_code(&user.id, &mac) {
                        CodeCheck::Match => {
                            let taken = d
                                .link_of(&user.id, provider)
                                .is_some_and(|l| l.external_id != link.external_id);
                            if taken {
                                LinkOutcome::Taken
                            } else {
                                d.set_link(&user.id, &user.name, provider, link);
                                LinkOutcome::Linked
                            }
                        }
                        CodeCheck::Wrong { frozen_now } => LinkOutcome::Wrong { frozen_now },
                        CodeCheck::NoCode | CodeCheck::Frozen => LinkOutcome::Refused,
                    })
                    .value
            }
            _ => LinkOutcome::Refused,
        };
        let who = target.map(|u| u.name.as_str()).unwrap_or("an unknown user");
        match outcome {
            LinkOutcome::Linked => {
                let user = target.expect("linked implies a target");
                self.guards().clear_link_fails(&key);
                self.audit(
                    "link",
                    caller.label(),
                    format!("linked to {}", user.name),
                    false,
                );
                crate::events::bump();
                Ok(serde_json::json!({ "ok": true, "user_name": user.name }))
            }
            LinkOutcome::Taken => {
                self.audit(
                    "link_refused",
                    caller.label(),
                    format!(
                        "entered the right code for {}, who is linked to another account; consider reassigning the code",
                        who
                    ),
                    true,
                );
                Err(IntError::conflict(
                    "already_linked_elsewhere",
                    "That Jellyfin account is already linked to another account. Ask an admin to unlink it",
                ))
            }
            LinkOutcome::Wrong { frozen_now } => {
                self.guards().record_link_fail(&key, now_ms());
                self.audit(
                    "link_failed",
                    caller.label(),
                    format!("wrong code for {}", who),
                    true,
                );
                if frozen_now {
                    self.audit(
                        "code_frozen",
                        caller.label(),
                        format!(
                            "the code for {} stopped working after too many wrong attempts; assign a new one",
                            who
                        ),
                        true,
                    );
                }
                Err(bad_credentials())
            }
            LinkOutcome::Refused => {
                self.guards().record_link_fail(&key, now_ms());
                self.audit(
                    "link_failed",
                    caller.label(),
                    format!("refused for {} (unknown, disabled, no code or frozen)", who),
                    true,
                );
                Err(bad_credentials())
            }
        }
    }

    pub async fn unlink(&self, provider: &str, actor: Actor) -> IntResult {
        let caller = self.caller(provider, actor)?;
        let removed = self
            .store()
            .update(|d| {
                let id = d
                    .linked_user(provider, &caller.actor.id)
                    .map(|(id, _)| id.to_string());
                id.map(|id| d.unlink(&id, provider))
            })
            .value;
        if removed != Some(true) {
            return Err(IntError::forbidden(
                "not_linked",
                "Your account isn't linked",
            ));
        }
        self.audit(
            "unlink",
            caller.label(),
            "unlinked themselves".into(),
            false,
        );
        crate::events::bump();
        Ok(serde_json::json!({ "ok": true }))
    }

    pub async fn whoami(&self, provider: &str, actor: Actor) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let rooms = self.0.rooms.read().await;
        let mut owns = Vec::new();
        let mut joined = Vec::new();
        for r in rooms.values() {
            let Some(c) = r.chat.as_ref().filter(|c| c.provider == provider) else {
                continue;
            };
            if c.owner == me.user_id {
                owns.push(r.room_id.clone());
            } else if c.is_participant(&me.user_id) {
                joined.push(r.room_id.clone());
            }
        }
        Ok(serde_json::json!({
            "user_id": me.user_id,
            "user_name": me.user_name,
            "is_admin": me.is_admin,
            "owns": owns,
            "joined": joined,
        }))
    }

    // --- rooms -------------------------------------------------------------

    pub async fn create_room(
        &self,
        provider: &str,
        actor: Actor,
        name: &str,
        password: Option<&str>,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let password = clean_password(password)?;
        if me.caller.settings.require_password && password.is_none() {
            return Err(IntError::invalid("Rooms need a password on this server")
                .with_reason("password_required"));
        }
        let id = {
            let mut rooms = self.0.rooms.write().await;
            let chat_rooms: Vec<&ChatRoom> = rooms
                .values()
                .filter_map(|r| r.chat.as_ref())
                .filter(|c| c.provider == provider)
                .collect();
            let owned = chat_rooms.iter().filter(|c| c.owner == me.user_id).count();
            if !me.is_admin && owned >= me.caller.settings.max_rooms_per_user as usize {
                return Err(IntError::conflict(
                    "room_limit",
                    format!("You already have {} room(s); close one first", owned),
                ));
            }
            if chat_rooms.len() >= me.caller.settings.max_rooms_total as usize {
                return Err(IntError::conflict(
                    "room_limit",
                    "There are too many rooms right now; try again later",
                ));
            }
            let chat = ChatRoom {
                provider: provider.to_string(),
                owner: me.user_id.clone(),
                owner_name: me.user_name.clone(),
                participants: vec![ChatParticipant {
                    user_id: me.user_id.clone(),
                    name: me.user_name.clone(),
                }],
                panel: None,
                empty_since: None,
            };
            ops::create_chat_room(name, password.as_deref(), chat, &mut rooms).map_err(op_error)?
        };
        self.audit(
            "room_create",
            me.label(),
            format!("created room {}", self.room_label(&id).await),
            false,
        );
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        let name = self.0.rooms.read().await.get(&id).map(|r| r.name.clone());
        Ok(serde_json::json!({ "ok": true, "id": id, "name": name }))
    }

    pub async fn join_room(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        password: Option<&str>,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            if chat(room).is_participant(&me.user_id) {
                return Ok(serde_json::json!({ "ok": true, "already": true }));
            }
            if chat(room).participants.len() >= MAX_PARTICIPANTS {
                return Err(IntError::conflict("room_full", "The room is full"));
            }
            if room.password_hash.is_some() && !me.is_admin {
                let now = now_ms();
                if let Some(wait) =
                    crate::ws::lockout_remaining_ms(&room.failed_joins, &me.user_id, now)
                {
                    return Err(IntError::wait(
                        "too_many_attempts",
                        format!(
                            "Too many wrong passwords. Try again in {}s",
                            wait.div_ceil(1000)
                        ),
                        wait,
                    ));
                }
                let ok = room.password_hash.as_ref().is_some_and(|(salt, hash)| {
                    verify_password(password.unwrap_or(""), salt, hash)
                });
                if !ok {
                    crate::ws::record_failed_join(&mut room.failed_joins, &me.user_id, now);
                    return Err(IntError::forbidden("wrong_password", "Wrong room password"));
                }
                room.failed_joins.remove(&me.user_id);
            }
            chat_mut(room).participants.push(ChatParticipant {
                user_id: me.user_id.clone(),
                name: me.user_name.clone(),
            });
        }
        self.audit(
            "room_join",
            me.label(),
            format!("joined room {}", self.room_label(room_id).await),
            false,
        );
        crate::events::bump();
        Ok(serde_json::json!({ "ok": true }))
    }

    /// `'Name' (id prefix)` for the audit log.
    async fn room_label(&self, room_id: &str) -> String {
        let short: String = room_id.chars().take(8).collect();
        match self.0.rooms.read().await.get(room_id) {
            Some(r) => format!("'{}' ({})", r.name, short),
            None => short,
        }
    }

    /// Bridges in `room_id` whose device belongs to `user_id`.
    async fn devices_of(&self, room_id: &str, user_id: &str) -> Vec<String> {
        let owners = self.bridges().owners();
        let rooms = self.0.rooms.read().await;
        rooms
            .get(room_id)
            .map(|r| {
                r.clients
                    .iter()
                    .filter(|id| owners.get(*id).is_some_and(|o| o == user_id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn leave_room(&self, provider: &str, actor: Actor, room_id: &str) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            let c = chat_mut(room);
            if c.owner == me.user_id {
                return Err(IntError::conflict(
                    "owner_cannot_leave",
                    "You own this room: hand it over to someone or close it",
                ));
            }
            if !c.is_participant(&me.user_id) {
                return Err(not_participant());
            }
            c.participants.retain(|p| p.user_id != me.user_id);
        }
        for id in self.devices_of(room_id, &me.user_id).await {
            self.bridges().remove(&id).await;
        }
        self.audit(
            "room_leave",
            me.label(),
            format!("left room {}", self.room_label(room_id).await),
            false,
        );
        crate::events::bump();
        Ok(serde_json::json!({ "ok": true }))
    }

    pub async fn update_room(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        update: RoomUpdate,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let password = match update.password {
            None => None,
            Some(p) => Some(clean_password(p.as_deref())?),
        };
        if me.caller.settings.require_password && matches!(password, Some(None)) {
            return Err(IntError::invalid("Rooms need a password on this server")
                .with_reason("password_required"));
        }
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            if !me.manages(chat(room)) {
                return Err(not_owner());
            }
            let pw = password.as_ref().map(|p| p.as_deref());
            ops::update_room(room_id, update.name.as_deref(), pw, &mut rooms).map_err(op_error)?;
        }
        self.audit(
            "room_update",
            me.label(),
            format!(
                "updated room {} (name: {}, password: {})",
                self.room_label(room_id).await,
                update.name.is_some(),
                match &password {
                    None => "kept",
                    Some(None) => "removed",
                    Some(Some(_)) => "changed",
                }
            ),
            false,
        );
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        Ok(serde_json::json!({ "ok": true }))
    }

    pub async fn close_room(&self, provider: &str, actor: Actor, room_id: &str) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let label = self.room_label(room_id).await;
        {
            let mut rooms = self.0.rooms.write().await;
            let mut clients = self.0.clients.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            if !me.manages(chat(room)) {
                return Err(not_owner());
            }
            ops::close_room(
                room_id,
                "The room owner closed the room",
                &mut rooms,
                &mut clients,
            )
            .map_err(op_error)?;
        }
        self.audit(
            "room_close",
            me.label(),
            format!("closed room {}", label),
            false,
        );
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        Ok(serde_json::json!({ "ok": true }))
    }

    /// The Jellyfin user linked to a chat account.
    fn linked(&self, provider: &str, external_id: &str) -> Option<(String, String)> {
        self.store().read(|d| {
            d.linked_user(provider, external_id)
                .map(|(id, r)| (id.to_string(), r.name.clone()))
        })
    }

    pub async fn transfer_room(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        to_external_id: &str,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let (to_id, to_name) = self.linked(provider, to_external_id).ok_or_else(|| {
            IntError::forbidden(
                "target_not_linked",
                "That person hasn't linked their Jellyfin account",
            )
        })?;
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            if !me.manages(chat(room)) {
                return Err(not_owner());
            }
            let c = chat_mut(room);
            if !c.is_participant(&to_id) {
                return Err(IntError::forbidden(
                    "not_participant",
                    "They need to join the room first",
                ));
            }
            c.owner = to_id;
            c.owner_name = to_name.clone();
        }
        self.audit(
            "room_transfer",
            me.label(),
            format!(
                "handed room {} to {}",
                self.room_label(room_id).await,
                to_name
            ),
            false,
        );
        crate::events::bump();
        Ok(serde_json::json!({ "ok": true }))
    }

    pub async fn kick(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        target: KickTarget,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            if !me.manages(chat(room)) {
                return Err(not_owner());
            }
        }
        match target {
            KickTarget::Member(member) => {
                self.remove_member(room_id, &member, "The room owner removed you")
                    .await?;
                self.audit(
                    "room_kick",
                    me.label(),
                    format!(
                        "removed member {} from room {}",
                        member,
                        self.room_label(room_id).await
                    ),
                    false,
                );
            }
            KickTarget::User(external_id) => {
                let (user_id, user_name) =
                    self.linked(provider, &external_id).ok_or_else(|| {
                        IntError::new(
                            StatusCode::NOT_FOUND,
                            "member_not_found",
                            "That person isn't in the room",
                        )
                    })?;
                {
                    let mut rooms = self.0.rooms.write().await;
                    let room = chat_room(&mut rooms, room_id, provider)?;
                    let c = chat_mut(room);
                    if c.owner == user_id {
                        return Err(IntError::conflict(
                            "is_owner",
                            "The owner can't be removed; hand the room over first",
                        ));
                    }
                    if !c.is_participant(&user_id) {
                        return Err(IntError::new(
                            StatusCode::NOT_FOUND,
                            "member_not_found",
                            "That person isn't in the room",
                        ));
                    }
                    c.participants.retain(|p| p.user_id != user_id);
                }
                for id in self.devices_of(room_id, &user_id).await {
                    self.bridges().remove(&id).await;
                }
                self.audit(
                    "room_kick",
                    me.label(),
                    format!(
                        "removed {} from room {}",
                        user_name,
                        self.room_label(room_id).await
                    ),
                    false,
                );
                crate::events::bump();
            }
        }
        Ok(serde_json::json!({ "ok": true }))
    }

    /// Takes a member out of a room: a bridged device stops being driven,
    /// anyone else is sent back to their lobby.
    async fn remove_member(
        &self,
        room_id: &str,
        member: &str,
        reason: &str,
    ) -> Result<(), IntError> {
        let is_bridge = {
            let rooms = self.0.rooms.read().await;
            let clients = self.0.clients.read().await;
            let room = rooms.get(room_id).ok_or_else(IntError::not_found)?;
            if !room.clients.iter().any(|c| c == member) {
                return Err(op_error(OpError::NotAMember));
            }
            clients
                .get(member)
                .is_some_and(|c| c.kind == ClientKind::Bridge)
        };
        if is_bridge && self.bridges().remove(member).await {
            return Ok(());
        }
        {
            let mut rooms = self.0.rooms.write().await;
            let mut clients = self.0.clients.write().await;
            ops::kick_member(room_id, member, reason, &mut rooms, &mut clients)
                .map_err(op_error)?;
        }
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        Ok(())
    }

    pub async fn set_host(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        member: &str,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        {
            let mut rooms = self.0.rooms.write().await;
            let clients = self.0.clients.read().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            if !me.manages(chat(room)) {
                return Err(not_owner());
            }
            ops::set_host(room_id, member, &mut rooms, &clients).map_err(op_error)?;
        }
        self.audit(
            "room_host",
            me.label(),
            format!(
                "made {} host of room {}",
                member,
                self.room_label(room_id).await
            ),
            false,
        );
        broadcast_room_list(&self.0.clients, &self.0.rooms).await;
        Ok(serde_json::json!({ "ok": true }))
    }

    // --- devices -----------------------------------------------------------

    /// The caller's own Jellyfin sessions that could join a room.
    pub async fn devices(&self, provider: &str, actor: Actor) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let snap = self.bridges().snapshot_for_admin().await;
        if let Some(e) = &snap.error {
            return Err(IntError::jellyfin(e.clone()));
        }
        let now = now_ms();
        let bridged: HashMap<String, (String, Option<String>)> = {
            let clients = self.0.clients.read().await;
            clients
                .iter()
                .filter(|(_, c)| c.room_id.is_some())
                .filter_map(|(id, c)| {
                    c.bridge_device
                        .clone()
                        .map(|d| (d, (id.clone(), c.room_id.clone())))
                })
                .collect()
        };
        let devices: Vec<_> = snap
            .sessions
            .iter()
            .filter(|s| s.user_id() == me.user_id && !s.is_own() && !s.runs_web_client())
            .filter(|s| {
                bridged.contains_key(s.device_id()) || s.recently_active(snap.clock_offset_ms, now)
            })
            .map(|s| {
                let b = bridged.get(s.device_id());
                serde_json::json!({
                    "session_id": s.id,
                    "device_name": s.device_name(),
                    "client": s.client_name(),
                    "remote_control": s.supports_remote_control,
                    "now_playing": s.item_name(),
                    "bridged_as": b.map(|b| &b.0),
                    "room_id": b.and_then(|b| b.1.as_ref()),
                })
            })
            .collect();
        Ok(serde_json::json!({ "devices": devices }))
    }

    pub async fn add_device(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        session_id: &str,
        role: DeviceRole,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let s = &me.caller.settings;
        let allowed = match role {
            DeviceRole::Host => s.allow_host,
            DeviceRole::Receiver => s.allow_receiver,
        };
        if !allowed {
            return Err(IntError::forbidden(
                "role_not_allowed",
                match role {
                    DeviceRole::Host => "Devices can't be added as host on this server",
                    DeviceRole::Receiver => "Devices can't be added as receiver on this server",
                },
            ));
        }
        if session_id.is_empty() || session_id.len() > 100 {
            return Err(IntError::invalid("Pick one of your devices"));
        }
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            let c = chat(room);
            if !me.is_admin && !c.is_participant(&me.user_id) {
                return Err(not_participant());
            }
            if role == DeviceRole::Host && !me.manages(c) && !room.is_hostless() {
                return Err(IntError::forbidden(
                    "not_owner",
                    "The room already has a host; only its owner can change that. Add your device as receiver",
                ));
            }
        }
        let bridge_role = match role {
            DeviceRole::Host => Role::Host,
            DeviceRole::Receiver => Role::Receiver,
        };
        let client_id = self
            .bridges()
            .add(room_id, session_id, bridge_role, Some(&me.user_id))
            .await
            .map_err(add_error)?;
        self.audit(
            "device_add",
            me.label(),
            format!(
                "added a device to room {} as {:?} (member {})",
                self.room_label(room_id).await,
                role,
                client_id
            ),
            false,
        );
        Ok(serde_json::json!({ "ok": true, "member_id": client_id }))
    }

    pub async fn remove_device(
        &self,
        provider: &str,
        actor: Actor,
        room_id: &str,
        member: &str,
    ) -> IntResult {
        let me = self.me_for(provider, actor).await?;
        let manages = {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            me.manages(chat(room))
        };
        let own = self
            .bridges()
            .owners()
            .get(member)
            .is_some_and(|o| *o == me.user_id);
        if !own && !manages {
            return Err(IntError::forbidden(
                "not_your_device",
                "You can only remove your own devices",
            ));
        }
        self.remove_member(room_id, member, "The room owner removed you")
            .await?;
        self.audit(
            "device_remove",
            me.label(),
            format!(
                "removed member {} from room {}",
                member,
                self.room_label(room_id).await
            ),
            false,
        );
        Ok(serde_json::json!({ "ok": true }))
    }

    /// Remembers where a room's panel message is (sidecar bookkeeping).
    pub async fn set_panel(
        &self,
        provider: &str,
        room_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> IntResult {
        if !valid_chat_id(channel_id) || !valid_external_id(message_id) {
            return Err(IntError::invalid("Invalid channel or message id"));
        }
        {
            let mut rooms = self.0.rooms.write().await;
            let room = chat_room(&mut rooms, room_id, provider)?;
            chat_mut(room).panel = Some(PanelRef {
                channel_id: channel_id.to_string(),
                message_id: message_id.to_string(),
            });
        }
        crate::events::bump();
        Ok(serde_json::json!({ "ok": true }))
    }

    // --- admin -------------------------------------------------------------

    /// Assigns a fresh code to a Jellyfin user (dropping their links).
    /// Returns the code: it is shown to the admin once and never stored.
    pub async fn assign_code(&self, user_id: &str) -> Result<String, String> {
        let user_id = normalize_id(user_id);
        let users = self.fresh_users().await?;
        let user = users
            .iter()
            .find(|u| u.id == user_id)
            .ok_or("No such Jellyfin user")?;
        let code = super::store::generate_code();
        let mac = self.store().code_mac(&user.id, &code);
        let saved = self
            .store()
            .update(|d| d.assign_code(&user.id, &user.name, mac, now_ms()))
            .saved;
        saved.map_err(|e| format!("Not saved: {}", e))?;
        self.audit(
            "code_assign",
            "admin".into(),
            format!("assigned a new code to {}", user.name),
            false,
        );
        crate::events::bump();
        Ok(code)
    }
}

fn bad_credentials() -> IntError {
    IntError::forbidden(
        "bad_credentials",
        "That username or code is wrong. Check them, or ask an admin for a new code",
    )
}

impl IntError {
    fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = reason;
        self
    }
}
