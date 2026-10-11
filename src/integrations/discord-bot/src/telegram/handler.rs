//! Telegram messages and button presses.
//!
//! - In the configured group: `/newroom`, `/rooms`, `/link`, `/groupid`,
//!   `/help`, and the room panels' buttons. Results of Join/Leave show as a
//!   short note; menus (devices, managing a room) go to the private chat.
//! - In a private chat: everything, including the steps that ask for a
//!   link code or a room password. The bot deletes those messages right
//!   after reading them.
//!
//! Messages are HTML: every name from a user goes through `escape`.

use super::client::TgError;
use super::ids::{Data, Pick, Start};
use super::panel::{self, keyboard, Button};
use super::text::{escape, join_within, len16, MESSAGE_MAX};
use super::types::{CallbackQuery, Message, User, GROUP_ANONYMOUS_BOT};
use super::{Conv, Tg};
use crate::api::{room_path, valid_room_id, Actor, ApiError, Room};
use serde_json::{json, Value};

/// What the bot sends as the role of group administrators; the server's
/// `admin_role_id` for Telegram is either empty or this.
const GROUP_ADMIN_ROLE: &str = "admin";
const MAX_ROOM_NAME: usize = 100;
const MAX_USERNAME: usize = 128;
const MAX_MENU_ROWS: usize = 20;

const NOT_IN_GROUP: &str =
    "Join the watch party group first (with this Telegram account), then try again.";
const ANONYMOUS: &str = "I can't tell who you are when you post anonymously or as a channel. Post as yourself, or message me privately.";
const OUTDATED: &str = "This button no longer works.";
const GONE: &str = "That room doesn't exist anymore.";

/// Where a request came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The configured group.
    Group {
        chat: i64,
    },
    Private,
}

/// What a flow answers: HTML text and buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub text: String,
    pub rows: Vec<Vec<Button>>,
}

impl From<String> for Out {
    fn from(text: String) -> Self {
        Self {
            text,
            rows: Vec::new(),
        }
    }
}

impl From<&str> for Out {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

impl Out {
    fn with(mut self, rows: Vec<Vec<Button>>) -> Self {
        self.rows = rows;
        self
    }
}

fn err(e: ApiError) -> Out {
    escape(&e.message, 500).into()
}

fn cancel_row() -> Vec<Button> {
    vec![Button::data("Cancel", Data::Cancel)]
}

/// `/cmd@bot args`: the command (lowercase) and its arguments, when it is
/// meant for this bot.
pub fn parse_command<'a>(text: &'a str, bot: &str) -> Option<(String, &'a str)> {
    let rest = text.strip_prefix('/')?;
    let (head, args) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    let (cmd, target) = match head.split_once('@') {
        Some((c, t)) => (c, Some(t)),
        None => (head, None),
    };
    if target.is_some_and(|t| !t.eq_ignore_ascii_case(bot)) {
        return None;
    }
    if cmd.is_empty()
        || cmd.len() > 32
        || !cmd.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    Some((cmd.to_ascii_lowercase(), args))
}

/// The plain text of an HTML message (for button notes, which aren't HTML).
pub fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Who is posting in a group: `Err(true)` for anonymous admins and
/// channels (worth telling), `Err(false)` for bots (ignored).
fn group_sender(m: &Message) -> Result<&User, bool> {
    if m.sender_chat.is_some() {
        return Err(true);
    }
    match &m.from {
        Some(u) if u.id == GROUP_ANONYMOUS_BOT => Err(true),
        Some(u) if u.is_bot => Err(false),
        Some(u) => Ok(u),
        None => Err(true),
    }
}

fn group_help() -> String {
    "<b>Watch parties with your own Jellyfin devices.</b>\n\n\
     /newroom &lt;name&gt; - create a room; its panel is posted here\n\
     /rooms - list the rooms\n\
     /link - link your Jellyfin account (in a private chat with me)\n\
     /groupid - this group's ID, for the admin panel\n\n\
     Each room has a panel here: <b>Join</b>, then <b>Add my device</b>."
        .into()
}

fn private_help() -> String {
    "<b>Watch parties with your own Jellyfin devices.</b>\n\n\
     /link - link your Jellyfin account (with the code from your admin)\n\
     /rooms - the rooms, with buttons to join and add your devices\n\
     /newroom - create a room\n\
     /whoami - your linked account and rooms\n\
     /unlink - unlink your Telegram account\n\
     /cancel - stop what you're doing\n\n\
     You need to be in the watch party group to use me."
        .into()
}

/// The rooms as text, within one message.
fn rooms_text(rooms: &[Room], actor_id: Option<&str>) -> String {
    if rooms.is_empty() {
        return "No rooms right now. Start one with /newroom.".into();
    }
    let lines: Vec<String> = rooms
        .iter()
        .map(|r| {
            format!(
                "• <b>{}</b> by {}: {} watching{}{}",
                escape(&r.name, 60),
                escape(&r.owner.name, 40),
                r.members.len(),
                if r.has_password { ", password" } else { "" },
                if actor_id.is_some_and(|a| r.is_participant(a)) {
                    ", you're in"
                } else {
                    ""
                }
            )
        })
        .collect();
    join_within(&lines, "\n", lines.len(), 3500)
}

impl Tg {
    fn room_name(&self, id: &str) -> String {
        self.room(id)
            .map(|r| format!("<b>{}</b>", escape(&r.name, MAX_ROOM_NAME)))
            .unwrap_or_else(|| "the room".into())
    }

    async fn act(&self, actor: &Actor, path: &str, body: Value) -> Result<Value, ApiError> {
        self.api.action(path, actor, body).await
    }

    /// What Telegram says about the person asking. In a private chat they
    /// must be in the configured group; the server checks the rest.
    async fn actor(&self, user: &User, place: Place) -> Result<Actor, String> {
        let wants_roles = self
            .settings()
            .is_some_and(|s| s.admin_role_id == GROUP_ADMIN_ROLE);
        let roles = |admin: bool| {
            if admin && wants_roles {
                vec![GROUP_ADMIN_ROLE.to_string()]
            } else {
                Vec::new()
            }
        };
        let mut actor = Actor {
            id: user.id.to_string(),
            name: user.display_name(),
            ..Default::default()
        };
        match place {
            Place::Group { chat } => {
                let admin =
                    wants_roles && self.member(chat, user.id).await.is_ok_and(|m| m.is_admin());
                actor.guild_id = chat.to_string();
                actor.channel_id = chat.to_string();
                actor.roles = roles(admin);
            }
            Place::Private => {
                actor.channel_id = user.id.to_string();
                // Without a group the server refuses anyway, with its reason.
                let Some(group) = self.group_id() else {
                    return Ok(actor);
                };
                match self.member(group, user.id).await {
                    Ok(m) if m.in_chat() => {
                        actor.guild_id = group.to_string();
                        actor.roles = roles(m.is_admin());
                    }
                    Ok(_) => return Err(NOT_IN_GROUP.into()),
                    Err(TgError::Gone(why)) => {
                        log::warn!("Telegram: can't see group {}: {}", group, why);
                        return Err(
                            "I can't see the watch party group right now; ask an admin.".into()
                        );
                    }
                    Err(e) => {
                        log::warn!("Telegram: membership check: {}", e);
                        return Err("Telegram didn't answer; try again in a moment.".into());
                    }
                }
            }
        }
        Ok(actor)
    }

    // --- sending -------------------------------------------------------------

    async fn send(&self, chat: i64, topic: Option<i64>, out: &Out) -> Result<Message, TgError> {
        let text = if len16(&out.text) > MESSAGE_MAX {
            "That's too long to show here.".to_string()
        } else {
            out.text.clone()
        };
        let markup = (!out.rows.is_empty()).then(|| keyboard(&out.rows));
        self.client.send_message(chat, topic, &text, markup).await
    }

    async fn reply(&self, chat: i64, topic: Option<i64>, out: impl Into<Out>) {
        if let Err(e) = self.send(chat, topic, &out.into()).await {
            log::warn!("Telegram: could not reply in {}: {}", chat, e);
        }
    }

    /// Replaces a private menu with the result of using it.
    async fn edit(&self, msg: &Message, out: Out) {
        let res = self
            .client
            .edit_message(msg.chat.id, msg.message_id, &out.text, keyboard(&out.rows))
            .await;
        if let Err(e) = res {
            log::warn!("Telegram: could not edit a message: {}", e);
            // The menu may be too old to edit: say it anew.
            self.reply(msg.chat.id, None, out).await;
        }
    }

    async fn answer(&self, cb: &CallbackQuery, text: Option<&str>, alert: bool) {
        if let Err(e) = self.client.answer_callback(&cb.id, text, alert, None).await {
            log::debug!("Telegram: could not answer a button: {}", e);
        }
    }

    async fn answer_url(&self, cb: &CallbackQuery, start: &Start) {
        let url = start.link(&self.username);
        if let Err(e) = self
            .client
            .answer_callback(&cb.id, None, false, Some(&url))
            .await
        {
            log::debug!("Telegram: could not answer a button: {}", e);
        }
    }

    /// A button's result as a note: an alert box for failures.
    async fn answer_result(&self, cb: &CallbackQuery, res: Result<Out, Out>) {
        match res {
            Ok(o) => self.answer(cb, Some(&plain(&o.text)), false).await,
            Err(o) => self.answer(cb, Some(&plain(&o.text)), true).await,
        }
    }

    /// Shows `out` in the presser's private chat. If the bot may not write
    /// there first, the button opens the chat with `start` instead.
    async fn to_private(
        &self,
        cb: &CallbackQuery,
        from_group: bool,
        out: Out,
        start: Start,
    ) -> bool {
        match self.send(cb.from.id, None, &out).await {
            Ok(_) => {
                let note = from_group.then_some("Sent you a private message.");
                self.answer(cb, note, false).await;
                true
            }
            Err(TgError::CantMessage) => {
                self.answer_url(cb, &start).await;
                false
            }
            Err(e) => {
                log::warn!("Telegram: could not message {}: {}", cb.from.id, e);
                self.answer(
                    cb,
                    Some("Telegram didn't take my message; try again."),
                    true,
                )
                .await;
                false
            }
        }
    }

    /// Removes a message holding a secret (a code or password).
    async fn forget_secret(&self, m: &Message) {
        if let Err(e) = self.client.delete_message(m.chat.id, m.message_id).await {
            log::warn!("Telegram: could not delete a message with a secret: {}", e);
        }
    }

    // --- messages ------------------------------------------------------------

    pub(super) async fn on_message(&self, m: Message) {
        if let Some(to) = m.migrate_to_chat_id {
            log::warn!(
                "Telegram: group {} became a supergroup with ID {}: put the new ID in the admin panel",
                m.chat.id,
                to
            );
            return;
        }
        let Some(text) = m.text.clone() else {
            return;
        };
        if m.chat.is_private() {
            self.on_private(&m, &text).await;
        } else if m.chat.is_group() {
            self.on_group(&m, &text).await;
        }
    }

    async fn on_group(&self, m: &Message, text: &str) {
        let Some((cmd, args)) = parse_command(text, &self.username) else {
            return;
        };
        let (chat, topic) = (m.chat.id, m.topic());
        match cmd.as_str() {
            "groupid" => {
                return self
                    .reply(
                        chat,
                        topic,
                        format!(
                            "This group's ID is <code>{}</code>. An admin puts it in the JellyWatchParty admin panel under <b>Telegram bot</b> &gt; <b>Group ID</b>.",
                            chat
                        ),
                    )
                    .await
            }
            "help" | "start" => return self.reply(chat, topic, group_help()).await,
            "newroom" | "rooms" | "link" | "unlink" | "whoami" => {}
            _ => return,
        }
        let from = match group_sender(m) {
            Ok(u) => u.clone(),
            Err(true) => return self.reply(chat, topic, ANONYMOUS).await,
            Err(false) => return,
        };
        if Some(chat) != self.group_id() {
            return self
                .reply(
                    chat,
                    topic,
                    "This group isn't set up for watch parties. An admin can set it up with the ID from /groupid.",
                )
                .await;
        }
        let private_chat = |label: &str, start: Start| {
            vec![vec![Button::Url(label.into(), start.link(&self.username))]]
        };
        match cmd.as_str() {
            "link" => {
                let text = if args.is_empty() {
                    "Linking happens in a private chat with me:"
                } else {
                    // A code typed into the group: don't leave it there.
                    self.forget_secret(m).await;
                    "Never send your code in the group: I removed your message if I could. Link in a private chat with me:"
                };
                let out = Out::from(text).with(private_chat("Link my account", Start::Link));
                self.reply(chat, topic, out).await
            }
            "unlink" | "whoami" => {
                let out =
                    Out::from(format!("Send /{} in a private chat with me.", cmd)).with(vec![
                        vec![Button::Url(
                            "Open a private chat".into(),
                            format!("https://t.me/{}", self.username),
                        )],
                    ]);
                self.reply(chat, topic, out).await
            }
            "rooms" => {
                self.reply(
                    chat,
                    topic,
                    rooms_text(&self.rooms(), Some(&from.id.to_string())),
                )
                .await
            }
            "newroom" => {
                let settings = self.settings().unwrap_or_default();
                if args.is_empty() || settings.require_password {
                    let why = if settings.require_password {
                        "Rooms need a password here, so let's set yours up in a private chat:"
                    } else {
                        "Name the room after the command (<code>/newroom Movie night</code>), or set it up in a private chat:"
                    };
                    let out = Out::from(why).with(private_chat("Create a room", Start::New(topic)));
                    return self.reply(chat, topic, out).await;
                }
                let actor = match self.actor(&from, Place::Group { chat }).await {
                    Ok(a) => a,
                    Err(e) => return self.reply(chat, topic, escape(&e, 300)).await,
                };
                let (out, posted) = self.create(&actor, args, None, topic).await;
                // A posted panel is answer enough.
                if !posted {
                    self.reply(chat, topic, out).await;
                }
            }
            _ => {}
        }
    }

    async fn on_private(&self, m: &Message, text: &str) {
        let Some(from) = m.from.clone().filter(|u| !u.is_bot) else {
            return;
        };
        let chat = m.chat.id;
        if let Some((cmd, args)) = parse_command(text, &self.username) {
            if cmd == "skip" {
                return match self.take_conv(from.id) {
                    Some(Conv::NewPassword { name, topic }) => {
                        self.new_room_step(&from, &name, None, topic).await
                    }
                    other => {
                        if let Some(c) = other {
                            self.set_conv(from.id, c);
                        }
                        self.reply(chat, None, "Nothing to skip.").await
                    }
                };
            }
            let had_conv = self.take_conv(from.id).is_some();
            return self.private_command(m, &from, &cmd, args, had_conv).await;
        }
        match self.take_conv(from.id) {
            Some(conv) => self.conv_step(m, &from, conv, text).await,
            None => {
                self.reply(
                    chat,
                    None,
                    "Send /rooms to see the rooms, or /help for everything I can do.",
                )
                .await
            }
        }
    }

    async fn private_command(
        &self,
        m: &Message,
        from: &User,
        cmd: &str,
        args: &str,
        had_conv: bool,
    ) {
        let chat = m.chat.id;
        match cmd {
            "start" => {
                if args.is_empty() {
                    return self.reply(chat, None, private_help()).await;
                }
                match Start::parse(args) {
                    Some(s) => self.start(from, s).await,
                    None => self.reply(chat, None, private_help()).await,
                }
            }
            "help" => self.reply(chat, None, private_help()).await,
            "cancel" => {
                let text = if had_conv {
                    "Cancelled."
                } else {
                    "Nothing to cancel."
                };
                self.reply(chat, None, text).await
            }
            "link" => {
                let words: Vec<&str> = args.split_whitespace().collect();
                match words.as_slice() {
                    [] => self.start(from, Start::Link).await,
                    [name] => {
                        self.set_conv(
                            from.id,
                            Conv::LinkCode {
                                username: name.to_string(),
                            },
                        );
                        self.reply(chat, None, code_prompt()).await
                    }
                    [name @ .., code] => {
                        self.forget_secret(m).await;
                        let out = match self.actor(from, Place::Private).await {
                            Ok(a) => self.link(&a, &name.join(" "), code).await,
                            Err(e) => escape(&e, 300).into(),
                        };
                        self.reply(chat, None, out).await
                    }
                }
            }
            "unlink" | "whoami" | "rooms" => {
                let actor = match self.actor(from, Place::Private).await {
                    Ok(a) => a,
                    Err(e) => return self.reply(chat, None, escape(&e, 300)).await,
                };
                let out = match cmd {
                    "unlink" => self.unlink(&actor).await,
                    "whoami" => self.whoami(&actor).await,
                    _ => self.rooms_menu(&actor),
                };
                self.reply(chat, None, out).await
            }
            "newroom" => {
                if args.is_empty() {
                    return self.start(from, Start::New(None)).await;
                }
                self.ask_new_password(from, args, None).await
            }
            "groupid" => {
                self.reply(chat, None, "Send /groupid in the group itself.")
                    .await
            }
            _ => {
                self.reply(
                    chat,
                    None,
                    "I don't know that command. /help lists what I can do.",
                )
                .await
            }
        }
    }

    /// Opens a flow in the private chat (from `/start <payload>` too).
    async fn start(&self, from: &User, s: Start) {
        let chat = from.id;
        match s {
            Start::Link => {
                self.set_conv(chat, Conv::LinkName);
                self.reply(
                    chat,
                    None,
                    Out::from(
                        "Let's link your Jellyfin account. What's your <b>Jellyfin user name</b>?",
                    )
                    .with(vec![cancel_row()]),
                )
                .await
            }
            Start::New(topic) => {
                self.set_conv(chat, Conv::NewName { topic });
                self.reply(
                    chat,
                    None,
                    Out::from("What should the room be called?").with(vec![cancel_row()]),
                )
                .await
            }
            Start::Join(room) => {
                let actor = match self.actor(from, Place::Private).await {
                    Ok(a) => a,
                    Err(e) => return self.reply(chat, None, escape(&e, 300)).await,
                };
                let needs_password = self
                    .fresh_room(&room)
                    .await
                    .is_some_and(|r| r.has_password && !r.is_participant(&actor.id));
                if !needs_password {
                    match self.join(&actor, &room, None).await {
                        Ok(o) => return self.reply(chat, None, o).await,
                        Err(e) if e.reason != "wrong_password" => {
                            return self.reply(chat, None, err(e)).await
                        }
                        // The room got a password since: ask for it.
                        Err(_) => {}
                    }
                }
                self.set_conv(chat, Conv::JoinPassword { room: room.clone() });
                self.reply(chat, None, self.password_prompt(&room)).await
            }
            Start::AddDevice(_) | Start::RemoveDevice(_) | Start::Manage(_) => {
                let actor = match self.actor(from, Place::Private).await {
                    Ok(a) => a,
                    Err(e) => return self.reply(chat, None, escape(&e, 300)).await,
                };
                let out = match &s {
                    Start::AddDevice(room) => self.device_menu(from.id, &actor, room).await,
                    Start::RemoveDevice(room) => self.remove_menu(from.id, &actor, room),
                    Start::Manage(room) => self.manage_menu(&actor, room).await,
                    _ => return,
                };
                self.reply(chat, None, out).await
            }
        }
    }

    fn password_prompt(&self, room: &str) -> Out {
        Out::from(format!(
            "Send the password for {}. I'll delete your message right after reading it.",
            self.room_name(room)
        ))
        .with(vec![cancel_row()])
    }

    async fn ask_new_password(&self, from: &User, name: &str, topic: Option<i64>) {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_ROOM_NAME {
            self.set_conv(from.id, Conv::NewName { topic });
            return self
                .reply(
                    from.id,
                    None,
                    format!(
                        "Give the room a name of at most {} characters.",
                        MAX_ROOM_NAME
                    ),
                )
                .await;
        }
        let required = self.settings().unwrap_or_default().require_password;
        self.set_conv(
            from.id,
            Conv::NewPassword {
                name: name.to_string(),
                topic,
            },
        );
        let text = if required {
            "Now send a password for the room (rooms need one here). I'll delete your message right after reading it."
        } else {
            "Now send a password for the room, or /skip for an open room. I'll delete your message right after reading it."
        };
        self.reply(from.id, None, Out::from(text).with(vec![cancel_row()]))
            .await
    }

    async fn conv_step(&self, m: &Message, from: &User, conv: Conv, text: &str) {
        let chat = m.chat.id;
        let text = text.trim();
        match conv {
            Conv::LinkName => {
                if text.is_empty() || text.chars().count() > MAX_USERNAME {
                    self.set_conv(from.id, Conv::LinkName);
                    return self
                        .reply(chat, None, "That isn't a user name. Try again, or /cancel.")
                        .await;
                }
                self.set_conv(
                    from.id,
                    Conv::LinkCode {
                        username: text.to_string(),
                    },
                );
                self.reply(chat, None, code_prompt()).await
            }
            Conv::LinkCode { username } => {
                self.forget_secret(m).await;
                let out = match self.actor(from, Place::Private).await {
                    Ok(a) => self.link(&a, &username, text).await,
                    Err(e) => escape(&e, 300).into(),
                };
                self.reply(chat, None, out).await
            }
            Conv::NewName { topic } => self.ask_new_password(from, text, topic).await,
            Conv::NewPassword { name, topic } => {
                self.forget_secret(m).await;
                self.new_room_step(from, &name, Some(text), topic).await
            }
            Conv::JoinPassword { room } => {
                self.forget_secret(m).await;
                let out = match self.actor(from, Place::Private).await {
                    Ok(a) => match self.join(&a, &room, Some(text)).await {
                        Ok(o) => o,
                        Err(e) => err(e),
                    },
                    Err(e) => escape(&e, 300).into(),
                };
                self.reply(chat, None, out).await
            }
            Conv::Rename { room } => {
                let out = match self.actor(from, Place::Private).await {
                    Ok(a) => match self
                        .act(&a, &room_path(&room, "update"), json!({ "name": text }))
                        .await
                    {
                        Ok(_) => "Renamed.".into(),
                        Err(e) => err(e),
                    },
                    Err(e) => escape(&e, 300).into(),
                };
                self.reply(chat, None, out).await
            }
            Conv::SetPassword { room } => {
                self.forget_secret(m).await;
                let remove = text == "-";
                let body = if remove {
                    json!({ "password": null })
                } else {
                    json!({ "password": text })
                };
                let out: Out = match self.actor(from, Place::Private).await {
                    Ok(a) => match self.act(&a, &room_path(&room, "update"), body).await {
                        Ok(_) if remove => "Password removed: anyone may join now.".into(),
                        Ok(_) => "Password changed. People already in the room stay; share the new one privately.".into(),
                        Err(e) => err(e),
                    },
                    Err(e) => escape(&e, 300).into(),
                };
                self.reply(chat, None, out).await
            }
        }
    }

    async fn new_room_step(
        &self,
        from: &User,
        name: &str,
        password: Option<&str>,
        topic: Option<i64>,
    ) {
        let out = match self.actor(from, Place::Private).await {
            Ok(a) => self.create(&a, name, password, topic).await.0,
            Err(e) => escape(&e, 300).into(),
        };
        self.reply(from.id, None, out).await
    }

    // --- buttons -------------------------------------------------------------

    pub(super) async fn on_callback(&self, cb: CallbackQuery) {
        let Some(data) = cb.data.as_deref().and_then(Data::parse) else {
            return self.answer(&cb, Some(OUTDATED), false).await;
        };
        let Some(msg) = cb.message.clone() else {
            return self.answer(&cb, Some(OUTDATED), false).await;
        };
        if cb.from.is_bot {
            return;
        }
        let from_group = !msg.chat.is_private();
        let place = if !from_group {
            Place::Private
        } else if Some(msg.chat.id) == self.group_id() {
            Place::Group { chat: msg.chat.id }
        } else {
            return self
                .answer(
                    &cb,
                    Some("This group isn't set up for watch parties."),
                    true,
                )
                .await;
        };
        let actor = match self.actor(&cb.from, place).await {
            Ok(a) => a,
            Err(e) => return self.answer(&cb, Some(&e), true).await,
        };
        let uid = cb.from.id;
        match data {
            Data::Join(room) => {
                let needs_password = self
                    .fresh_room(&room)
                    .await
                    .is_some_and(|r| r.has_password && !r.is_participant(&actor.id));
                if !needs_password {
                    match self.join(&actor, &room, None).await {
                        Ok(o) => return self.answer(&cb, Some(&plain(&o.text)), false).await,
                        Err(e) if e.reason != "wrong_password" => {
                            return self.answer(&cb, Some(&e.message), true).await
                        }
                        // The room got a password since: ask for it.
                        Err(_) => {}
                    }
                }
                let prompt = self.password_prompt(&room);
                if self
                    .to_private(&cb, from_group, prompt, Start::Join(room.clone()))
                    .await
                {
                    self.set_conv(uid, Conv::JoinPassword { room });
                }
            }
            Data::Leave(room) => {
                let res = self.leave(&actor, &room).await;
                self.answer_result(&cb, res).await
            }
            Data::AddDevice(room) => {
                let out = self.device_menu(uid, &actor, &room).await;
                self.to_private(&cb, from_group, out, Start::AddDevice(room))
                    .await;
            }
            Data::RemoveDevice(room) => {
                let out = self.remove_menu(uid, &actor, &room);
                self.to_private(&cb, from_group, out, Start::RemoveDevice(room))
                    .await;
            }
            Data::Manage(room) => {
                let out = self.manage_menu(&actor, &room).await;
                self.to_private(&cb, from_group, out, Start::Manage(room.clone()))
                    .await;
            }
            Data::Room(room) => {
                let out: Out = match self.room(&room) {
                    Some(r) => {
                        let v = panel::render(&r);
                        Out::from(v.text).with(v.rows)
                    }
                    None => GONE.into(),
                };
                self.to_private(&cb, from_group, out, Start::Manage(room))
                    .await;
            }
            Data::Rename(_) | Data::Password(_) => {
                let (renaming, room) = match data {
                    Data::Rename(r) => (true, r),
                    Data::Password(r) => (false, r),
                    _ => return,
                };
                if let Err(out) = self.may_manage(&actor, &room).await {
                    return self.answer(&cb, Some(&plain(&out.text)), true).await;
                }
                let prompt = if renaming {
                    format!("Send the new name for {}.", self.room_name(&room))
                } else if self.settings().unwrap_or_default().require_password {
                    format!(
                        "Send the new password for {}. I'll delete your message right after reading it.",
                        self.room_name(&room)
                    )
                } else {
                    format!(
                        "Send the new password for {}, or <code>-</code> to remove it. I'll delete your message right after reading it.",
                        self.room_name(&room)
                    )
                };
                let out = Out::from(prompt).with(vec![cancel_row()]);
                if self
                    .to_private(&cb, from_group, out, Start::Manage(room.clone()))
                    .await
                {
                    self.set_conv(
                        uid,
                        if renaming {
                            Conv::Rename { room }
                        } else {
                            Conv::SetPassword { room }
                        },
                    );
                }
            }
            Data::HostMenu(_) | Data::KickMenu(_) | Data::TransferMenu(_) => {
                let (out, room) = match data {
                    Data::HostMenu(r) => (self.host_menu(uid, &actor, &r).await, r),
                    Data::KickMenu(r) => (self.kick_menu(uid, &actor, &r).await, r),
                    Data::TransferMenu(r) => (self.transfer_menu(uid, &actor, &r).await, r),
                    _ => return,
                };
                self.to_private(&cb, from_group, out, Start::Manage(room))
                    .await;
            }
            Data::Close(room) => {
                let out = Out::from(format!(
                    "Close {}? Everyone in it stops watching together, and devices stop being controlled.",
                    self.room_name(&room)
                ))
                .with(vec![vec![
                    Button::data("Close the room", Data::ConfirmClose(room.clone())),
                    Button::data("Keep it", Data::Cancel),
                ]]);
                self.to_private(&cb, from_group, out, Start::Manage(room))
                    .await;
            }
            Data::ConfirmClose(room) => {
                self.answer(&cb, None, false).await;
                let name = self.room_name(&room);
                let out = match self
                    .act(&actor, &room_path(&room, "close"), Value::Null)
                    .await
                {
                    Ok(_) => format!("Closed {}.", name).into(),
                    Err(e) => err(e),
                };
                self.edit(&msg, out).await
            }
            Data::Repost(room) => {
                self.answer(&cb, None, false).await;
                let out = self.repost(&actor, &room).await;
                self.edit(&msg, out).await
            }
            Data::Pick(token) => {
                let pick = self
                    .picks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&token, uid);
                let Some(pick) = pick else {
                    return self
                        .answer(&cb, Some("That menu expired; open it again."), true)
                        .await;
                };
                self.answer(&cb, None, false).await;
                let out = self.do_pick(&actor, pick).await;
                self.edit(&msg, out).await
            }
            Data::Cancel => {
                self.answer(&cb, None, false).await;
                self.take_conv(uid);
                if !from_group {
                    self.edit(&msg, "Cancelled.".into()).await
                }
            }
        }
    }

    // --- flows ---------------------------------------------------------------

    async fn link(&self, actor: &Actor, username: &str, code: &str) -> Out {
        match self
            .act(actor, "link", json!({ "username": username, "code": code }))
            .await
        {
            Ok(v) => format!(
                "Linked to the Jellyfin account <b>{}</b>. Create a room with /newroom, or join one from its panel in the group.",
                escape(v["user_name"].as_str().unwrap_or("?"), 60)
            )
            .into(),
            Err(e) => {
                let mut out = err(e);
                out.text.push_str("\nSend /link to try again.");
                out
            }
        }
    }

    async fn unlink(&self, actor: &Actor) -> Out {
        match self.act(actor, "unlink", Value::Null).await {
            Ok(_) => {
                "Unlinked. Rooms you own stay yours; link again with /link to use them.".into()
            }
            Err(e) => err(e),
        }
    }

    async fn whoami(&self, actor: &Actor) -> Out {
        match self.api.me(actor).await {
            Ok(me) => {
                let rooms = self.rooms();
                let names = |ids: &[String]| {
                    let list: Vec<String> = ids
                        .iter()
                        .filter_map(|id| rooms.iter().find(|r| &r.id == id))
                        .map(|r| format!("<b>{}</b>", escape(&r.name, 60)))
                        .collect();
                    if list.is_empty() {
                        "none".into()
                    } else {
                        join_within(&list, ", ", list.len(), 1500)
                    }
                };
                format!(
                    "Linked to the Jellyfin account <b>{}</b>{}.\nYour rooms: {}\nJoined: {}",
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

    /// The rooms, with a button each to open them here.
    fn rooms_menu(&self, actor: &Actor) -> Out {
        let rooms = self.rooms();
        let rows = rooms
            .iter()
            .take(MAX_MENU_ROWS)
            .map(|r| vec![Button::data(&r.name, Data::Room(r.id.clone()))])
            .collect();
        Out::from(rooms_text(&rooms, Some(&actor.id))).with(rows)
    }

    /// Creates a room and posts its panel in the group. Also says whether
    /// the panel went up.
    async fn create(
        &self,
        actor: &Actor,
        name: &str,
        password: Option<&str>,
        topic: Option<i64>,
    ) -> (Out, bool) {
        let password = password.map(str::trim).filter(|p| !p.is_empty());
        let body = match password {
            Some(p) => json!({ "name": name, "password": p }),
            None => json!({ "name": name }),
        };
        let created = match self.act(actor, "rooms", body).await {
            Ok(v) => v,
            Err(e) => return (err(e), false),
        };
        let id = created["id"].as_str().unwrap_or_default().to_string();
        let shown = escape(created["name"].as_str().unwrap_or(name), MAX_ROOM_NAME);
        let mut text = format!("Created <b>{}</b>. ", shown);
        let posted = match self.group_id() {
            Some(group) => {
                match self.post_panel(&id, group, topic).await {
                    Ok(()) => {
                        text.push_str("Its panel is in the group: friends join there, then add their devices.");
                        true
                    }
                    Err(e) => {
                        text.push_str(&format!(
                        "I couldn't post its panel in the group ({}). I need to be allowed to send messages there; then use <b>Manage</b> &gt; <b>Post panel again</b>.",
                        escape(&e, 200)
                    ));
                        false
                    }
                }
            }
            None => false,
        };
        if password.is_some() {
            text.push_str(" Share the password privately.");
        }
        (text.into(), posted)
    }

    async fn join(
        &self,
        actor: &Actor,
        room: &str,
        password: Option<&str>,
    ) -> Result<Out, ApiError> {
        let body = match password {
            Some(p) => json!({ "password": p }),
            None => Value::Null,
        };
        let res = self.act(actor, &room_path(room, "join"), body).await;
        // A brand-new room may not be in the cache yet.
        let name = self
            .fresh_room(room)
            .await
            .map(|r| format!("<b>{}</b>", escape(&r.name, MAX_ROOM_NAME)))
            .unwrap_or_else(|| "the room".into());
        match res {
            Ok(v) if v["already"] == true => Ok(format!("You're already in {}.", name).into()),
            Ok(_) => Ok(format!(
                "You joined {}. Now add your device with Add my device.",
                name
            )
            .into()),
            Err(e) => Err(e),
        }
    }

    /// A room, from the cache or (if it's new) from the server.
    async fn fresh_room(&self, id: &str) -> Option<Room> {
        if let Some(r) = self.room(id) {
            return Some(r);
        }
        self.api
            .rooms(None)
            .await
            .ok()?
            .rooms
            .into_iter()
            .find(|r| r.id == id)
    }

    async fn leave(&self, actor: &Actor, room: &str) -> Result<Out, Out> {
        let name = self.room_name(room);
        match self
            .act(actor, &room_path(room, "leave"), Value::Null)
            .await
        {
            Ok(_) => Ok(format!("You left {}. Your devices left with you.", name).into()),
            Err(e) => Err(err(e)),
        }
    }

    /// Only for the owner or an admin (the server checks again).
    async fn may_manage(&self, actor: &Actor, room: &str) -> Result<Room, Out> {
        let Some(r) = self.room(room) else {
            return Err(GONE.into());
        };
        if r.is_owner(&actor.id) {
            return Ok(r);
        }
        match self.api.me(actor).await {
            Ok(me) if me.is_admin => Ok(r),
            Ok(_) => Err("Only the room's owner (or an admin) can do that.".into()),
            Err(e) => Err(err(e)),
        }
    }

    fn offer(&self, user: i64, pick: Pick) -> Data {
        self.picks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .offer(user, pick)
    }

    /// The caller's devices, each as receiver and/or host.
    async fn device_menu(&self, user: i64, actor: &Actor, room: &str) -> Out {
        if !valid_room_id(room) || self.room(room).is_none() {
            return GONE.into();
        }
        let settings = self.settings().unwrap_or_default();
        let devices = match self.api.devices(actor).await {
            Ok(d) => d,
            Err(e) => return err(e),
        };
        let hostless = self.room(room).is_some_and(|r| r.host.is_none());
        let mut rows = Vec::new();
        for d in devices.iter().filter(|d| d.bridged_as.is_none()) {
            if settings.allow_receiver && d.remote_control {
                let data = self.offer(
                    user,
                    Pick::AddDevice {
                        room: room.into(),
                        session: d.session_id.clone(),
                        host: false,
                    },
                );
                rows.push(vec![Button::data(
                    &format!("{}: follow the host", d.label()),
                    data,
                )]);
            }
            if settings.allow_host {
                let data = self.offer(
                    user,
                    Pick::AddDevice {
                        room: room.into(),
                        session: d.session_id.clone(),
                        host: true,
                    },
                );
                rows.push(vec![Button::data(
                    &format!("{}: be the host", d.label()),
                    data,
                )]);
            }
        }
        rows.truncate(MAX_MENU_ROWS);
        if rows.is_empty() {
            let rooms = self.rooms();
            let busy: Vec<String> = devices
                .iter()
                .filter(|d| d.bridged_as.is_some())
                .map(|d| {
                    let place = d
                        .room_id
                        .as_ref()
                        .and_then(|id| rooms.iter().find(|r| &r.id == id))
                        .map(|r| format!(" is in <b>{}</b>", escape(&r.name, 60)))
                        .unwrap_or_else(|| " is in another watch party".into());
                    format!("{}{}", escape(&d.label(), 60), place)
                })
                .collect();
            return format!(
                "No device to add. Open the Jellyfin app on your TV or phone, signed in as you, and try again.{}",
                if busy.is_empty() {
                    String::new()
                } else {
                    format!("\n({}.)", join_within(&busy, "; ", busy.len(), 1500))
                }
            )
            .into();
        }
        rows.push(cancel_row());
        let note = if !settings.allow_host {
            ""
        } else if hostless {
            "\nThe host is who everyone follows."
        } else {
            "\nOnly the owner can replace the current host."
        };
        Out::from(format!(
            "Which device should join {}?{}",
            self.room_name(room),
            note
        ))
        .with(rows)
    }

    fn remove_menu(&self, user: i64, actor: &Actor, room: &str) -> Out {
        let Some(r) = self.room(room) else {
            return GONE.into();
        };
        let mut rows: Vec<Vec<Button>> = r
            .members
            .iter()
            .filter(|m| m.owner_external_id.as_deref() == Some(actor.id.as_str()))
            .take(MAX_MENU_ROWS)
            .map(|m| {
                let data = self.offer(
                    user,
                    Pick::RemoveDevice {
                        room: room.into(),
                        member: m.id.clone(),
                    },
                );
                vec![Button::data(&m.name, data)]
            })
            .collect();
        if rows.is_empty() {
            return "None of your devices are in this room.".into();
        }
        rows.push(cancel_row());
        Out::from(format!(
            "Which device should leave {}?",
            self.room_name(room)
        ))
        .with(rows)
    }

    async fn manage_menu(&self, actor: &Actor, room: &str) -> Out {
        if let Err(out) = self.may_manage(actor, room).await {
            return out;
        }
        let r = || room.to_string();
        Out::from(format!("Manage {}:", self.room_name(room))).with(vec![
            vec![
                Button::data("Rename", Data::Rename(r())),
                Button::data("Password", Data::Password(r())),
            ],
            vec![
                Button::data("Pick the host", Data::HostMenu(r())),
                Button::data("Remove someone", Data::KickMenu(r())),
            ],
            vec![
                Button::data("Hand over", Data::TransferMenu(r())),
                Button::data("Post panel again", Data::Repost(r())),
            ],
            vec![
                Button::data("Close the room", Data::Close(r())),
                Button::data("Done", Data::Cancel),
            ],
        ])
    }

    async fn host_menu(&self, user: i64, actor: &Actor, room: &str) -> Out {
        let r = match self.may_manage(actor, room).await {
            Ok(r) => r,
            Err(out) => return out,
        };
        let mut rows: Vec<Vec<Button>> = r
            .members
            .iter()
            .filter(|m| !m.is_host)
            .take(MAX_MENU_ROWS)
            .map(|m| {
                let data = self.offer(
                    user,
                    Pick::Host {
                        room: room.into(),
                        member: m.id.clone(),
                    },
                );
                vec![Button::data(&m.name, data)]
            })
            .collect();
        if rows.is_empty() {
            return "Nobody else is watching yet.".into();
        }
        rows.push(cancel_row());
        Out::from("Who should everyone follow?").with(rows)
    }

    async fn kick_menu(&self, user: i64, actor: &Actor, room: &str) -> Out {
        let r = match self.may_manage(actor, room).await {
            Ok(r) => r,
            Err(out) => return out,
        };
        let mut rows = Vec::new();
        for m in &r.members {
            let data = self.offer(
                user,
                Pick::Kick {
                    room: room.into(),
                    member: Some(m.id.clone()),
                    user: None,
                },
            );
            rows.push(vec![Button::data(&format!("{} (watching)", m.name), data)]);
        }
        for p in r
            .participants
            .iter()
            .filter(|p| p.user_id != r.owner.user_id)
        {
            if let Some(ext) = &p.external_id {
                let data = self.offer(
                    user,
                    Pick::Kick {
                        room: room.into(),
                        member: None,
                        user: Some(ext.clone()),
                    },
                );
                rows.push(vec![Button::data(
                    &format!("{} (and their devices)", p.name),
                    data,
                )]);
            }
        }
        rows.truncate(MAX_MENU_ROWS);
        if rows.is_empty() {
            return "Nobody to remove.".into();
        }
        rows.push(cancel_row());
        Out::from("Who should leave the room?").with(rows)
    }

    async fn transfer_menu(&self, user: i64, actor: &Actor, room: &str) -> Out {
        let r = match self.may_manage(actor, room).await {
            Ok(r) => r,
            Err(out) => return out,
        };
        let mut rows: Vec<Vec<Button>> = r
            .participants
            .iter()
            .filter(|p| p.user_id != r.owner.user_id)
            .filter_map(|p| {
                let ext = p.external_id.clone()?;
                let data = self.offer(
                    user,
                    Pick::Transfer {
                        room: room.into(),
                        to: ext,
                    },
                );
                Some(vec![Button::data(&p.name, data)])
            })
            .take(MAX_MENU_ROWS)
            .collect();
        if rows.is_empty() {
            return "Nobody to hand the room to: they need to join it here first.".into();
        }
        rows.push(cancel_row());
        Out::from("Who should own the room from now on?").with(rows)
    }

    async fn do_pick(&self, actor: &Actor, pick: Pick) -> Out {
        match pick {
            Pick::AddDevice {
                room,
                session,
                host,
            } => {
                let body = json!({ "session_id": session, "role": if host { "host" } else { "receiver" } });
                match self.act(actor, &room_path(&room, "devices"), body).await {
                    Ok(_) if host => format!(
                        "Your device is the host of {}: start playing on it and everyone follows.",
                        self.room_name(&room)
                    )
                    .into(),
                    Ok(_) => format!(
                        "Your device follows the host of {} now.",
                        self.room_name(&room)
                    )
                    .into(),
                    Err(e) => err(e),
                }
            }
            Pick::RemoveDevice { room, member } => {
                match self
                    .act(
                        actor,
                        &room_path(&room, "devices/remove"),
                        json!({ "member": member }),
                    )
                    .await
                {
                    Ok(_) => "Removed: the device isn't controlled by the room anymore.".into(),
                    Err(e) => err(e),
                }
            }
            Pick::Host { room, member } => {
                let who = self
                    .room(&room)
                    .and_then(|r| r.members.into_iter().find(|m| m.id == member))
                    .map(|m| escape(&m.name, 60))
                    .unwrap_or_else(|| "They".into());
                match self
                    .act(
                        actor,
                        &room_path(&room, "host"),
                        json!({ "member": member }),
                    )
                    .await
                {
                    Ok(_) => format!("{} is the host now: everyone follows them.", who).into(),
                    Err(e) => err(e),
                }
            }
            Pick::Kick { room, member, user } => {
                let body = match (member, user) {
                    (Some(m), _) => json!({ "member": m }),
                    (None, Some(u)) => json!({ "user": u }),
                    (None, None) => return "Pick someone.".into(),
                };
                match self.act(actor, &room_path(&room, "kick"), body).await {
                    Ok(_) => "Removed.".into(),
                    Err(e) => err(e),
                }
            }
            Pick::Transfer { room, to } => {
                let who = self
                    .room(&room)
                    .and_then(|r| {
                        r.participants
                            .into_iter()
                            .find(|p| p.external_id.as_deref() == Some(to.as_str()))
                    })
                    .map(|p| escape(&p.name, 60))
                    .unwrap_or_else(|| "They".into());
                match self
                    .act(actor, &room_path(&room, "owner"), json!({ "to": to }))
                    .await
                {
                    Ok(_) => format!("{} now owns {}.", who, self.room_name(&room)).into(),
                    Err(e) => err(e),
                }
            }
        }
    }

    async fn repost(&self, actor: &Actor, room: &str) -> Out {
        if let Err(out) = self.may_manage(actor, room).await {
            return out;
        }
        let Some(group) = self.group_id() else {
            return "No watch party group is set up.".into();
        };
        match self.post_panel(room, group, None).await {
            Ok(()) => "Posted the panel in the group.".into(),
            Err(e) => format!(
                "I couldn't post in the group ({}). I need to be allowed to send messages there.",
                escape(&e, 200)
            )
            .into(),
        }
    }

    /// Posts a room's panel in `chat` and tells the server where it is.
    async fn post_panel(&self, room_id: &str, chat: i64, topic: Option<i64>) -> Result<(), String> {
        // A new room may not have reached the cache yet.
        let room = match self.room(room_id) {
            Some(r) => r,
            None => self
                .api
                .rooms(None)
                .await
                .map_err(|e| e.message)?
                .rooms
                .into_iter()
                .find(|r| r.id == room_id)
                .ok_or("the room is gone")?,
        };
        let view = panel::render(&room);
        let msg = self
            .client
            .send_message(chat, topic, &view.text, Some(keyboard(&view.rows)))
            .await
            .map_err(|e| e.to_string())?;
        self.panels()
            .posted((chat, msg.message_id), &room.name, view);
        self.api
            .set_panel(room_id, &chat.to_string(), &msg.message_id.to_string())
            .await
            .map_err(|e| e.message)?;
        Ok(())
    }
}

fn code_prompt() -> Out {
    Out::from(
        "Now send the <b>4-digit code</b> from your admin. I'll delete your message right after reading it.",
    )
    .with(vec![cancel_row()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::RoomsResponse;
    use crate::telegram::types::Chat;

    #[test]
    fn commands_for_this_bot_only() {
        assert_eq!(
            parse_command("/rooms", "JwpBot"),
            Some(("rooms".into(), ""))
        );
        assert_eq!(
            parse_command("/NewRoom@jwpbot  Movie night ", "JwpBot"),
            Some(("newroom".into(), "Movie night"))
        );
        assert_eq!(
            parse_command("/link alice 1234", "JwpBot"),
            Some(("link".into(), "alice 1234"))
        );
        assert_eq!(parse_command("/rooms@OtherBot", "JwpBot"), None);
        assert_eq!(parse_command("rooms", "JwpBot"), None);
        assert_eq!(parse_command("/", "JwpBot"), None);
        assert_eq!(parse_command("/a-b", "JwpBot"), None);
        assert_eq!(
            parse_command("/start join-7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10", "JwpBot"),
            Some(("start".into(), "join-7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10"))
        );
    }

    #[test]
    fn notes_are_plain_text() {
        assert_eq!(
            plain("You joined <b>Tom &amp; Jerry &lt;3</b>."),
            "You joined Tom & Jerry <3."
        );
    }

    fn msg(from: Option<User>, sender_chat: Option<Chat>) -> Message {
        Message {
            message_id: 1,
            from,
            sender_chat,
            chat: Chat {
                id: -100,
                kind: "supergroup".into(),
                title: None,
            },
            text: Some("/rooms".into()),
            message_thread_id: None,
            is_topic_message: false,
            migrate_to_chat_id: None,
        }
    }

    fn user(id: i64, is_bot: bool) -> User {
        User {
            id,
            is_bot,
            first_name: "x".into(),
            last_name: None,
            username: None,
        }
    }

    #[test]
    fn only_people_posting_as_themselves_are_served() {
        assert_eq!(
            group_sender(&msg(Some(user(5, false)), None)).unwrap().id,
            5
        );
        assert!(group_sender(&msg(Some(user(GROUP_ANONYMOUS_BOT, true)), None)).unwrap_err());
        let channel = Chat {
            id: -200,
            kind: "channel".into(),
            title: None,
        };
        assert!(group_sender(&msg(Some(user(136817688, true)), Some(channel))).unwrap_err());
        assert!(!group_sender(&msg(Some(user(9, true)), None)).unwrap_err());
    }

    #[test]
    fn the_room_list_fits_one_message() {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../../fixtures/rooms.json")).unwrap();
        let mut room = r.rooms[0].clone();
        room.name = "&<>".repeat(30);
        let rooms: Vec<Room> = (0..100).map(|_| room.clone()).collect();
        let text = rooms_text(&rooms, Some("1"));
        assert!(len16(&text) <= MESSAGE_MAX);
        assert!(text.ends_with("more"));
        assert!(rooms_text(&[], None).starts_with("No rooms"));
        assert!(rooms_text(&r.rooms, Some("100000000000000001")).contains("you're in"));
    }
}
