//! The parts of Telegram Bot API objects this bot reads.

use serde::Deserialize;

/// Telegram's stand-in sender for anonymous group administrators.
pub const GROUP_ANONYMOUS_BOT: i64 = 1087968824;

#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
    #[serde(default)]
    pub my_chat_member: Option<ChatMemberUpdated>,
}

impl Update {
    /// The person behind the update, if any.
    pub fn user_id(&self) -> Option<i64> {
        if let Some(m) = &self.message {
            return m.from.as_ref().map(|u| u.id);
        }
        self.callback_query.as_ref().map(|c| c.from.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

impl User {
    pub fn display_name(&self) -> String {
        let full = match &self.last_name {
            Some(l) if !l.trim().is_empty() => format!("{} {}", self.first_name.trim(), l.trim()),
            _ => self.first_name.trim().to_string(),
        };
        if !full.is_empty() {
            return full;
        }
        self.username
            .clone()
            .unwrap_or_else(|| format!("user {}", self.id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Chat {
    pub id: i64,
    /// `private`, `group`, `supergroup` or `channel`.
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
}

impl Chat {
    pub fn is_group(&self) -> bool {
        matches!(self.kind.as_str(), "group" | "supergroup")
    }

    pub fn is_private(&self) -> bool {
        self.kind == "private"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<User>,
    /// Set when someone posts as a channel or as the (anonymous) group.
    #[serde(default)]
    pub sender_chat: Option<Chat>,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub message_thread_id: Option<i64>,
    #[serde(default)]
    pub is_topic_message: bool,
    /// The group became a supergroup with this id.
    #[serde(default)]
    pub migrate_to_chat_id: Option<i64>,
}

impl Message {
    /// The forum topic the message is in (only meaningful in forums).
    pub fn topic(&self) -> Option<i64> {
        self.message_thread_id.filter(|_| self.is_topic_message)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    /// The message with the button (possibly just its chat and id).
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChatMember {
    /// `creator`, `administrator`, `member`, `restricted`, `left` or `kicked`.
    pub status: String,
    /// For `restricted`: whether they're still in the chat.
    #[serde(default)]
    pub is_member: Option<bool>,
}

impl ChatMember {
    pub fn in_chat(&self) -> bool {
        match self.status.as_str() {
            "creator" | "administrator" | "member" => true,
            "restricted" => self.is_member.unwrap_or(false),
            _ => false,
        }
    }

    pub fn is_admin(&self) -> bool {
        matches!(self.status.as_str(), "creator" | "administrator")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMemberUpdated {
    pub chat: Chat,
    pub new_chat_member: ChatMember,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_updates() {
        let u: Update = serde_json::from_str(
            r#"{"update_id":7,"message":{"message_id":3,"date":1,
                "from":{"id":42,"is_bot":false,"first_name":"Al","last_name":"Ice","language_code":"en"},
                "chat":{"id":-1001,"type":"supergroup","title":"Movies"},
                "text":"/rooms@JwpBot","message_thread_id":9,"is_topic_message":true}}"#,
        )
        .unwrap();
        let m = u.message.as_ref().unwrap();
        assert_eq!(u.user_id(), Some(42));
        assert!(m.chat.is_group());
        assert_eq!(m.topic(), Some(9));
        assert_eq!(m.from.as_ref().unwrap().display_name(), "Al Ice");

        // A button on a message the bot can't read anymore.
        let u: Update = serde_json::from_str(
            r#"{"update_id":8,"callback_query":{"id":"c","chat_instance":"x",
                "from":{"id":5,"is_bot":false,"first_name":""},
                "message":{"message_id":4,"date":0,"chat":{"id":5,"type":"private"}},
                "data":"v1:cancel"}}"#,
        )
        .unwrap();
        let c = u.callback_query.unwrap();
        assert!(c.message.unwrap().chat.is_private());
        assert_eq!(c.from.display_name(), "user 5");
    }

    #[test]
    fn membership() {
        let m = |s: &str, is_member: Option<bool>| ChatMember {
            status: s.into(),
            is_member,
        };
        assert!(m("creator", None).is_admin());
        assert!(m("member", None).in_chat() && !m("member", None).is_admin());
        assert!(m("restricted", Some(true)).in_chat());
        assert!(!m("restricted", Some(false)).in_chat());
        assert!(!m("left", None).in_chat() && !m("kicked", None).in_chat());
    }
}
