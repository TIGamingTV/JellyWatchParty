//! Settings from the environment. Only how to reach Discord and the session
//! server; everything else (server, channels, roles, limits) is set in the
//! admin UI and fetched from the session server.

use std::fmt;

/// Must match the session server's minimum for integration tokens.
pub const MIN_TOKEN_LEN: usize = 32;
pub const DEFAULT_API_URL: &str = "http://session-server:3002";

pub struct Config {
    pub discord_token: String,
    pub api_url: String,
    pub api_token: String,
}

// Never print the tokens.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("api_url", &self.api_url)
            .finish_non_exhaustive()
    }
}

/// Reads `NAME`, or the file named by `NAME_FILE` (Docker secrets).
fn secret(get: &impl Fn(&str) -> Option<String>, name: &str) -> Result<Option<String>, String> {
    if let Some(v) = get(name)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Ok(Some(v));
    }
    let file_var = format!("{}_FILE", name);
    match get(&file_var).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(path) => std::fs::read_to_string(&path)
            .map(|s| Some(s.trim().to_string()).filter(|s| !s.is_empty()))
            .map_err(|e| format!("cannot read {} ({}): {}", file_var, path, e)),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let discord_token = secret(&get, "DISCORD_BOT_TOKEN")?
            .ok_or("DISCORD_BOT_TOKEN (or DISCORD_BOT_TOKEN_FILE) is not set")?;
        let api_token = secret(&get, "JWP_INTEGRATION_TOKEN")?.ok_or(
            "JWP_INTEGRATION_TOKEN (or JWP_INTEGRATION_TOKEN_FILE) is not set; use the same value as DISCORD_INTEGRATION_TOKEN on the session server",
        )?;
        if api_token.chars().count() < MIN_TOKEN_LEN {
            return Err(format!(
                "JWP_INTEGRATION_TOKEN must be at least {} characters",
                MIN_TOKEN_LEN
            ));
        }
        let api_url = get("JWP_INTEGRATION_URL")
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        if !(api_url.starts_with("http://") || api_url.starts_with("https://")) {
            return Err(format!(
                "JWP_INTEGRATION_URL must start with http:// or https:// (got '{}')",
                api_url
            ));
        }
        Ok(Self {
            discord_token,
            api_url,
            api_token,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(vars: &[(&str, &str)]) -> Result<Config, String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(move |k| map.get(k).cloned())
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn needs_both_tokens() {
        assert!(cfg(&[]).is_err());
        assert!(cfg(&[("DISCORD_BOT_TOKEN", "d")]).is_err());
        assert!(cfg(&[
            ("DISCORD_BOT_TOKEN", "d"),
            ("JWP_INTEGRATION_TOKEN", "short")
        ])
        .is_err());
        let c = cfg(&[("DISCORD_BOT_TOKEN", "d"), ("JWP_INTEGRATION_TOKEN", TOKEN)]).unwrap();
        assert_eq!(c.api_url, DEFAULT_API_URL);
        assert!(
            !format!("{:?}", c).contains(TOKEN),
            "tokens are not printed"
        );
    }

    #[test]
    fn url_is_checked() {
        let base = [("DISCORD_BOT_TOKEN", "d"), ("JWP_INTEGRATION_TOKEN", TOKEN)];
        let mut v = base.to_vec();
        v.push(("JWP_INTEGRATION_URL", "jwp:3002"));
        assert!(cfg(&v).is_err());
        let mut v = base.to_vec();
        v.push(("JWP_INTEGRATION_URL", " http://jwp:3002/ "));
        assert_eq!(cfg(&v).unwrap().api_url, "http://jwp:3002");
    }
}
