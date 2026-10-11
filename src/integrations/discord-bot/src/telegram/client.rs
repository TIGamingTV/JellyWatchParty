//! A small client for the Telegram Bot API: only the calls this bot makes.
//!
//! The bot token is part of every request URL, so errors are logged
//! without URLs, and redirects are never followed.

use super::types::{ChatMember, Message, Update, User};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TgError {
    /// The bot token was refused.
    Unauthorized,
    /// Too many requests: wait this many seconds.
    RetryAfter(u64),
    /// An edit that changes nothing (not a failure).
    NotModified,
    /// The message or chat is gone, or the bot is no longer in it.
    Gone(String),
    /// The user never started a private chat with the bot (or blocked it).
    CantMessage,
    /// The group became a supergroup with this id.
    Migrated(i64),
    /// Network or Telegram trouble; may go away by itself.
    Transient(String),
    Other(String),
}

impl std::fmt::Display for TgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TgError::Unauthorized => write!(f, "the bot token was refused"),
            TgError::RetryAfter(s) => write!(f, "rate limited for {} s", s),
            TgError::NotModified => write!(f, "message not modified"),
            TgError::Gone(d) => write!(f, "gone: {}", d),
            TgError::CantMessage => write!(f, "the user hasn't started a chat with the bot"),
            TgError::Migrated(id) => write!(f, "the group moved to {}", id),
            TgError::Transient(d) => write!(f, "temporary failure: {}", d),
            TgError::Other(d) => write!(f, "{}", d),
        }
    }
}

/// Turns a failed answer (`{ok: false, error_code, description,
/// parameters}`) into an error.
pub fn classify(status: u16, body: &Value) -> TgError {
    let desc = body["description"].as_str().unwrap_or("").to_string();
    let lower = desc.to_lowercase();
    let code = body["error_code"]
        .as_u64()
        .map(|c| c as u16)
        .unwrap_or(status);
    if let Some(s) = body["parameters"]["retry_after"].as_u64() {
        return TgError::RetryAfter(s.max(1));
    }
    if let Some(id) = body["parameters"]["migrate_to_chat_id"].as_i64() {
        return TgError::Migrated(id);
    }
    match code {
        // 404 is what a malformed token gets.
        401 | 404 => TgError::Unauthorized,
        429 => TgError::RetryAfter(5),
        400 if lower.contains("message is not modified") => TgError::NotModified,
        400 if [
            "message to edit not found",
            "message to delete not found",
            "message can't be edited",
            "message can't be deleted",
            "chat not found",
            "message_id_invalid",
        ]
        .iter()
        .any(|s| lower.contains(s)) =>
        {
            TgError::Gone(desc)
        }
        403 if [
            "bot can't initiate conversation",
            "bot was blocked by the user",
            "user is deactivated",
        ]
        .iter()
        .any(|s| lower.contains(s)) =>
        {
            TgError::CantMessage
        }
        403 => TgError::Gone(desc),
        c if c >= 500 => TgError::Transient(desc),
        _ => TgError::Other(if desc.is_empty() {
            format!("HTTP {}", code)
        } else {
            desc
        }),
    }
}

pub struct Client {
    http: reqwest::Client,
    /// `<api>/bot<token>`: never logged.
    base: String,
}

impl Client {
    pub fn new(api_url: &str, token: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("JellyWatchParty-Bot/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;
        Ok(Self {
            http,
            base: format!("{}/bot{}", api_url.trim_end_matches('/'), token),
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: &Value,
        timeout: Duration,
    ) -> Result<T, TgError> {
        let res = self
            .http
            .post(format!("{}/{}", self.base, method))
            .json(params)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| TgError::Transient(e.without_url().to_string()))?;
        let status = res.status().as_u16();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| TgError::Transient(e.without_url().to_string()))?;
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if body["ok"] != true {
            return Err(classify(status, &body));
        }
        serde_json::from_value(body["result"].clone())
            .map_err(|e| TgError::Other(format!("unexpected answer to {}: {}", method, e)))
    }

    pub async fn get_me(&self) -> Result<User, TgError> {
        self.call("getMe", &json!({}), TIMEOUT).await
    }

    /// `getUpdates` only works without a webhook.
    pub async fn delete_webhook(&self) -> Result<bool, TgError> {
        self.call("deleteWebhook", &json!({}), TIMEOUT).await
    }

    /// Waits up to `wait` seconds for updates after `offset`.
    pub async fn get_updates(
        &self,
        offset: Option<i64>,
        wait: u64,
    ) -> Result<Vec<Update>, TgError> {
        let mut params = json!({
            "timeout": wait,
            "allowed_updates": ["message", "callback_query", "my_chat_member"],
        });
        if let Some(o) = offset {
            params["offset"] = o.into();
        }
        self.call(
            "getUpdates",
            &params,
            Duration::from_secs(wait) + Duration::from_secs(15),
        )
        .await
    }

    /// Sends an HTML message without link previews.
    pub async fn send_message(
        &self,
        chat_id: i64,
        topic: Option<i64>,
        text: &str,
        markup: Option<Value>,
    ) -> Result<Message, TgError> {
        let mut params = json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
        });
        if let Some(t) = topic {
            params["message_thread_id"] = t.into();
        }
        if let Some(m) = markup {
            params["reply_markup"] = m;
        }
        self.call("sendMessage", &params, TIMEOUT).await
    }

    /// Replaces a message's text and buttons. An edit that changes nothing
    /// counts as done.
    pub async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        markup: Value,
    ) -> Result<(), TgError> {
        let params = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
            "reply_markup": markup,
        });
        match self
            .call::<Value>("editMessageText", &params, TIMEOUT)
            .await
        {
            Ok(_) | Err(TgError::NotModified) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Answers a button press: a short note (an alert box with `alert`), or
    /// a `t.me/<bot>?start=...` link to open.
    pub async fn answer_callback(
        &self,
        id: &str,
        text: Option<&str>,
        alert: bool,
        url: Option<&str>,
    ) -> Result<(), TgError> {
        let mut params = json!({ "callback_query_id": id, "show_alert": alert });
        if let Some(t) = text {
            params["text"] = crate::text::fit(t, 200).into();
        }
        if let Some(u) = url {
            params["url"] = u.into();
        }
        self.call::<Value>("answerCallbackQuery", &params, TIMEOUT)
            .await
            .map(|_| ())
    }

    pub async fn get_chat_member(&self, chat_id: i64, user_id: i64) -> Result<ChatMember, TgError> {
        self.call(
            "getChatMember",
            &json!({ "chat_id": chat_id, "user_id": user_id }),
            TIMEOUT,
        )
        .await
    }

    pub async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), TgError> {
        self.call::<Value>(
            "deleteMessage",
            &json!({ "chat_id": chat_id, "message_id": message_id }),
            TIMEOUT,
        )
        .await
        .map(|_| ())
    }

    /// The command menu for a scope (`{"type": "all_private_chats"}`, ...).
    pub async fn set_commands(
        &self,
        commands: &[(&str, &str)],
        scope: Value,
    ) -> Result<(), TgError> {
        let list: Vec<Value> = commands
            .iter()
            .map(|(c, d)| json!({ "command": c, "description": d }))
            .collect();
        self.call::<Value>(
            "setMyCommands",
            &json!({ "commands": list, "scope": scope }),
            TIMEOUT,
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_commands(&self, scope: Value) -> Result<(), TgError> {
        self.call::<Value>("deleteMyCommands", &json!({ "scope": scope }), TIMEOUT)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_classified() {
        let e = |status: u16, body: Value| classify(status, &body);
        assert_eq!(
            e(
                401,
                json!({"ok":false,"error_code":401,"description":"Unauthorized"})
            ),
            TgError::Unauthorized
        );
        assert_eq!(
            e(
                429,
                json!({"ok":false,"error_code":429,"description":"Too Many Requests: retry after 7","parameters":{"retry_after":7}})
            ),
            TgError::RetryAfter(7)
        );
        assert_eq!(
            e(
                400,
                json!({"ok":false,"error_code":400,"description":"Bad Request: message is not modified: specified new message content and reply markup are exactly the same"})
            ),
            TgError::NotModified
        );
        assert!(matches!(
            e(
                400,
                json!({"ok":false,"error_code":400,"description":"Bad Request: message to edit not found"})
            ),
            TgError::Gone(_)
        ));
        assert_eq!(
            e(
                403,
                json!({"ok":false,"error_code":403,"description":"Forbidden: bot can't initiate conversation with a user"})
            ),
            TgError::CantMessage
        );
        assert!(matches!(
            e(
                403,
                json!({"ok":false,"error_code":403,"description":"Forbidden: bot was kicked from the supergroup chat"})
            ),
            TgError::Gone(_)
        ));
        assert_eq!(
            e(
                400,
                json!({"ok":false,"error_code":400,"description":"Bad Request: group chat was upgraded to a supergroup chat","parameters":{"migrate_to_chat_id":-1001234}})
            ),
            TgError::Migrated(-1001234)
        );
        assert!(matches!(e(502, Value::Null), TgError::Transient(_)));
        assert!(matches!(
            e(
                409,
                json!({"ok":false,"error_code":409,"description":"Conflict: terminated by other getUpdates request"})
            ),
            TgError::Other(_)
        ));
    }

    /// Serves canned answers, one per connection, and records requests.
    async fn mock(
        answers: Vec<(u16, &'static str)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let mut seen = Vec::new();
            for (status, body) in answers {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let mut req = Vec::new();
                loop {
                    let n = sock.read(&mut buf).await.unwrap();
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if req.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8_lossy(&req).to_string());
                let resp = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
            seen
        });
        (url, handle)
    }

    #[tokio::test]
    async fn calls_carry_the_token_in_the_path_and_parse_results() {
        let (url, server) = mock(vec![
            (
                200,
                r#"{"ok":true,"result":[{"update_id":5,"message":{"message_id":1,"chat":{"id":2,"type":"private"},"text":"hi"}}]}"#,
            ),
            (
                400,
                r#"{"ok":false,"error_code":400,"description":"Bad Request: message is not modified"}"#,
            ),
            (401, r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#),
        ])
        .await;
        let c = Client::new(&url, "123:abc").unwrap();
        let ups = c.get_updates(Some(4), 0).await.unwrap();
        assert_eq!(ups[0].update_id, 5);
        assert!(c
            .edit_message(2, 1, "x", json!({"inline_keyboard": []}))
            .await
            .is_ok());
        assert_eq!(c.get_me().await.unwrap_err(), TgError::Unauthorized);

        let reqs = server.await.unwrap();
        assert!(reqs[0].starts_with("POST /bot123:abc/getUpdates HTTP/1.1"));
        let body: Value = serde_json::from_str(reqs[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["offset"], 4);
        assert_eq!(body["allowed_updates"][1], "callback_query");
        let body: Value = serde_json::from_str(reqs[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["parse_mode"], "HTML");
    }

    #[tokio::test]
    async fn network_errors_never_show_the_token() {
        let c = Client::new("http://127.0.0.1:9", "123:secret").unwrap();
        let e = c.get_me().await.unwrap_err();
        assert!(matches!(e, TgError::Transient(_)));
        assert!(!e.to_string().contains("secret"), "{}", e);
    }
}
