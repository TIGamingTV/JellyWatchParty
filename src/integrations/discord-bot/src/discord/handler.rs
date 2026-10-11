//! Discord interactions: slash commands, autocomplete, buttons, menus and
//! modals. Every reply is ephemeral (only the clicker sees it) and sent
//! without mentions; the public room panels are handled in `sync.rs`.

use super::commands::{self, Invocation};
use super::flush::{self, DiscordPanels};
use super::ids::{self, Id};
use super::panel::{self, Style};
use super::text::{escape, fit, join_within, label, MESSAGE_MAX};
use crate::api::{room_path, Actor, Api, ApiError, Device, Room};
use crate::core::{Core, Platform};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use twilight_http::Client;
use twilight_model::application::command::{CommandOptionChoice, CommandOptionChoiceValue};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::{
    ActionRow, Component, SelectMenu, SelectMenuOption, SelectMenuType, TextInput, TextInputStyle,
};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ApplicationMarker, ChannelMarker, GuildMarker};
use twilight_model::id::Id as DcId;

const DEVICE_CACHE: Duration = Duration::from_secs(15);

/// State shared by the interaction handler and the background loops.
/// Derefs to the platform-independent `Core` (API, settings, rooms).
pub struct Shared {
    pub core: Core,
    pub http: Arc<Client>,
    pub app_id: DcId<ApplicationMarker>,
    pub panels: Mutex<DiscordPanels>,
    pub registered_guild: tokio::sync::Mutex<Option<u64>>,
    devices: Mutex<HashMap<String, (Instant, Vec<Device>)>>,
}

impl std::ops::Deref for Shared {
    type Target = Core;

    fn deref(&self) -> &Core {
        &self.core
    }
}

impl Platform for Shared {
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

impl Shared {
    pub fn new(api: Api, http: Arc<Client>, app_id: DcId<ApplicationMarker>) -> Self {
        Self {
            core: Core::new("Discord", api),
            http,
            app_id,
            panels: Mutex::new(flush::new_panels()),
            registered_guild: tokio::sync::Mutex::new(None),
            devices: Mutex::new(HashMap::new()),
        }
    }

    pub fn panels(&self) -> std::sync::MutexGuard<'_, DiscordPanels> {
        self.panels.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The caller's devices, briefly cached (autocomplete asks on every key).
    async fn devices(&self, actor: &Actor, fresh: bool) -> Result<Vec<Device>, ApiError> {
        if !fresh {
            let cache = self.devices.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, d)) = cache.get(&actor.id) {
                if at.elapsed() < DEVICE_CACHE {
                    return Ok(d.clone());
                }
            }
        }
        let list = self.api.devices(actor).await?;
        let mut cache = self.devices.lock().unwrap_or_else(|e| e.into_inner());
        cache.retain(|_, (at, _)| at.elapsed() < DEVICE_CACHE);
        cache.insert(actor.id.clone(), (Instant::now(), list.clone()));
        Ok(list)
    }

    fn forget_devices(&self, actor: &Actor) {
        self.devices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&actor.id);
    }

    /// Registers `/jwp` in the configured server (and removes it from a
    /// server that is no longer configured).
    pub async fn sync_commands(&self) {
        let want = self
            .settings()
            .and_then(|s| s.guild_id.parse::<u64>().ok())
            .and_then(DcId::<GuildMarker>::new_checked);
        let mut reg = self.registered_guild.lock().await;
        if *reg == want.map(|g| g.get()) {
            return;
        }
        let client = self.http.interaction(self.app_id);
        if let Some(old) = reg.and_then(DcId::<GuildMarker>::new_checked) {
            if let Err(e) = client.set_guild_commands(old, &[]).await {
                log::warn!("could not remove the commands from server {}: {}", old, e);
            }
        }
        *reg = None;
        if let Some(g) = want {
            match client.set_guild_commands(g, &[commands::definition()]).await {
                Ok(_) => {
                    log::info!("/{} registered in server {}", commands::NAME, g);
                    *reg = Some(g.get());
                }
                Err(e) => log::error!(
                    "could not register /{} in server {} (is the bot in that server, invited with the applications.commands scope?): {}",
                    commands::NAME,
                    g,
                    e
                ),
            }
        }
    }
}

fn no_mentions() -> AllowedMentions {
    AllowedMentions::default()
}

/// What a flow answers: text and optional components (a menu, a button).
pub struct Out {
    text: String,
    components: Vec<Component>,
}

impl From<String> for Out {
    fn from(text: String) -> Self {
        Self {
            text,
            components: Vec::new(),
        }
    }
}

impl From<&str> for Out {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

fn err(e: ApiError) -> Out {
    e.message.into()
}

fn row(c: Component) -> Component {
    Component::ActionRow(ActionRow {
        components: vec![c],
    })
}

fn select(id: Id, placeholder: &str, options: Vec<SelectMenuOption>) -> Component {
    row(Component::SelectMenu(SelectMenu {
        channel_types: None,
        custom_id: id.encode(),
        default_values: None,
        disabled: false,
        kind: SelectMenuType::Text,
        max_values: Some(1),
        min_values: Some(1),
        options: Some(options),
        placeholder: Some(placeholder.to_string()),
    }))
}

fn option(label_text: &str, value: String, description: Option<String>) -> SelectMenuOption {
    SelectMenuOption {
        default: false,
        description: description.map(|d| label(&d)),
        emoji: None,
        label: label(label_text),
        value,
    }
}

fn input(id: &str, label: &str, required: bool, max: u16) -> Component {
    row(Component::TextInput(TextInput {
        custom_id: id.to_string(),
        label: label.to_string(),
        max_length: Some(max),
        min_length: None,
        placeholder: None,
        required: Some(required),
        style: TextInputStyle::Short,
        value: None,
    }))
}

/// A form: (custom id, title, inputs).
pub struct Modal {
    id: Id,
    title: String,
    inputs: Vec<Component>,
}

fn link_modal() -> Modal {
    Modal {
        id: Id::LinkModal,
        title: "Link your Jellyfin account".into(),
        inputs: vec![
            input("username", "Your Jellyfin user name", true, 128),
            input("code", "The 4-digit code from your admin", true, 9),
        ],
    }
}

fn create_modal(require_password: bool) -> Modal {
    Modal {
        id: Id::CreateModal,
        title: "New watch party".into(),
        inputs: vec![
            input("name", "Room name", true, 100),
            input(
                "password",
                if require_password {
                    "Password (required)"
                } else {
                    "Password (empty: an open room)"
                },
                require_password,
                200,
            ),
        ],
    }
}

fn join_modal(room: &Room) -> Modal {
    Modal {
        id: Id::JoinModal(room.id.clone()),
        title: label(&format!("Join {}", room.name))
            .chars()
            .take(45)
            .collect(),
        inputs: vec![input("password", "Room password", true, 200)],
    }
}

fn password_modal(room_id: &str, required: bool) -> Modal {
    Modal {
        id: Id::PasswordModal(room_id.to_string()),
        title: "Room password".into(),
        inputs: vec![input(
            "password",
            if required {
                "New password"
            } else {
                "New password (empty: no password)"
            },
            required,
            200,
        )],
    }
}

/// One interaction being answered.
struct Ix<'a> {
    shared: &'a Shared,
    ix: &'a Interaction,
}

impl Ix<'_> {
    async fn respond(&self, kind: InteractionResponseType, data: Option<InteractionResponseData>) {
        let res = self
            .shared
            .http
            .interaction(self.shared.app_id)
            .create_response(
                self.ix.id,
                &self.ix.token,
                &InteractionResponse { kind, data },
            )
            .await;
        if let Err(e) = res {
            log::warn!("could not answer an interaction: {}", e);
        }
    }

    fn ephemeral() -> InteractionResponseData {
        InteractionResponseData {
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(no_mentions()),
            ..Default::default()
        }
    }

    /// "Thinking..." visible only to the user.
    async fn defer(&self) {
        self.respond(
            InteractionResponseType::DeferredChannelMessageWithSource,
            Some(Self::ephemeral()),
        )
        .await;
    }

    /// Acknowledges a click on a private message; `edit` then replaces it.
    async fn defer_update(&self) {
        self.respond(InteractionResponseType::DeferredUpdateMessage, None)
            .await;
    }

    async fn edit(&self, out: Out) {
        let mentions = no_mentions();
        // Longer text is refused, which would leave "thinking..." forever.
        let text = fit(&out.text, MESSAGE_MAX);
        let res = self
            .shared
            .http
            .interaction(self.shared.app_id)
            .update_response(&self.ix.token)
            .content(Some(&text))
            .components(Some(&out.components))
            .allowed_mentions(Some(&mentions))
            .await;
        if let Err(e) = res {
            log::warn!("could not edit an interaction reply: {}", e);
        }
    }

    async fn reply_now(&self, out: Out) {
        let data = InteractionResponseData {
            content: Some(fit(&out.text, MESSAGE_MAX)),
            components: Some(out.components),
            ..Self::ephemeral()
        };
        self.respond(
            InteractionResponseType::ChannelMessageWithSource,
            Some(data),
        )
        .await;
    }

    async fn modal(&self, m: Modal) {
        let data = InteractionResponseData {
            custom_id: Some(m.id.encode()),
            title: Some(m.title),
            components: Some(m.inputs),
            ..Default::default()
        };
        self.respond(InteractionResponseType::Modal, Some(data))
            .await;
    }

    async fn choices(&self, choices: Vec<(String, String)>) {
        let data = InteractionResponseData {
            choices: Some(
                choices
                    .into_iter()
                    .take(25)
                    .map(|(name, value)| CommandOptionChoice {
                        name: label(&name),
                        name_localizations: None,
                        value: CommandOptionChoiceValue::String(value),
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        self.respond(
            InteractionResponseType::ApplicationCommandAutocompleteResult,
            Some(data),
        )
        .await;
    }
}

/// What Discord says about the person asking. `None` outside a server.
fn actor(ix: &Interaction) -> Option<Actor> {
    let guild = ix.guild_id?;
    let member = ix.member.as_ref();
    let user = ix.author()?;
    let name = member
        .and_then(|m| m.nick.clone())
        .or_else(|| user.global_name.clone())
        .unwrap_or_else(|| user.name.clone());
    Some(Actor {
        id: user.id.get().to_string(),
        name,
        guild_id: guild.get().to_string(),
        channel_id: channel_of(ix)
            .map(|c| c.get().to_string())
            .unwrap_or_default(),
        roles: member
            .map(|m| m.roles.iter().map(|r| r.get().to_string()).collect())
            .unwrap_or_default(),
    })
}

fn channel_of(ix: &Interaction) -> Option<DcId<ChannelMarker>> {
    ix.channel.as_ref().map(|c| c.id)
}

const OUTSIDE_SERVER: &str = "Use this in the watch party server.";

fn room_name(shared: &Shared, id: &str) -> String {
    shared
        .room(id)
        .map(|r| format!("**{}**", escape(&r.name, 100)))
        .unwrap_or_else(|| "the room".into())
}

#[derive(Clone)]
pub struct Bot {
    pub shared: Arc<Shared>,
}

impl Bot {
    fn api(&self) -> &Api {
        &self.shared.api
    }

    async fn act(&self, actor: &Actor, path: &str, body: Value) -> Result<Value, ApiError> {
        self.api().action(path, actor, body).await
    }

    pub async fn handle(&self, interaction: Interaction) {
        let ix = Ix {
            shared: &self.shared,
            ix: &interaction,
        };
        let Some(actor) = actor(&interaction) else {
            if interaction.kind != InteractionType::ApplicationCommandAutocomplete {
                ix.reply_now(OUTSIDE_SERVER.into()).await;
            }
            return;
        };
        match (&interaction.kind, interaction.data.clone()) {
            (InteractionType::ApplicationCommand, Some(InteractionData::ApplicationCommand(c)))
                if c.name == commands::NAME =>
            {
                self.command(&ix, &actor, commands::parse(c.options)).await
            }
            (
                InteractionType::ApplicationCommandAutocomplete,
                Some(InteractionData::ApplicationCommand(c)),
            ) if c.name == commands::NAME => {
                self.autocomplete(&ix, &actor, commands::parse(c.options))
                    .await
            }
            (InteractionType::MessageComponent, Some(InteractionData::MessageComponent(c))) => {
                if let Some(id) = Id::parse(&c.custom_id) {
                    self.component(&ix, &actor, id, c.values.first().cloned())
                        .await;
                }
            }
            (InteractionType::ModalSubmit, Some(InteractionData::ModalSubmit(m))) => {
                if let Some(id) = Id::parse(&m.custom_id) {
                    let values: HashMap<String, String> = m
                        .components
                        .into_iter()
                        .flat_map(|r| r.components)
                        .map(|c| (c.custom_id, c.value.unwrap_or_default()))
                        .collect();
                    self.modal(&ix, &actor, id, values).await;
                }
            }
            _ => {}
        }
    }

    // --- slash commands ------------------------------------------------------

    async fn command(&self, ix: &Ix<'_>, actor: &Actor, inv: Invocation) {
        let settings = self.shared.settings().unwrap_or_default();

        // Commands that open a form must answer with it right away.
        if inv.is(&["link"]) {
            return ix.modal(link_modal()).await;
        }
        if inv.is(&["room", "create"]) {
            return ix.modal(create_modal(settings.require_password)).await;
        }
        if inv.is(&["room", "password"]) {
            let room = inv.string("room").unwrap_or_default();
            if !ids::valid_room_id(room) {
                return ix.reply_now("Pick a room from the list.".into()).await;
            }
            return ix
                .modal(password_modal(room, settings.require_password))
                .await;
        }
        if inv.is(&["room", "join"]) {
            if let Some(room) = inv.string("room").and_then(|id| self.shared.room(id)) {
                if room.has_password && !room.is_participant(&actor.id) {
                    return ix.modal(join_modal(&room)).await;
                }
            }
        }

        ix.defer().await;
        let out = self.run(ix, actor, &inv).await;
        ix.edit(out).await;
    }

    async fn run(&self, ix: &Ix<'_>, actor: &Actor, inv: &Invocation) -> Out {
        let room = inv.string("room").unwrap_or_default().to_string();
        if inv.options.iter().any(|o| o.name == "room") && !ids::valid_room_id(&room) {
            return "Pick a room from the list.".into();
        }
        match inv.path().as_slice() {
            ["unlink"] => match self.act(actor, "unlink", Value::Null).await {
                Ok(_) => {
                    "Unlinked. Rooms you own stay yours; link again with `/jwp link` to use them."
                        .into()
                }
                Err(e) => err(e),
            },
            ["whoami"] => self.whoami(actor).await,
            ["room", "list"] => self.list_rooms(actor),
            ["room", "join"] => self.join(actor, &room, None).await,
            ["room", "leave"] => self.leave(actor, &room).await,
            ["room", "host"] => {
                let member = inv.string("member").unwrap_or_default();
                self.set_host(actor, &room, member).await
            }
            ["room", "rename"] => {
                let name = inv.string("name").unwrap_or_default();
                match self
                    .act(actor, &room_path(&room, "update"), json!({ "name": name }))
                    .await
                {
                    Ok(_) => "Renamed.".into(),
                    Err(e) => err(e),
                }
            }
            ["room", "transfer"] => {
                let Some(user) = inv.user("user") else {
                    return "Pick someone.".into();
                };
                match self
                    .act(
                        actor,
                        &room_path(&room, "owner"),
                        json!({ "to": user.get().to_string() }),
                    )
                    .await
                {
                    Ok(_) => format!(
                        "<@{}> now owns {}.",
                        user.get(),
                        room_name(&self.shared, &room)
                    )
                    .into(),
                    Err(e) => err(e),
                }
            }
            ["room", "kick"] => {
                let Some((is_member, id)) = inv.string("who").and_then(ids::parse_kick_choice)
                else {
                    return "Pick someone from the list.".into();
                };
                let body = if is_member {
                    json!({ "member": id })
                } else {
                    json!({ "user": id })
                };
                match self.act(actor, &room_path(&room, "kick"), body).await {
                    Ok(_) => "Removed.".into(),
                    Err(e) => err(e),
                }
            }
            ["room", "close"] => self.close(actor, &room).await,
            ["room", "panel"] => match channel_of(ix.ix) {
                Some(channel) => self.repost_panel(actor, &room, channel).await,
                None => OUTSIDE_SERVER.into(),
            },
            ["device", "add"] => {
                let session = inv.string("device").unwrap_or_default();
                let host = inv.string("role") == Some("host");
                self.add_device(actor, &room, session, host).await
            }
            ["device", "remove"] => {
                let member = inv.string("device").unwrap_or_default();
                self.remove_device(actor, &room, member).await
            }
            _ => "Unknown command.".into(),
        }
    }

    // --- autocomplete --------------------------------------------------------

    async fn autocomplete(&self, ix: &Ix<'_>, actor: &Actor, inv: Invocation) {
        let Some((field, typed)) = inv.focused() else {
            return;
        };
        let typed = typed.to_lowercase();
        let matches = |s: &str| typed.is_empty() || s.to_lowercase().contains(&typed);
        let room = inv.string("room").and_then(|id| self.shared.room(id));

        let mut choices: Vec<(String, String)> = Vec::new();
        match field {
            "room" => {
                let mut rooms = self.shared.rooms();
                // Rooms you're in first.
                rooms.sort_by_key(|r| !r.is_participant(&actor.id));
                for r in rooms.iter().filter(|r| matches(&r.name)) {
                    let mut l = format!("{} (by {})", r.name, r.owner.name);
                    if r.has_password {
                        l.push_str(", password");
                    }
                    choices.push((l, r.id.clone()));
                }
            }
            "member" => {
                if let Some(r) = &room {
                    for m in r.members.iter().filter(|m| !m.is_host && matches(&m.name)) {
                        choices.push((m.name.clone(), m.id.clone()));
                    }
                }
            }
            "who" => {
                if let Some(r) = &room {
                    for m in r.members.iter().filter(|m| matches(&m.name)) {
                        choices.push((format!("{} (watching)", m.name), format!("m:{}", m.id)));
                    }
                    for p in r
                        .participants
                        .iter()
                        .filter(|p| matches(&p.name) && p.user_id != r.owner.user_id)
                    {
                        if let Some(ext) = &p.external_id {
                            choices.push((
                                format!("{} (and their devices)", p.name),
                                format!("u:{}", ext),
                            ));
                        }
                    }
                }
            }
            "device" if inv.is(&["device", "add"]) => {
                match self.shared.devices(actor, false).await {
                    Ok(devices) => {
                        for d in devices
                            .iter()
                            .filter(|d| d.bridged_as.is_none() && matches(&d.label()))
                        {
                            let mut l = d.label();
                            if !d.remote_control {
                                l.push_str(" - host only");
                            }
                            choices.push((l, d.session_id.clone()));
                        }
                    }
                    Err(e) => log::debug!("device autocomplete: {}", e),
                }
            }
            "device" => {
                if let Some(r) = &room {
                    for m in r.members.iter().filter(|m| {
                        m.owner_external_id.as_deref() == Some(actor.id.as_str())
                            && matches(&m.name)
                    }) {
                        choices.push((m.name.clone(), m.id.clone()));
                    }
                }
            }
            _ => {}
        }
        ix.choices(choices).await;
    }

    // --- buttons and menus ---------------------------------------------------

    async fn component(&self, ix: &Ix<'_>, actor: &Actor, id: Id, picked: Option<String>) {
        match id {
            Id::Join(room) => {
                if let Some(r) = self.shared.room(&room) {
                    if r.has_password && !r.is_participant(&actor.id) {
                        return ix.modal(join_modal(&r)).await;
                    }
                }
                ix.defer().await;
                ix.edit(self.join(actor, &room, None).await).await;
            }
            Id::AddDevice(room) => {
                ix.defer().await;
                ix.edit(self.device_menu(actor, &room).await).await;
            }
            Id::RemoveDevice(room) => ix.reply_now(self.remove_menu(actor, &room)).await,
            Id::PickHost(room) => {
                ix.defer().await;
                ix.edit(self.host_menu(actor, &room).await).await;
            }
            Id::Leave(room) => {
                ix.defer().await;
                ix.edit(self.leave(actor, &room).await).await;
            }
            Id::Close(room) => {
                let confirm = Component::ActionRow(ActionRow {
                    components: vec![panel::button(
                        &Id::ConfirmClose(room.clone()),
                        "Close the room",
                        Style::Danger,
                    )],
                });
                ix.reply_now(Out {
                    text: format!(
                        "Close {}? Everyone in it stops watching together, and devices stop being controlled.",
                        room_name(&self.shared, &room)
                    ),
                    components: vec![confirm],
                })
                .await;
            }
            // Follow-ups on private messages: replace that message with the result.
            Id::ConfirmClose(room) => {
                ix.defer_update().await;
                ix.edit(self.close(actor, &room).await).await;
            }
            Id::DevicePick(room) => {
                ix.defer_update().await;
                let out = match picked.as_deref().and_then(ids::parse_device_choice) {
                    Some((session, host)) => self.add_device(actor, &room, &session, host).await,
                    None => "Pick a device.".into(),
                };
                ix.edit(out).await;
            }
            Id::HostPick(room) => {
                ix.defer_update().await;
                let out = self
                    .set_host(actor, &room, picked.as_deref().unwrap_or_default())
                    .await;
                ix.edit(out).await;
            }
            Id::RemovePick(room) => {
                ix.defer_update().await;
                let out = self
                    .remove_device(actor, &room, picked.as_deref().unwrap_or_default())
                    .await;
                ix.edit(out).await;
            }
            Id::LinkModal | Id::CreateModal | Id::JoinModal(_) | Id::PasswordModal(_) => {}
        }
    }

    // --- modals --------------------------------------------------------------

    async fn modal(&self, ix: &Ix<'_>, actor: &Actor, id: Id, v: HashMap<String, String>) {
        let field = |k: &str| v.get(k).map(|s| s.trim().to_string()).unwrap_or_default();
        ix.defer().await;
        let out = match id {
            Id::LinkModal => match self
                .act(
                    actor,
                    "link",
                    json!({ "username": field("username"), "code": field("code") }),
                )
                .await
            {
                Ok(v) => format!(
                    "Linked to the Jellyfin account **{}**. Create a room with `/jwp room create`, or join one from its panel.",
                    escape(v["user_name"].as_str().unwrap_or("?"), 60)
                )
                .into(),
                Err(e) => err(e),
            },
            Id::CreateModal => match channel_of(ix.ix) {
                Some(channel) => {
                    self.create(actor, &field("name"), &field("password"), channel)
                        .await
                }
                None => OUTSIDE_SERVER.into(),
            },
            Id::JoinModal(room) => self.join(actor, &room, Some(&field("password"))).await,
            Id::PasswordModal(room) => {
                let pw = field("password");
                let body = if pw.is_empty() {
                    json!({ "password": null })
                } else {
                    json!({ "password": pw })
                };
                match self.act(actor, &room_path(&room, "update"), body).await {
                    Ok(_) if pw.is_empty() => "Password removed: anyone may join now.".into(),
                    Ok(_) => "Password changed. People already in the room stay; share the new one privately.".into(),
                    Err(e) => err(e),
                }
            }
            _ => "Unknown form.".into(),
        };
        ix.edit(out).await;
    }

    // --- flows ---------------------------------------------------------------

    async fn whoami(&self, actor: &Actor) -> Out {
        match self.api().me(actor).await {
            Ok(me) => {
                let rooms = self.shared.rooms();
                // An admin may own many rooms: keep the reply within limits.
                let names = |ids: &[String]| {
                    let list: Vec<String> = ids
                        .iter()
                        .filter_map(|id| rooms.iter().find(|r| &r.id == id))
                        .map(|r| format!("**{}**", escape(&r.name, 60)))
                        .collect();
                    if list.is_empty() {
                        "none".into()
                    } else {
                        join_within(&list, ", ", list.len(), 800)
                    }
                };
                format!(
                    "Linked to the Jellyfin account **{}**{}.\nYour rooms: {}\nJoined: {}",
                    escape(&me.user_name, 60),
                    if me.is_admin { " (admin)" } else { "" },
                    names(&me.owns),
                    names(&me.joined)
                )
                .into()
            }
            Err(e) => err(e),
        }
    }

    fn list_rooms(&self, actor: &Actor) -> Out {
        rooms_text(&self.shared.rooms(), &actor.id).into()
    }

    async fn create(
        &self,
        actor: &Actor,
        name: &str,
        password: &str,
        channel: DcId<ChannelMarker>,
    ) -> Out {
        let body = if password.is_empty() {
            json!({ "name": name })
        } else {
            json!({ "name": name, "password": password })
        };
        let created = match self.act(actor, "rooms", body).await {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let id = created["id"].as_str().unwrap_or_default().to_string();
        let shown = escape(created["name"].as_str().unwrap_or(name), 100);
        let mut text = format!("Created **{}**. ", shown);
        match self.post_panel(&id, channel).await {
            Ok(()) => text.push_str(
                "Its panel is in this channel: friends join there, then add their devices.",
            ),
            Err(e) => text.push_str(&format!(
                "I couldn't post its panel here ({}). I need *Send Messages* and *Embed Links* in this channel; then use `/jwp room panel`.",
                e
            )),
        }
        if !password.is_empty() {
            text.push_str(" Share the password privately.");
        }
        text.into()
    }

    async fn join(&self, actor: &Actor, room: &str, password: Option<&str>) -> Out {
        let body = match password {
            Some(p) => json!({ "password": p }),
            None => Value::Null,
        };
        match self.act(actor, &room_path(room, "join"), body).await {
            Ok(v) if v["already"] == true => {
                format!("You're already in {}.", room_name(&self.shared, room)).into()
            }
            Ok(_) => format!(
                "You joined {}. Now add your device with **Add my device** on its panel, or `/jwp device add`. Jellyfin web users join from their Watch Party panel.",
                room_name(&self.shared, room)
            )
            .into(),
            Err(e) if e.reason == "wrong_password" && password.is_none() => {
                "This room has a password: use the **Join** button on its panel to enter it."
                    .into()
            }
            Err(e) => err(e),
        }
    }

    async fn leave(&self, actor: &Actor, room: &str) -> Out {
        let name = room_name(&self.shared, room);
        match self
            .act(actor, &room_path(room, "leave"), Value::Null)
            .await
        {
            Ok(_) => format!("You left {}. Your devices left with you.", name).into(),
            Err(e) => err(e),
        }
    }

    async fn close(&self, actor: &Actor, room: &str) -> Out {
        let name = room_name(&self.shared, room);
        match self
            .act(actor, &room_path(room, "close"), Value::Null)
            .await
        {
            Ok(_) => format!("Closed {}.", name).into(),
            Err(e) => err(e),
        }
    }

    async fn set_host(&self, actor: &Actor, room: &str, member: &str) -> Out {
        if member.is_empty() || member.len() > 64 {
            return "Pick a member.".into();
        }
        match self
            .act(actor, &room_path(room, "host"), json!({ "member": member }))
            .await
        {
            Ok(_) => {
                let who = self
                    .shared
                    .room(room)
                    .and_then(|r| r.members.into_iter().find(|m| m.id == member))
                    .map(|m| escape(&m.name, 60))
                    .unwrap_or_else(|| "They".into());
                format!("{} is the host now: everyone follows them.", who).into()
            }
            Err(e) => err(e),
        }
    }

    async fn add_device(&self, actor: &Actor, room: &str, session: &str, host: bool) -> Out {
        if session.is_empty() || session.len() > 64 {
            return "Pick one of your devices from the list.".into();
        }
        let body = json!({ "session_id": session, "role": if host { "host" } else { "receiver" } });
        match self.act(actor, &room_path(room, "devices"), body).await {
            Ok(_) => {
                self.shared.forget_devices(actor);
                if host {
                    format!(
                        "Your device is the host of {}: start playing on it and everyone follows.",
                        room_name(&self.shared, room)
                    )
                    .into()
                } else {
                    format!(
                        "Your device follows the host of {} now.",
                        room_name(&self.shared, room)
                    )
                    .into()
                }
            }
            Err(e) => err(e),
        }
    }

    async fn remove_device(&self, actor: &Actor, room: &str, member: &str) -> Out {
        if member.is_empty() || member.len() > 64 {
            return "Pick a device.".into();
        }
        match self
            .act(
                actor,
                &room_path(room, "devices/remove"),
                json!({ "member": member }),
            )
            .await
        {
            Ok(_) => {
                self.shared.forget_devices(actor);
                "Removed: the device isn't controlled by the room anymore.".into()
            }
            Err(e) => err(e),
        }
    }

    /// A private menu of the caller's devices, each as receiver and/or host.
    async fn device_menu(&self, actor: &Actor, room: &str) -> Out {
        let settings = self.shared.settings().unwrap_or_default();
        let devices = match self.shared.devices(actor, true).await {
            Ok(d) => d,
            Err(e) => return err(e),
        };
        let hostless = self.shared.room(room).is_some_and(|r| r.host.is_none());
        let mut options = Vec::new();
        for d in devices.iter().filter(|d| d.bridged_as.is_none()) {
            if settings.allow_receiver && d.remote_control {
                options.push(option(
                    &format!("{} - follow the host", d.label()),
                    ids::encode_device_choice(&d.session_id, false),
                    Some(
                        d.now_playing
                            .clone()
                            .unwrap_or_else(|| "Receiver: kept in sync by remote control".into()),
                    ),
                ));
            }
            if settings.allow_host {
                options.push(option(
                    &format!("{} - be the host", d.label()),
                    ids::encode_device_choice(&d.session_id, true),
                    Some(if hostless {
                        "Everyone follows what this device plays".into()
                    } else {
                        "Only the owner can replace the current host".into()
                    }),
                ));
            }
        }
        options.truncate(25);
        if options.is_empty() {
            let rooms = self.shared.rooms();
            let busy: Vec<String> = devices
                .iter()
                .filter(|d| d.bridged_as.is_some())
                .map(|d| {
                    let place = d
                        .room_id
                        .as_ref()
                        .and_then(|id| rooms.iter().find(|r| &r.id == id))
                        .map(|r| format!(" is in **{}**", escape(&r.name, 60)))
                        .unwrap_or_else(|| " is in another watch party".into());
                    format!("{}{}", escape(&d.label(), 60), place)
                })
                .collect();
            return format!(
                "No device to add. Open the Jellyfin app on your TV or phone, signed in as you, and try again.{}",
                if busy.is_empty() {
                    String::new()
                } else {
                    format!("\n({}.)", busy.join("; "))
                }
            )
            .into();
        }
        Out {
            text: format!(
                "Which device should join {}?",
                room_name(&self.shared, room)
            ),
            components: vec![select(
                Id::DevicePick(room.to_string()),
                "Pick a device",
                options,
            )],
        }
    }

    fn remove_menu(&self, actor: &Actor, room: &str) -> Out {
        let Some(r) = self.shared.room(room) else {
            return "That room doesn't exist anymore.".into();
        };
        let options: Vec<_> = r
            .members
            .iter()
            .filter(|m| m.owner_external_id.as_deref() == Some(actor.id.as_str()))
            .take(25)
            .map(|m| option(&m.name, m.id.clone(), None))
            .collect();
        if options.is_empty() {
            return "None of your devices are in this room.".into();
        }
        Out {
            text: "Which device should leave?".into(),
            components: vec![select(
                Id::RemovePick(room.to_string()),
                "Pick a device",
                options,
            )],
        }
    }

    /// Only for the owner or an admin (the server checks again).
    async fn may_manage(&self, actor: &Actor, room: &Room) -> Result<(), Out> {
        if room.is_owner(&actor.id) {
            return Ok(());
        }
        match self.api().me(actor).await {
            Ok(me) if me.is_admin => Ok(()),
            Ok(_) => Err("Only the room's owner (or an admin) can do that.".into()),
            Err(e) => Err(err(e)),
        }
    }

    async fn host_menu(&self, actor: &Actor, room: &str) -> Out {
        let Some(r) = self.shared.room(room) else {
            return "That room doesn't exist anymore.".into();
        };
        if let Err(out) = self.may_manage(actor, &r).await {
            return out;
        }
        let options: Vec<_> = r
            .members
            .iter()
            .filter(|m| !m.is_host)
            .take(25)
            .map(|m| option(&m.name, m.id.clone(), None))
            .collect();
        if options.is_empty() {
            return "Nobody else is watching yet.".into();
        }
        Out {
            text: "Who should everyone follow?".into(),
            components: vec![select(
                Id::HostPick(room.to_string()),
                "Pick the new host",
                options,
            )],
        }
    }

    async fn repost_panel(&self, actor: &Actor, room: &str, channel: DcId<ChannelMarker>) -> Out {
        let Some(r) = self.shared.room(room) else {
            return "That room doesn't exist anymore.".into();
        };
        if let Err(out) = self.may_manage(actor, &r).await {
            return out;
        }
        match self.post_panel(room, channel).await {
            Ok(()) => "Posted the panel here.".into(),
            Err(e) => format!(
                "I couldn't post here ({}). I need *Send Messages* and *Embed Links*.",
                e
            )
            .into(),
        }
    }

    /// Posts a room's panel in `channel` and tells the server where it is.
    async fn post_panel(&self, room_id: &str, channel: DcId<ChannelMarker>) -> Result<(), String> {
        // The new room may not have reached the cache yet.
        let room = match self.shared.room(room_id) {
            Some(r) => r,
            None => self
                .api()
                .rooms(None)
                .await
                .map_err(|e| e.message)?
                .rooms
                .into_iter()
                .find(|r| r.id == room_id)
                .ok_or("the room is gone")?,
        };
        let view = panel::render(&room);
        let embeds = [panel::embed(&view)];
        let components = panel::components(&view);
        let mentions = no_mentions();
        let msg = self
            .shared
            .http
            .create_message(channel)
            .embeds(&embeds)
            .components(&components)
            .allowed_mentions(Some(&mentions))
            .await
            .map_err(|e| e.to_string())?
            .model()
            .await
            .map_err(|e| e.to_string())?;
        self.shared
            .panels()
            .posted((channel.get(), msg.id.get()), &room.name, view);
        self.api()
            .set_panel(
                room_id,
                &channel.get().to_string(),
                &msg.id.get().to_string(),
            )
            .await
            .map_err(|e| e.message)?;
        Ok(())
    }
}

/// `/jwp room list`: one line per room, within one message.
fn rooms_text(rooms: &[Room], actor_id: &str) -> String {
    if rooms.is_empty() {
        return "No rooms right now. Start one with `/jwp room create`.".into();
    }
    let lines: Vec<String> = rooms
        .iter()
        .map(|r| {
            format!(
                "- **{}** by {}: {} watching{}{}",
                escape(&r.name, 60),
                escape(&r.owner.name, 40),
                r.members.len(),
                if r.has_password { ", password" } else { "" },
                if r.is_participant(actor_id) {
                    ", you're in"
                } else {
                    ""
                }
            )
        })
        .collect();
    join_within(&lines, "\n", lines.len(), MESSAGE_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(i: usize, name: &str) -> Room {
        Room {
            id: format!("7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e{:04}", i),
            name: name.into(),
            has_password: true,
            owner: crate::api::Person {
                user_id: "u".into(),
                name: name.into(),
                external_id: None,
            },
            participants: vec![],
            host: None,
            members: vec![],
            media_id: None,
            play_state: String::new(),
            panel: None,
            created_at: 0,
            empty_since: None,
        }
    }

    #[test]
    fn the_room_list_fits_one_message() {
        let name = "-_*".repeat(40);
        let rooms: Vec<Room> = (0..100).map(|i| room(i, &name)).collect();
        let text = rooms_text(&rooms, "1");
        assert!(text.chars().count() <= MESSAGE_MAX, "{}", text.len());
        assert!(text.ends_with("more"), "{}", text);
        twilight_validate::message::content(&text).unwrap();
        assert!(rooms_text(&rooms[..2], "1").starts_with("- **"));
        assert!(rooms_text(&[], "1").starts_with("No rooms"));
    }

    #[test]
    fn modals_fit_discords_limits() {
        let room = room(1, "A very long room name that goes on and on and on");
        for m in [
            link_modal(),
            create_modal(true),
            create_modal(false),
            password_modal(&room.id, false),
            join_modal(&room),
        ] {
            assert!(m.id.encode().len() <= 100);
            assert!(m.title.chars().count() <= 45, "{}", m.title);
            assert!(m.inputs.len() <= 5);
            for row in &m.inputs {
                let Component::ActionRow(r) = row else {
                    panic!("rows only")
                };
                for c in &r.components {
                    let Component::TextInput(t) = c else {
                        panic!("inputs only")
                    };
                    assert!(t.label.chars().count() <= 45, "{}", t.label);
                }
            }
        }
    }

    #[test]
    fn select_options_fit_discords_limits() {
        let o = option(&"x".repeat(150), "v".into(), Some("d".repeat(150)));
        assert!(o.label.chars().count() <= 100);
        assert!(o.description.unwrap().chars().count() <= 100);
    }
}
