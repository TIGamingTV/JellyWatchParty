//! Minimal Jellyfin REST client: list sessions, send playstate commands,
//! start playback. Authenticates with an API key created in the Jellyfin
//! dashboard (Dashboard > API Keys).

use serde::Deserialize;
use std::time::Duration;

/// The device id this server presents to Jellyfin. Its own session (which
/// Jellyfin creates for remote-control calls) is hidden from device lists.
pub const OWN_DEVICE_ID: &str = "jellywatchparty-session-server";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Sessions that showed activity this recently are listed.
const ACTIVE_WITHIN_SECS: u32 = 960;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct JfItem {
    pub id: Option<String>,
    pub name: Option<String>,
    pub series_name: Option<String>,
    pub run_time_ticks: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct JfPlayState {
    pub position_ticks: Option<i64>,
    pub is_paused: bool,
}

/// The parts of a Jellyfin `SessionInfo` the bridge uses.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct JfSession {
    pub id: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub client: Option<String>,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub supports_remote_control: bool,
    pub now_playing_item: Option<JfItem>,
    pub play_state: Option<JfPlayState>,
    pub last_playback_check_in: Option<String>,
}

/// Jellyfin writes GUIDs without dashes in most places but not all; compare
/// them in one canonical form (lowercase hex, no dashes).
pub fn normalize_id(id: &str) -> String {
    id.chars()
        .filter(|c| *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

impl JfSession {
    pub fn device_id(&self) -> &str {
        self.device_id.as_deref().unwrap_or("")
    }

    pub fn user_id(&self) -> String {
        normalize_id(self.user_id.as_deref().unwrap_or(""))
    }

    pub fn item_id(&self) -> Option<String> {
        self.now_playing_item
            .as_ref()
            .and_then(|i| i.id.as_deref())
            .map(normalize_id)
            .filter(|id| !id.is_empty())
    }

    pub fn item_name(&self) -> Option<String> {
        let item = self.now_playing_item.as_ref()?;
        let name = item.name.clone()?;
        Some(match &item.series_name {
            Some(series) if !series.is_empty() => format!("{} - {}", series, name),
            _ => name,
        })
    }

    pub fn client_name(&self) -> &str {
        self.client.as_deref().unwrap_or("")
    }

    pub fn device_name(&self) -> &str {
        self.device_name.as_deref().unwrap_or("")
    }

    pub fn user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or("")
    }

    /// Clients that run the injected Watch Party script themselves; they
    /// join rooms as normal web clients and must not be bridged.
    pub fn runs_web_client(&self) -> bool {
        let c = self.client_name().to_ascii_lowercase();
        ["jellyfin web", "jellyfin desktop", "jellyfin media player"]
            .iter()
            .any(|p| c.starts_with(p))
    }

    pub fn is_own(&self) -> bool {
        self.device_id() == OWN_DEVICE_ID
    }
}

#[derive(Clone)]
pub struct JellyfinApi {
    http: reqwest::Client,
    base: String,
    auth: String,
}

fn quote(v: &str) -> String {
    v.replace('"', "")
}

impl JellyfinApi {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!(
                "JellyWatchParty-SessionServer/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;
        // Jellyfin creates a session from these fields for remote-control
        // calls; Client/Device/DeviceId/Version must all be present.
        let auth = format!(
            "MediaBrowser Client=\"JellyWatchParty Session Server\", Device=\"Session Server\", \
             DeviceId=\"{}\", Version=\"{}\", Token=\"{}\"",
            OWN_DEVICE_ID,
            env!("CARGO_PKG_VERSION"),
            quote(api_key)
        );
        Ok(Self {
            http,
            base: base_url.trim_end_matches('/').to_string(),
            auth,
        })
    }

    async fn check(res: reqwest::Response) -> Result<reqwest::Response, String> {
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        let hint = match status.as_u16() {
            401 => " (check JELLYFIN_API_KEY)",
            403 => " (the API key may lack permission)",
            404 => " (session gone, or check JELLYFIN_URL)",
            _ => "",
        };
        Err(format!("Jellyfin answered {}{}", status, hint))
    }

    fn err(e: reqwest::Error) -> String {
        if e.is_timeout() {
            "Jellyfin did not answer in time".to_string()
        } else if e.is_connect() {
            "Cannot connect to Jellyfin (check JELLYFIN_URL)".to_string()
        } else {
            format!("Jellyfin request failed: {}", e.without_url())
        }
    }

    pub async fn sessions(&self) -> Result<Vec<JfSession>, String> {
        let res = self
            .http
            .get(format!("{}/Sessions", self.base))
            .query(&[("activeWithinSeconds", ACTIVE_WITHIN_SECS)])
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .send()
            .await
            .map_err(Self::err)?;
        Self::check(res)
            .await?
            .json::<Vec<JfSession>>()
            .await
            .map_err(|e| format!("Unexpected /Sessions response: {}", e))
    }

    /// `Pause`, `Unpause` or `Seek` (with `seek_ticks`).
    pub async fn playstate(
        &self,
        session_id: &str,
        command: &str,
        seek_ticks: Option<i64>,
    ) -> Result<(), String> {
        let mut req = self
            .http
            .post(format!(
                "{}/Sessions/{}/Playing/{}",
                self.base,
                urlencode(session_id),
                command
            ))
            .header(reqwest::header::AUTHORIZATION, &self.auth);
        if let Some(t) = seek_ticks {
            req = req.query(&[("seekPositionTicks", t)]);
        }
        Self::check(req.send().await.map_err(Self::err)?)
            .await
            .map(|_| ())
    }

    /// Tells the session to play `item_id` from `start_ticks`.
    pub async fn play_now(
        &self,
        session_id: &str,
        item_id: &str,
        start_ticks: i64,
    ) -> Result<(), String> {
        let req = self
            .http
            .post(format!(
                "{}/Sessions/{}/Playing",
                self.base,
                urlencode(session_id)
            ))
            .query(&[
                ("playCommand", "PlayNow".to_string()),
                ("itemIds", item_id.to_string()),
                ("startPositionTicks", start_ticks.to_string()),
            ])
            .header(reqwest::header::AUTHORIZATION, &self.auth);
        Self::check(req.send().await.map_err(Self::err)?)
            .await
            .map(|_| ())
    }
}

/// Percent-encodes a path segment (session ids are hex, but be safe).
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_jellyfin_session() {
        let json = r#"[{
            "Id": "abc123",
            "UserId": "6a1b2c3d4e5f60718293a4b5c6d7e8f9",
            "UserName": "Alice",
            "Client": "Android TV",
            "DeviceName": "Living room",
            "DeviceId": "dev-1",
            "SupportsRemoteControl": true,
            "NowPlayingItem": { "Id": "0123456789ABCDEF0123456789ABCDEF", "Name": "Pilot", "SeriesName": "Show", "RunTimeTicks": 36000000000 },
            "PlayState": { "PositionTicks": 600000000, "IsPaused": false, "CanSeek": true },
            "LastPlaybackCheckIn": "2026-10-03T19:52:36.1234567Z",
            "Unknown": 1
        }, { "Id": "bare", "UserId": null, "Client": "Jellyfin Web 12.0.0" }]"#;
        let sessions: Vec<JfSession> = serde_json::from_str(json).unwrap();
        let s = &sessions[0];
        assert_eq!(
            s.item_id().as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(s.item_name().as_deref(), Some("Show - Pilot"));
        assert_eq!(
            s.play_state.as_ref().unwrap().position_ticks,
            Some(600_000_000)
        );
        assert!(s.supports_remote_control);
        assert!(!s.runs_web_client());
        assert_eq!(sessions[1].user_id(), "");
        assert!(sessions[1].runs_web_client());
        assert_eq!(sessions[1].item_id(), None);
    }

    #[test]
    fn ids_are_compared_without_dashes_or_case() {
        assert_eq!(
            normalize_id("6A1B2C3D-4E5F-6071-8293-A4B5C6D7E8F9"),
            "6a1b2c3d4e5f60718293a4b5c6d7e8f9"
        );
    }

    #[test]
    fn auth_header_carries_device_fields() {
        let api = JellyfinApi::new("http://jf:8096/", "k\"ey").unwrap();
        assert_eq!(api.base, "http://jf:8096");
        assert!(api
            .auth
            .contains("DeviceId=\"jellywatchparty-session-server\""));
        assert!(api.auth.contains("Token=\"key\""));
    }

    #[test]
    fn urlencode_path_segments() {
        assert_eq!(urlencode("abc-123"), "abc-123");
        assert_eq!(urlencode("a/b c"), "a%2Fb%20c");
    }
}
