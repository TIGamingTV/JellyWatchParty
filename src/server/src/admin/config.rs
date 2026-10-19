//! Admin panel settings, read from the environment.

use std::net::SocketAddr;

const DEFAULT_PORT: u16 = 3001;
const DEFAULT_SESSION_TTL_SECS: u64 = 12 * 60 * 60;
const DEFAULT_EMPTY_GROUP_TTL_SECS: u64 = 600;
/// Upper bound for the `*_SECS` settings (30 days), so ms maths can't
/// overflow.
const MAX_SECS: u64 = 30 * 24 * 60 * 60;
/// Shorter passwords still work but log a warning at startup.
pub const RECOMMENDED_MIN_PASSWORD_LEN: usize = 12;

/// Where a credential came from. File sources are re-read while the server
/// runs, so a rotated Docker secret applies without a restart.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Source {
    #[default]
    Default,
    Env(&'static str),
    File {
        var: &'static str,
        path: String,
    },
}

impl Source {
    /// The file's current contents, or `None` when this isn't a file source.
    pub fn reread(&self) -> Option<Result<String, String>> {
        match self {
            Source::File { var, path } => Some(read_secret_file(var, path)),
            _ => None,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Source::File { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Source::Default => "default".to_string(),
            Source::Env(var) => var.to_string(),
            Source::File { var, path } => format!("{} ({})", var, path),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CredentialSources {
    pub username: Source,
    pub password: Source,
}

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub addr: SocketAddr,
    /// Login at startup. The live values are in `auth::LiveCredentials`.
    pub username: String,
    pub password: String,
    pub sources: CredentialSources,
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

/// Contents of a secret file. A trailing newline is ignored; an empty file
/// is an error, so a half-written file never becomes an empty password.
pub fn read_secret_file(var: &str, path: &str) -> Result<String, String> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {} ({}): {}", var, path, e))?;
    let s = s.trim_end_matches(['\r', '\n']);
    if s.is_empty() {
        return Err(format!("{} ({}) is empty", var, path));
    }
    Ok(s.to_string())
}

/// Reads `name`, or the contents of the file named by `file_var` (Docker
/// secrets). The variable wins when both are set.
fn secret(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    file_var: &'static str,
) -> Result<Option<(String, Source)>, String> {
    let file = get(file_var).filter(|v| !v.trim().is_empty());
    if let Some(v) = get(name).filter(|v| !v.is_empty()) {
        if file.is_some() {
            log::warn!(
                "Admin panel: {} and {} are both set; using {} and ignoring the file",
                name,
                file_var,
                name
            );
        }
        return Ok(Some((v, Source::Env(name))));
    }
    match file {
        None => Ok(None),
        Some(path) => {
            let path = path.trim().to_string();
            read_secret_file(file_var, &path).map(|v| {
                Some((
                    v,
                    Source::File {
                        var: file_var,
                        path,
                    },
                ))
            })
        }
    }
}

/// `ADMIN_USERNAME` is trimmed and falls back to `admin`.
pub fn normalize_username(u: &str) -> Option<String> {
    let u = u.trim();
    (!u.is_empty()).then(|| u.to_string())
}

impl AdminSetup {
    pub fn from_env() -> Self {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        match get("ADMIN_ENABLED").filter(|v| !v.trim().is_empty()) {
            None => {}
            Some(v) => match parse_bool(&v) {
                Some(true) => {}
                Some(false) => return AdminSetup::Disabled,
                // Better to not start than to start when the admin meant
                // "off" in words we don't recognise.
                None => {
                    return AdminSetup::Misconfigured(format!(
                        "ADMIN_ENABLED must be true or false (got '{}')",
                        v
                    ))
                }
            },
        }

        let (password, password_source) =
            match secret(&get, "ADMIN_PASSWORD", "ADMIN_PASSWORD_FILE") {
                Ok(Some(p)) => p,
                Ok(None) => {
                    return AdminSetup::Misconfigured(
                        "ADMIN_PASSWORD (or ADMIN_PASSWORD_FILE) is not set".to_string(),
                    )
                }
                Err(e) => return AdminSetup::Misconfigured(e),
            };
        let (username, username_source) =
            match secret(&get, "ADMIN_USERNAME", "ADMIN_USERNAME_FILE") {
                Ok(Some((u, src))) => match normalize_username(&u) {
                    Some(u) => (u, src),
                    None => ("admin".to_string(), Source::Default),
                },
                Ok(None) => ("admin".to_string(), Source::Default),
                Err(e) => return AdminSetup::Misconfigured(e),
            };

        let host = get("ADMIN_HOST").unwrap_or_else(|| "0.0.0.0".to_string());
        let port = match get("ADMIN_PORT") {
            None => DEFAULT_PORT,
            Some(p) => match p.trim().parse::<u16>() {
                Ok(p) => p,
                Err(_) => return AdminSetup::Misconfigured(format!("invalid ADMIN_PORT '{}'", p)),
            },
        };
        let addr = match host.trim().parse::<std::net::IpAddr>() {
            Ok(ip) => SocketAddr::new(ip, port),
            Err(_) => {
                return AdminSetup::Misconfigured(format!(
                    "invalid ADMIN_HOST '{}' (use an IP address such as 0.0.0.0 or ::)",
                    host
                ))
            }
        };
        let main_port = get("PORT")
            .and_then(|p| p.trim().parse::<u16>().ok())
            .unwrap_or(3000);
        if port == main_port {
            return AdminSetup::Misconfigured(format!(
                "ADMIN_PORT ({}) must differ from PORT, the websocket server's port",
                port
            ));
        }

        let secs = |name: &str, default: u64| -> u64 {
            match get(name) {
                None => default,
                Some(v) => match v.trim().parse::<u64>() {
                    Ok(n) if n > 0 => n.min(MAX_SECS),
                    _ => {
                        log::warn!(
                            "{}='{}' is not a positive number; using {}",
                            name,
                            v,
                            default
                        );
                        default
                    }
                },
            }
        };
        let flag = |name: &str| get(name).and_then(|v| parse_bool(&v)).unwrap_or(false);

        AdminSetup::Enabled(AdminConfig {
            addr,
            username,
            password,
            sources: CredentialSources {
                username: username_source,
                password: password_source,
            },
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
    fn unknown_enabled_value_is_reported_not_treated_as_on() {
        assert!(matches!(
            setup(&[("ADMIN_ENABLED", "disabled"), ("ADMIN_PASSWORD", "x")]),
            AdminSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[("ADMIN_ENABLED", ""), ("ADMIN_PASSWORD", "x")]),
            AdminSetup::Enabled(_)
        ));
    }

    #[test]
    fn ipv6_host_huge_ttls_and_port_clash() {
        let AdminSetup::Enabled(c) = setup(&[
            ("ADMIN_PASSWORD", "pw"),
            ("ADMIN_HOST", "::"),
            ("ADMIN_SESSION_TTL_SECS", "18446744073709551615"),
        ]) else {
            panic!("expected enabled");
        };
        assert_eq!(c.addr.to_string(), "[::]:3001");
        assert_eq!(c.session_ttl_ms, MAX_SECS * 1000);
        assert!(matches!(
            setup(&[("ADMIN_PASSWORD", "pw"), ("ADMIN_PORT", "3000")]),
            AdminSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[
                ("ADMIN_PASSWORD", "pw"),
                ("PORT", "8080"),
                ("ADMIN_PORT", "8080")
            ]),
            AdminSetup::Misconfigured(_)
        ));
        assert!(matches!(
            setup(&[("ADMIN_PASSWORD", "pw"), ("ADMIN_HOST", "localhost")]),
            AdminSetup::Misconfigured(_)
        ));
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
        assert_eq!(
            c.sources.password,
            Source::File {
                var: "ADMIN_PASSWORD_FILE",
                path: p.clone()
            }
        );
        assert_eq!(c.sources.username, Source::Default);
        std::fs::remove_file(&path).ok();

        assert!(matches!(
            setup(&[("ADMIN_PASSWORD_FILE", "/nonexistent/jwp")]),
            AdminSetup::Misconfigured(_)
        ));
    }

    #[test]
    fn empty_password_file_is_refused() {
        let path = std::env::temp_dir().join(format!("jwp-admin-pw-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "\n").unwrap();
        let p = path.to_string_lossy().to_string();
        assert!(matches!(
            setup(&[("ADMIN_PASSWORD_FILE", &p)]),
            AdminSetup::Misconfigured(_)
        ));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn username_file_and_env_precedence() {
        let path = std::env::temp_dir().join(format!("jwp-admin-user-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, " boss\n").unwrap();
        let p = path.to_string_lossy().to_string();
        let AdminSetup::Enabled(c) =
            setup(&[("ADMIN_PASSWORD", "pw"), ("ADMIN_USERNAME_FILE", &p)])
        else {
            panic!("expected enabled");
        };
        assert_eq!(c.username, "boss");
        assert!(c.sources.username.is_file());
        assert_eq!(c.sources.password, Source::Env("ADMIN_PASSWORD"));

        // The variable wins over the file.
        let AdminSetup::Enabled(c) = setup(&[
            ("ADMIN_PASSWORD", "pw"),
            ("ADMIN_USERNAME", "chief"),
            ("ADMIN_USERNAME_FILE", &p),
        ]) else {
            panic!("expected enabled");
        };
        assert_eq!(c.username, "chief");
        assert_eq!(c.sources.username, Source::Env("ADMIN_USERNAME"));
        std::fs::remove_file(&path).ok();
    }
}
