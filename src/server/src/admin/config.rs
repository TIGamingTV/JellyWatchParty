//! Admin panel settings, read from the environment.

use std::net::SocketAddr;

const DEFAULT_PORT: u16 = 3001;
const DEFAULT_SESSION_TTL_SECS: u64 = 12 * 60 * 60;
const DEFAULT_EMPTY_GROUP_TTL_SECS: u64 = 600;
/// Shorter passwords still work but log a warning at startup.
pub const RECOMMENDED_MIN_PASSWORD_LEN: usize = 12;

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub addr: SocketAddr,
    pub username: String,
    pub password: String,
    /// How long a login stays valid.
    pub session_ttl_ms: u64,
    /// Mark the session cookie `Secure` (set when served over HTTPS).
    pub cookie_secure: bool,
    /// How long an admin-created group may stay empty before it is removed.
    pub empty_group_ttl_ms: u64,
    /// Take the client IP for login throttling from the last
    /// `X-Forwarded-For` entry (only behind a reverse proxy you control).
    pub trust_forwarded_for: bool,
}

#[derive(Debug)]
pub enum AdminSetup {
    Enabled(AdminConfig),
    /// Turned off on purpose (`ADMIN_ENABLED=false`).
    Disabled,
    /// Wanted but not usable; the reason is logged.
    Misconfigured(String),
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Reads `NAME`, or the contents of the file named by `NAME_FILE` (Docker
/// secrets). A trailing newline in the file is ignored.
fn secret(get: &impl Fn(&str) -> Option<String>, name: &str) -> Result<Option<String>, String> {
    if let Some(v) = get(name).filter(|v| !v.is_empty()) {
        return Ok(Some(v));
    }
    let file_var = format!("{}_FILE", name);
    match get(&file_var).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(path) => std::fs::read_to_string(&path)
            .map(|s| Some(s.trim_end_matches(['\r', '\n']).to_string()))
            .map_err(|e| format!("cannot read {} ({}): {}", file_var, path, e)),
    }
}

impl AdminSetup {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let enabled = get("ADMIN_ENABLED")
            .map(|v| parse_bool(&v).unwrap_or(true))
            .unwrap_or(true);
        if !enabled {
            return AdminSetup::Disabled;
        }

        let password = match secret(&get, "ADMIN_PASSWORD") {
            Ok(Some(p)) => p,
            Ok(None) => {
                return AdminSetup::Misconfigured(
                    "ADMIN_PASSWORD (or ADMIN_PASSWORD_FILE) is not set".to_string(),
                )
            }
            Err(e) => return AdminSetup::Misconfigured(e),
        };
        let username = get("ADMIN_USERNAME")
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "admin".to_string());

        let host = get("ADMIN_HOST").unwrap_or_else(|| "0.0.0.0".to_string());
        let port = match get("ADMIN_PORT") {
            None => DEFAULT_PORT,
            Some(p) => match p.trim().parse::<u16>() {
                Ok(p) => p,
                Err(_) => return AdminSetup::Misconfigured(format!("invalid ADMIN_PORT '{}'", p)),
            },
        };
        let addr = match format!("{}:{}", host, port).parse::<SocketAddr>() {
            Ok(a) => a,
            Err(_) => {
                return AdminSetup::Misconfigured(format!(
                    "invalid ADMIN_HOST/ADMIN_PORT '{}:{}'",
                    host, port
                ))
            }
        };

        let secs = |name: &str, default: u64| -> u64 {
            get(name)
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(default)
        };
        let flag = |name: &str| get(name).and_then(|v| parse_bool(&v)).unwrap_or(false);

        AdminSetup::Enabled(AdminConfig {
            addr,
            username,
            password,
            session_ttl_ms: secs("ADMIN_SESSION_TTL_SECS", DEFAULT_SESSION_TTL_SECS) * 1000,
            cookie_secure: flag("ADMIN_COOKIE_SECURE"),
            empty_group_ttl_ms: secs("ADMIN_EMPTY_GROUP_TTL_SECS", DEFAULT_EMPTY_GROUP_TTL_SECS)
                * 1000,
            trust_forwarded_for: flag("ADMIN_TRUST_X_FORWARDED_FOR"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn setup(vars: &[(&str, &str)]) -> AdminSetup {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        AdminSetup::from_lookup(move |k| map.get(k).cloned())
    }

    #[test]
    fn missing_password_disables_admin() {
        assert!(matches!(setup(&[]), AdminSetup::Misconfigured(_)));
        assert!(matches!(
            setup(&[("ADMIN_PASSWORD", "")]),
            AdminSetup::Misconfigured(_)
        ));
    }

    #[test]
    fn opt_out() {
        for v in ["false", "0", "no", "OFF"] {
            assert!(matches!(
                setup(&[("ADMIN_ENABLED", v), ("ADMIN_PASSWORD", "x")]),
                AdminSetup::Disabled
            ));
        }
    }

    #[test]
    fn defaults() {
        let AdminSetup::Enabled(c) = setup(&[("ADMIN_PASSWORD", "pw")]) else {
            panic!("expected enabled");
        };
        assert_eq!(c.addr.port(), 3001);
        assert_eq!(c.username, "admin");
        assert_eq!(c.password, "pw");
        assert_eq!(c.session_ttl_ms, 43_200_000);
        assert_eq!(c.empty_group_ttl_ms, 600_000);
        assert!(!c.cookie_secure);
        assert!(!c.trust_forwarded_for);
    }

    #[test]
    fn overrides() {
        let AdminSetup::Enabled(c) = setup(&[
            ("ADMIN_PASSWORD", "pw"),
            ("ADMIN_USERNAME", " boss "),
            ("ADMIN_HOST", "127.0.0.1"),
            ("ADMIN_PORT", "4000"),
            ("ADMIN_SESSION_TTL_SECS", "60"),
            ("ADMIN_COOKIE_SECURE", "true"),
            ("ADMIN_EMPTY_GROUP_TTL_SECS", "5"),
            ("ADMIN_TRUST_X_FORWARDED_FOR", "1"),
        ]) else {
            panic!("expected enabled");
        };
        assert_eq!(c.addr.to_string(), "127.0.0.1:4000");
        assert_eq!(c.username, "boss");
        assert_eq!(c.session_ttl_ms, 60_000);
        assert_eq!(c.empty_group_ttl_ms, 5_000);
        assert!(c.cookie_secure);
        assert!(c.trust_forwarded_for);
    }

    #[test]
    fn bad_port_is_reported() {
        assert!(matches!(
            setup(&[("ADMIN_PASSWORD", "pw"), ("ADMIN_PORT", "99999")]),
            AdminSetup::Misconfigured(_)
        ));
    }

    #[test]
    fn password_file() {
        let path = std::env::temp_dir().join(format!("jwp-admin-pw-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "from-file\n").unwrap();
        let p = path.to_string_lossy().to_string();
        let AdminSetup::Enabled(c) = setup(&[("ADMIN_PASSWORD_FILE", &p)]) else {
            panic!("expected enabled");
        };
        assert_eq!(c.password, "from-file");
        std::fs::remove_file(&path).ok();

        assert!(matches!(
            setup(&[("ADMIN_PASSWORD_FILE", "/nonexistent/jwp")]),
            AdminSetup::Misconfigured(_)
        ));
    }
}
