//! Client for the session server's integration API, and the shapes it
//! answers with (see docs/technical/integration-api.md). The server makes
//! every decision; this side only says who is asking.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
/// The rooms long poll waits up to 25 s on the server.
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(40);

/// Platform settings, as edited in the admin UI. Defaults match the
/// server's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub guild_id: String,
    pub channel_ids: Vec<String>,
    pub required_role_id: String,
    pub admin_role_id: String,
    pub max_rooms_per_user: u32,
    pub max_rooms_total: u32,
    pub require_password: bool,
    pub allow_host: bool,
    pub allow_receiver: bool,
    pub empty_room_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            guild_id: String::new(),
            channel_ids: Vec::new(),
            required_role_id: String::new(),
            admin_role_id: String::new(),
            max_rooms_per_user: 2,
            max_rooms_total: 20,
            require_password: false,
            allow_host: true,
            allow_receiver: true,
            empty_room_minutes: 30,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigResponse {
    pub version: u64,
    #[serde(default)]
    pub settings: Option<Settings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Person {
    pub user_id: String,
    pub name: String,
    #[serde(default)]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Host {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// `jellyfin` (a bridged device), `web` or `plugin_bridge`.
    pub kind: String,
    pub is_host: bool,
    pub status: String,
    #[serde(default)]
    pub owner_user_id: Option<String>,
    /// The chat account that owns this device, if it is a bridged one.
    #[serde(default)]
    pub owner_external_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PanelRef {
    pub channel_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub has_password: bool,
    pub owner: Person,
    pub participants: Vec<Person>,
    #[serde(default)]
    pub host: Option<Host>,
    pub members: Vec<Member>,
    #[serde(default)]
    pub media_id: Option<String>,
    #[serde(default)]
    pub play_state: String,
    #[serde(default)]
    pub panel: Option<PanelRef>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub empty_since: Option<u64>,
}

impl Room {
    pub fn is_participant(&self, external_id: &str) -> bool {
        self.participants
            .iter()
            .any(|p| p.external_id.as_deref() == Some(external_id))
    }

    pub fn is_owner(&self, external_id: &str) -> bool {
        self.owner.external_id.as_deref() == Some(external_id)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomsResponse {
    pub version: u64,
    pub rooms: Vec<Room>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    pub session_id: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub remote_control: bool,
    #[serde(default)]
    pub now_playing: Option<String>,
    #[serde(default)]
    pub bridged_as: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
}

impl Device {
    pub fn label(&self) -> String {
        match (self.device_name.is_empty(), self.client.is_empty()) {
            (false, false) => format!("{} ({})", self.device_name, self.client),
            (false, true) => self.device_name.clone(),
            (true, false) => self.client.clone(),
            (true, true) => "Jellyfin device".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct DevicesResponse {
    devices: Vec<Device>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Me {
    pub user_name: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub owns: Vec<String>,
    #[serde(default)]
    pub joined: Vec<String>,
}

/// Who is asking: what the chat platform told us about them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub guild_id: String,
    pub channel_id: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP status, or 0 if the server couldn't be reached.
    pub status: u16,
    pub reason: String,
    pub message: String,
}

impl ApiError {
    fn unreachable() -> Self {
        Self {
            status: 0,
            reason: "unreachable".into(),
            message: "The watch party server can't be reached right now. Try again in a minute"
                .into(),
        }
    }

    /// Turns an error body (`{error, reason}`) into an error.
    pub fn from_body(status: u16, body: &Value) -> Self {
        let message = body["error"]
            .as_str()
            .filter(|m| !m.is_empty())
            .unwrap_or("Something went wrong on the watch party server")
            .to_string();
        Self {
            status,
            reason: body["reason"].as_str().unwrap_or("error").to_string(),
            message,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}, {})", self.message, self.reason, self.status)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

pub struct Api {
    http: reqwest::Client,
    base: String,
    auth: String,
}

impl Api {
    pub fn new(base: &str, token: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "JellyWatchParty-DiscordBot/",
                env!("CARGO_PKG_VERSION")
            ))
            // The integration API never redirects; don't carry the token
            // (or room passwords and link codes) anywhere else.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;
        Ok(Self {
            http,
            base: format!("{}/v1", base.trim_end_matches('/')),
            auth: format!("Bearer {}", token),
        })
    }

    async fn send<T: for<'de> Deserialize<'de>>(
        &self,
        req: reqwest::RequestBuilder,
        timeout: Duration,
    ) -> ApiResult<T> {
        let res = req
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                log::warn!("session server request failed: {}", e.without_url());
                ApiError::unreachable()
            })?;
        let status = res.status().as_u16();
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let err = ApiError::from_body(status, &body);
            if status == 401 {
                log::error!("session server refused our token: check JWP_INTEGRATION_TOKEN");
            }
            return Err(err);
        }
        serde_json::from_value(body).map_err(|e| {
            log::error!("unexpected answer from the session server: {}", e);
            ApiError {
                status,
                reason: "bad_response".into(),
                message: "The watch party server sent an answer this bot doesn't understand (versions out of step?)".into(),
            }
        })
    }

    pub async fn config(&self) -> ApiResult<ConfigResponse> {
        self.send(self.http.get(format!("{}/config", self.base)), TIMEOUT)
            .await
    }

    pub async fn heartbeat(&self, bot_name: &str) -> ApiResult<Value> {
        self.send(
            self.http
                .post(format!("{}/heartbeat", self.base))
                .json(&serde_json::json!({ "bot_name": bot_name })),
            TIMEOUT,
        )
        .await
    }

    /// The rooms; with `since`, waits until they changed after that version.
    pub async fn rooms(&self, since: Option<u64>) -> ApiResult<RoomsResponse> {
        let mut req = self.http.get(format!("{}/rooms", self.base));
        if let Some(v) = since {
            req = req.query(&[("since", v)]);
        }
        self.send(req, LONG_POLL_TIMEOUT).await
    }

    pub async fn set_panel(
        &self,
        room_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> ApiResult<Value> {
        self.send(
            self.http
                .put(format!("{}/rooms/{}/panel", self.base, path(room_id)))
                .json(&serde_json::json!({ "channel_id": channel_id, "message_id": message_id })),
            TIMEOUT,
        )
        .await
    }

    /// A user action: POST `path` with `{actor, ...extra}`.
    pub async fn action(&self, path: &str, actor: &Actor, extra: Value) -> ApiResult<Value> {
        let mut body = match extra {
            Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        body.insert(
            "actor".into(),
            serde_json::to_value(actor).unwrap_or(Value::Null),
        );
        self.send(
            self.http
                .post(format!("{}/{}", self.base, path))
                .json(&Value::Object(body)),
            TIMEOUT,
        )
        .await
    }

    pub async fn me(&self, actor: &Actor) -> ApiResult<Me> {
        let v = self.action("me", actor, Value::Null).await?;
        serde_json::from_value(v).map_err(|_| ApiError::from_body(200, &Value::Null))
    }

    pub async fn devices(&self, actor: &Actor) -> ApiResult<Vec<Device>> {
        let v = self.action("devices", actor, Value::Null).await?;
        serde_json::from_value::<DevicesResponse>(v)
            .map(|d| d.devices)
            .map_err(|_| ApiError::from_body(200, &Value::Null))
    }
}

/// Server room ids are UUIDs.
pub fn valid_room_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// A room id as a path segment (server ids are UUIDs; anything else is
/// refused before it reaches a URL).
pub fn path(room_id: &str) -> &str {
    if valid_room_id(room_id) {
        room_id
    } else {
        "invalid"
    }
}

/// `rooms/<id>/<action>`.
pub fn room_path(room_id: &str, action: &str) -> String {
    format!("rooms/{}/{}", path(room_id), action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shared_fixture() {
        let r: RoomsResponse =
            serde_json::from_str(include_str!("../../fixtures/rooms.json")).unwrap();
        assert_eq!(r.version, 42);
        let room = &r.rooms[0];
        assert!(room.is_owner("100000000000000001"));
        assert!(room.is_participant("100000000000000001"));
        assert!(!room.is_participant("2"));
        assert_eq!(
            room.members[0].owner_external_id.as_deref(),
            Some("100000000000000001")
        );
        assert_eq!(room.host.as_ref().unwrap().id, "c1");
        assert_eq!(
            room.panel.as_ref().unwrap().message_id,
            "300000000000000003"
        );
    }

    #[test]
    fn errors_carry_the_servers_message() {
        let e = ApiError::from_body(
            403,
            &serde_json::json!({ "error": "Join the room first", "reason": "not_participant" }),
        );
        assert_eq!(e.reason, "not_participant");
        assert_eq!(e.message, "Join the room first");
        let e = ApiError::from_body(500, &Value::Null);
        assert_eq!(e.reason, "error");
        assert!(!e.message.is_empty());
    }

    #[test]
    fn room_paths_only_take_room_ids() {
        assert_eq!(
            room_path("7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10", "join"),
            "rooms/7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10/join"
        );
        assert_eq!(room_path("../admin", "join"), "rooms/invalid/join");
    }

    /// Serves one canned answer per connection and records the requests.
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
    async fn actions_send_the_token_and_the_actor() {
        let (url, server) = mock(vec![
            (200, r#"{"ok":true,"id":"x","name":"N"}"#),
            (
                403,
                r#"{"error":"Join the room first","reason":"not_participant"}"#,
            ),
        ])
        .await;
        let api = Api::new(&url, "secret-token").unwrap();
        let actor = Actor {
            id: "42".into(),
            name: "al".into(),
            guild_id: "1".into(),
            channel_id: "2".into(),
            roles: vec!["9".into()],
        };
        let v = api
            .action("rooms", &actor, serde_json::json!({ "name": "N" }))
            .await
            .unwrap();
        assert_eq!(v["id"], "x");
        let e = api
            .action(
                &room_path("7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10", "devices"),
                &actor,
                Value::Null,
            )
            .await
            .unwrap_err();
        assert_eq!((e.status, e.reason.as_str()), (403, "not_participant"));

        let reqs = server.await.unwrap();
        assert!(reqs[0].starts_with("POST /v1/rooms HTTP/1.1"));
        assert!(reqs[0]
            .to_ascii_lowercase()
            .contains("authorization: bearer secret-token"));
        let body: Value = serde_json::from_str(reqs[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["name"], "N");
        assert_eq!(body["actor"]["id"], "42");
        assert_eq!(body["actor"]["roles"][0], "9");
        assert!(reqs[1].starts_with("POST /v1/rooms/7b0d3c1e-0c39-4a43-9a0e-2f1f3a3e9d10/devices "));
    }

    #[tokio::test]
    async fn an_unreachable_server_is_reported_plainly() {
        let api = Api::new("http://127.0.0.1:9", "t").unwrap();
        let e = api.config().await.unwrap_err();
        assert_eq!(e.reason, "unreachable");
        assert_eq!(e.status, 0);
    }

    #[test]
    fn settings_default_like_the_server() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.allow_host && s.allow_receiver && !s.enabled);
        assert_eq!(s.empty_room_minutes, 30);
    }

    #[test]
    fn device_labels() {
        let mut d = Device {
            session_id: "s".into(),
            device_name: "TV".into(),
            client: "Android TV".into(),
            remote_control: true,
            now_playing: None,
            bridged_as: None,
            room_id: None,
        };
        assert_eq!(d.label(), "TV (Android TV)");
        d.client.clear();
        assert_eq!(d.label(), "TV");
        d.device_name.clear();
        assert_eq!(d.label(), "Jellyfin device");
    }
}
