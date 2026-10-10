//! Settings from the environment: how to reach the session server and each
//! chat platform. A platform runs when its bot token is set. Everything
//! else (server or group, channels, roles, limits) is set in the admin UI
//! and fetched from the session server.

use std::fmt;

/// Must match the session server's minimum for integration tokens.
pub const MIN_TOKEN_LEN: usize = 32;
pub const DEFAULT_API_URL: &str = "http://session-server:3002";
pub const DEFAULT_TELEGRAM_API_URL: &str = "https://api.telegram.org";

/// One platform's secrets.
pub struct Tokens {
    /// The platform's bot token.
    pub bot: String,
    /// This platform's token for the session server's integration API.
    pub integration: String,
}

pub struct Config {
    pub api_url: String,
    pub discord: Option<Tokens>,
    pub telegram: Option<Tokens>,
    /// The Telegram Bot API server (a self-hosted one, or a test double).
    pub telegram_api_url: String,
    /// Worth telling the admin about, but not fatal.
    pub warnings: Vec<String>,
}

// Never print the tokens.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("api_url", &self.api_url)
            .field("discord", &self.discord.is_some())
            .field("telegram", &self.telegram.is_some())
            .field("telegram_api_url", &self.telegram_api_url)
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

fn url(get: &impl Fn(&str) -> Option<String>, name: &str, default: &str) -> Result<String, String> {
    let u = get(name)
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| default.to_string());
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err(format!(
            "{} must start with http:// or https:// (got '{}')",
            name, u
        ));
    }
    Ok(u)
}

fn check_len(name: &str, token: &str) -> Result<(), String> {
    if token.chars().count() < MIN_TOKEN_LEN {
        return Err(format!(
            "{} must be at least {} characters",
            name, MIN_TOKEN_LEN
        ));
    }
    Ok(())
}

/// A Telegram bot token as BotFather gives it (`123456:ABC-...`). It goes
/// into every request URL, so nothing else is accepted.
fn valid_telegram_token(t: &str) -> bool {
    match t.split_once(':') {
        Some((id, rest)) => {
            !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        None => false,
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut warnings = Vec::new();

        let discord = match secret(&get, "DISCORD_BOT_TOKEN")? {
            None => None,
            Some(bot) => {
                let integration = match secret(&get, "DISCORD_INTEGRATION_TOKEN")? {
                    Some(t) => t,
                    // The name before Telegram support.
                    None => match secret(&get, "JWP_INTEGRATION_TOKEN")? {
                        Some(t) => {
                            warnings.push(
                                "JWP_INTEGRATION_TOKEN is deprecated: name it DISCORD_INTEGRATION_TOKEN on the bot too".into(),
                            );
                            t
                        }
                        None => return Err(
                            "DISCORD_BOT_TOKEN is set but DISCORD_INTEGRATION_TOKEN (or DISCORD_INTEGRATION_TOKEN_FILE) is not; use the same value as on the session server".into(),
                        ),
                    },
                };
                check_len("DISCORD_INTEGRATION_TOKEN", &integration)?;
                Some(Tokens { bot, integration })
            }
        };

        let telegram = match secret(&get, "TELEGRAM_BOT_TOKEN")? {
            None => None,
            Some(bot) => {
                if !valid_telegram_token(&bot) {
                    return Err(
                        "TELEGRAM_BOT_TOKEN doesn't look like a token from @BotFather (123456:ABC...)"
                            .into(),
                    );
                }
                let integration = secret(&get, "TELEGRAM_INTEGRATION_TOKEN")?.ok_or(
                    "TELEGRAM_BOT_TOKEN is set but TELEGRAM_INTEGRATION_TOKEN (or TELEGRAM_INTEGRATION_TOKEN_FILE) is not; use the same value as on the session server",
                )?;
                check_len("TELEGRAM_INTEGRATION_TOKEN", &integration)?;
                Some(Tokens { bot, integration })
            }
        };

        if discord.is_none() && telegram.is_none() {
            return Err(
                "No chat platform set up: set DISCORD_BOT_TOKEN and/or TELEGRAM_BOT_TOKEN (or their _FILE variants)"
                    .into(),
            );
        }
        if let (Some(d), Some(t)) = (&discord, &telegram) {
            if d.integration == t.integration {
                return Err(
                    "DISCORD_INTEGRATION_TOKEN and TELEGRAM_INTEGRATION_TOKEN must be different"
                        .into(),
                );
            }
        }

        Ok(Self {
            api_url: url(&get, "JWP_INTEGRATION_URL", DEFAULT_API_URL)?,
            discord,
            telegram,
            telegram_api_url: url(&get, "TELEGRAM_API_URL", DEFAULT_TELEGRAM_API_URL)?,
            warnings,
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
    const OTHER: &str = "fedcba9876543210fedcba9876543210";
    const TG_BOT: &str = "123456:AAH-abc_DEF";

    #[test]
    fn needs_a_platform_and_its_tokens() {
        assert!(cfg(&[]).is_err());
        assert!(cfg(&[("DISCORD_INTEGRATION_TOKEN", TOKEN)]).is_err());
        assert!(cfg(&[("DISCORD_BOT_TOKEN", "d")]).is_err());
        assert!(cfg(&[
            ("DISCORD_BOT_TOKEN", "d"),
            ("DISCORD_INTEGRATION_TOKEN", "short")
        ])
        .is_err());
        let c = cfg(&[
            ("DISCORD_BOT_TOKEN", "d"),
            ("DISCORD_INTEGRATION_TOKEN", TOKEN),
        ])
        .unwrap();
        assert_eq!(c.api_url, DEFAULT_API_URL);
        assert!(c.telegram.is_none() && c.warnings.is_empty());
        assert_eq!(c.discord.unwrap().integration, TOKEN);
    }

    #[test]
    fn the_old_token_name_still_works_for_discord() {
        let c = cfg(&[("DISCORD_BOT_TOKEN", "d"), ("JWP_INTEGRATION_TOKEN", TOKEN)]).unwrap();
        assert_eq!(c.discord.unwrap().integration, TOKEN);
        assert_eq!(c.warnings.len(), 1);
        // ...but not for Telegram.
        assert!(cfg(&[
            ("TELEGRAM_BOT_TOKEN", TG_BOT),
            ("JWP_INTEGRATION_TOKEN", TOKEN)
        ])
        .is_err());
    }

    #[test]
    fn telegram_alone_or_with_discord() {
        let c = cfg(&[
            ("TELEGRAM_BOT_TOKEN", TG_BOT),
            ("TELEGRAM_INTEGRATION_TOKEN", OTHER),
        ])
        .unwrap();
        assert!(c.discord.is_none());
        assert_eq!(c.telegram.as_ref().unwrap().bot, TG_BOT);
        assert_eq!(c.telegram_api_url, DEFAULT_TELEGRAM_API_URL);

        let both = [
            ("DISCORD_BOT_TOKEN", "d"),
            ("DISCORD_INTEGRATION_TOKEN", TOKEN),
            ("TELEGRAM_BOT_TOKEN", TG_BOT),
            ("TELEGRAM_INTEGRATION_TOKEN", OTHER),
        ];
        let c = cfg(&both).unwrap();
        assert!(c.discord.is_some() && c.telegram.is_some());
        let dbg = format!("{:?}", c);
        for secret in [TOKEN, OTHER, TG_BOT] {
            assert!(!dbg.contains(secret), "tokens are not printed");
        }

        let mut same = both.to_vec();
        same[3] = ("TELEGRAM_INTEGRATION_TOKEN", TOKEN);
        assert!(cfg(&same).is_err(), "one token for both platforms");
    }

    #[test]
    fn telegram_tokens_are_checked() {
        for bad in ["abc", ":x", "12:", "12:a/b", "12:a?b=c", "x1:abc"] {
            assert!(
                cfg(&[
                    ("TELEGRAM_BOT_TOKEN", bad),
                    ("TELEGRAM_INTEGRATION_TOKEN", OTHER)
                ])
                .is_err(),
                "{}",
                bad
            );
        }
        assert!(cfg(&[("TELEGRAM_BOT_TOKEN", TG_BOT)]).is_err());
    }

    #[test]
    fn urls_are_checked() {
        let base = [
            ("DISCORD_BOT_TOKEN", "d"),
            ("DISCORD_INTEGRATION_TOKEN", TOKEN),
        ];
        let mut v = base.to_vec();
        v.push(("JWP_INTEGRATION_URL", "jwp:3002"));
        assert!(cfg(&v).is_err());
        let mut v = base.to_vec();
        v.push(("JWP_INTEGRATION_URL", " http://jwp:3002/ "));
        assert_eq!(cfg(&v).unwrap().api_url, "http://jwp:3002");
        let mut v = base.to_vec();
        v.push(("TELEGRAM_API_URL", "http://bot-api:8081/"));
        assert_eq!(cfg(&v).unwrap().telegram_api_url, "http://bot-api:8081");
        let mut v = base.to_vec();
        v.push(("TELEGRAM_API_URL", "ftp://x"));
        assert!(cfg(&v).is_err());
    }
}
