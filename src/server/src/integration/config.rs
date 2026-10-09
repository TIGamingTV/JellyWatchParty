//! Chat integration settings from the environment. Only secrets and the
//! listener live here; everything an admin may change at runtime (guild,
//! channels, limits, ...) is in the data file and edited in the admin UI.

use std::net::SocketAddr;
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 3002;
/// Shared secrets between the server and a sidecar must be at least this
/// long (e.g. `openssl rand -hex 32`).
pub const MIN_TOKEN_LEN: usize = 32;

/// Platforms with a sidecar, and the env var holding each one's token.
pub const PROVIDERS: &[(&str, &str)] = &[("discord", "DISCORD_INTEGRATION_TOKEN")];

#[derive(Debug, Clone)]
pub struct IntegrationConfig {
    pub data_dir: PathBuf,
    pub addr: SocketAddr,
    /// `(provider, token)` for every sidecar that is set up.
    pub tokens: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum IntegrationSetup {
    Enabled(IntegrationConfig),
    /// `DATA_DIR` is not set: no chat integrations (the reason is shown in
    /// the admin UI).
    NotConfigured(String),
    Misconfigured(String),
}

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

impl IntegrationSetup {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let Some(data_dir) = get("DATA_DIR")
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
        else {
            return IntegrationSetup::NotConfigured(
                "Set DATA_DIR on the session server (a writable, persistent directory) to use chat integrations"
                    .into(),
            );
        };

        let mut tokens = Vec::new();
        for (provider, var) in PROVIDERS {
            match secret(&get, var) {
                Ok(None) => {}
                Ok(Some(t)) if t.chars().count() < MIN_TOKEN_LEN => {
                    return IntegrationSetup::Misconfigured(format!(
                        "{} must be at least {} characters (use e.g. `openssl rand -hex 32`)",
                        var, MIN_TOKEN_LEN
                    ))
                }
                Ok(Some(t)) => tokens.push((provider.to_string(), t)),
                Err(e) => return IntegrationSetup::Misconfigured(e),
            }
        }
        if tokens.len() > 1 {
            let mut seen = std::collections::HashSet::new();
            if !tokens.iter().all(|(_, t)| seen.insert(t.clone())) {
                return IntegrationSetup::Misconfigured(
                    "every integration token must be different".into(),
                );
            }
        }

        // Loopback by default: in Docker, set INTEGRATION_HOST=0.0.0.0 and
        // don't publish the port; only the sidecar needs to reach it.
        let host = get("INTEGRATION_HOST").unwrap_or_else(|| "127.0.0.1".into());
        let port = match get("INTEGRATION_PORT") {
            None => DEFAULT_PORT,
            Some(p) => match p.trim().parse::<u16>() {
                Ok(p) => p,
                Err(_) => {
                    return IntegrationSetup::Misconfigured(format!(
                        "invalid INTEGRATION_PORT '{}'",
                        p
                    ))
                }
            },
        };
        let addr = match host.trim().parse::<std::net::IpAddr>() {
            Ok(ip) => SocketAddr::new(ip, port),
            Err(_) => {
                return IntegrationSetup::Misconfigured(format!(
                "invalid INTEGRATION_HOST '{}' (use an IP address such as 127.0.0.1 or 0.0.0.0)",
                host
            ))
            }
        };
        let port_of = |name: &str, default: u16| {
            get(name)
                .and_then(|p| p.trim().parse::<u16>().ok())
                .unwrap_or(default)
        };
        if port == port_of("PORT", 3000) || port == port_of("ADMIN_PORT", 3001) {
            return IntegrationSetup::Misconfigured(format!(
                "INTEGRATION_PORT ({}) must differ from PORT and ADMIN_PORT",
                port
            ));
        }

        IntegrationSetup::Enabled(IntegrationConfig {
            data_dir: PathBuf::from(data_dir),
            addr,
            tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn setup(vars: &[(&str, &str)]) -> IntegrationSetup {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        IntegrationSetup::from_lookup(move |k| map.get(k).cloned())
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn needs_a_data_dir() {
        assert!(matches!(setup(&[]), IntegrationSetup::NotConfigured(_)));
        assert!(matches!(
            setup(&[("DATA_DIR", "  ")]),
            IntegrationSetup::NotConfigured(_)
        ));
    }

    #[test]
    fn defaults_to_loopback_without_tokens() {
        let IntegrationSetup::Enabled(c) = setup(&[("DATA_DIR", "/data")]) else {
            panic!("expected enabled")
        };
        assert_eq!(c.addr, "127.0.0.1:3002".parse().unwrap());
        assert!(c.tokens.is_empty());
    }

    #[test]
    fn tokens_must_be_long_and_distinct() {
        assert!(matches!(
            setup(&[("DATA_DIR", "/d"), ("DISCORD_INTEGRATION_TOKEN", "short")]),
            IntegrationSetup::Misconfigured(_)
        ));
        let IntegrationSetup::Enabled(c) =
            setup(&[("DATA_DIR", "/d"), ("DISCORD_INTEGRATION_TOKEN", TOKEN)])
        else {
            panic!("expected enabled")
        };
        assert_eq!(c.tokens, vec![("discord".to_string(), TOKEN.to_string())]);
    }

    #[test]
    fn reads_the_token_from_a_file() {
        let path = std::env::temp_dir().join(format!("jwp-token-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, format!("{}\n", TOKEN)).unwrap();
        let IntegrationSetup::Enabled(c) = setup(&[
            ("DATA_DIR", "/d"),
            ("DISCORD_INTEGRATION_TOKEN_FILE", path.to_str().unwrap()),
        ]) else {
            panic!("expected enabled")
        };
        assert_eq!(c.tokens[0].1, TOKEN);
        let _ = std::fs::remove_file(path);
        assert!(matches!(
            setup(&[
                ("DATA_DIR", "/d"),
                ("DISCORD_INTEGRATION_TOKEN_FILE", "/nonexistent/x")
            ]),
            IntegrationSetup::Misconfigured(_)
        ));
    }

    #[test]
    fn port_must_not_clash() {
        assert!(matches!(
            setup(&[("DATA_DIR", "/d"), ("INTEGRATION_PORT", "3001")]),
            IntegrationSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[
                ("DATA_DIR", "/d"),
                ("INTEGRATION_PORT", "4000"),
                ("PORT", "4000")
            ]),
            IntegrationSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[("DATA_DIR", "/d"), ("INTEGRATION_HOST", "localhost")]),
            IntegrationSetup::Misconfigured(_)
        ));
    }
}
