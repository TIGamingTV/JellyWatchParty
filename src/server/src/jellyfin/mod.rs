//! Device bridge for the admin panel: puts Jellyfin sessions that can't run
//! the Watch Party script (TV apps, Fladder, Swiftfin, ...) into rooms by
//! driving them over the Jellyfin REST API. Replaces the plugin's
//! in-panel Host/Receiver bridges for admins.

pub mod api;
pub mod bridge;
pub mod logic;
pub mod time;

pub use bridge::Bridges;

const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;
const MIN_POLL_INTERVAL_MS: u64 = 250;

#[derive(Debug, Clone)]
pub struct JellyfinConfig {
    pub url: String,
    pub api_key: String,
    pub poll_interval_ms: u64,
}

#[derive(Debug)]
pub enum JellyfinSetup {
    Enabled(JellyfinConfig),
    /// Neither `JELLYFIN_URL` nor an API key is set: the admin panel works,
    /// minus Jellyfin devices.
    NotConfigured,
    Misconfigured(String),
}

impl JellyfinSetup {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let url = get("JELLYFIN_URL")
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty());
        let key = match get("JELLYFIN_API_KEY").filter(|k| !k.trim().is_empty()) {
            Some(k) => Some(k.trim().to_string()),
            None => match get("JELLYFIN_API_KEY_FILE").filter(|p| !p.is_empty()) {
                None => None,
                Some(path) => match std::fs::read_to_string(&path) {
                    Ok(k) => Some(k.trim().to_string()).filter(|k| !k.is_empty()),
                    Err(e) => {
                        return JellyfinSetup::Misconfigured(format!(
                            "cannot read JELLYFIN_API_KEY_FILE ({}): {}",
                            path, e
                        ))
                    }
                },
            },
        };
        match (url, key) {
            (None, None) => JellyfinSetup::NotConfigured,
            (None, Some(_)) => JellyfinSetup::Misconfigured("JELLYFIN_URL is not set".into()),
            (Some(_), None) => JellyfinSetup::Misconfigured(
                "JELLYFIN_API_KEY (or JELLYFIN_API_KEY_FILE) is not set".into(),
            ),
            (Some(url), Some(api_key)) => {
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return JellyfinSetup::Misconfigured(format!(
                        "JELLYFIN_URL must start with http:// or https:// (got '{}')",
                        url
                    ));
                }
                let poll_interval_ms = get("BRIDGE_POLL_INTERVAL_MS")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(DEFAULT_POLL_INTERVAL_MS)
                    .max(MIN_POLL_INTERVAL_MS);
                JellyfinSetup::Enabled(JellyfinConfig {
                    url,
                    api_key,
                    poll_interval_ms,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn setup(vars: &[(&str, &str)]) -> JellyfinSetup {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        JellyfinSetup::from_lookup(move |k| map.get(k).cloned())
    }

    #[test]
    fn config() {
        assert!(matches!(setup(&[]), JellyfinSetup::NotConfigured));
        assert!(matches!(
            setup(&[("JELLYFIN_URL", "http://jf:8096")]),
            JellyfinSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[("JELLYFIN_API_KEY", "k")]),
            JellyfinSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[("JELLYFIN_URL", "jf:8096"), ("JELLYFIN_API_KEY", "k")]),
            JellyfinSetup::Misconfigured(_)
        ));
        let JellyfinSetup::Enabled(c) = setup(&[
            ("JELLYFIN_URL", " http://jf:8096/ "),
            ("JELLYFIN_API_KEY", "k"),
            ("BRIDGE_POLL_INTERVAL_MS", "10"),
        ]) else {
            panic!("expected enabled")
        };
        assert_eq!(c.url, "http://jf:8096");
        assert_eq!(c.poll_interval_ms, MIN_POLL_INTERVAL_MS);
        let JellyfinSetup::Enabled(c) =
            setup(&[("JELLYFIN_URL", "https://jf"), ("JELLYFIN_API_KEY", "k")])
        else {
            panic!("expected enabled")
        };
        assert_eq!(c.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
    }
}
