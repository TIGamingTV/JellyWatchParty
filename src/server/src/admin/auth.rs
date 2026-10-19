//! Admin login: one shared account, in-memory sessions, brute-force throttle.

use super::config::{clean_credential, normalize_username, AdminConfig, CredentialSources};
use crate::password::ct_eq;
use crate::utils::random_token;
use axum::http::{header, HeaderMap};
use log::{info, warn};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, RwLock};

pub const COOKIE_NAME: &str = "jwp_admin";
/// Failed logins allowed per client IP per window.
pub const LOGIN_FAILS_PER_IP: u32 = 5;
/// Failed logins allowed from everyone together per window. Caps a
/// distributed guesser at the cost of possibly locking the real admin out
/// for a minute while under attack.
pub const LOGIN_FAILS_GLOBAL: u32 = 30;
pub const LOGIN_WINDOW_MS: u64 = 60_000;
/// Oldest sessions are dropped beyond this many.
const MAX_SESSIONS: usize = 100;

#[derive(Default)]
pub struct AuthStore {
    /// token -> expiry (ms since epoch)
    sessions: HashMap<String, u64>,
    /// ip -> (failures, window start)
    per_ip: HashMap<IpAddr, (u32, u64)>,
    global: (u32, u64),
}

fn window_remaining(entry: (u32, u64), limit: u32, now: u64) -> Option<u64> {
    let elapsed = now.saturating_sub(entry.1);
    (entry.0 >= limit && elapsed < LOGIN_WINDOW_MS).then(|| LOGIN_WINDOW_MS - elapsed)
}

fn bump(entry: &mut (u32, u64), now: u64) {
    if now.saturating_sub(entry.1) >= LOGIN_WINDOW_MS {
        *entry = (0, now);
    }
    entry.0 += 1;
}

impl AuthStore {
    /// If logins from `ip` are currently refused, how many ms until retry.
    pub fn login_blocked(&self, ip: IpAddr, now: u64) -> Option<u64> {
        let ip_wait = self
            .per_ip
            .get(&ip)
            .and_then(|e| window_remaining(*e, LOGIN_FAILS_PER_IP, now));
        let global_wait = window_remaining(self.global, LOGIN_FAILS_GLOBAL, now);
        ip_wait.max(global_wait)
    }

    pub fn record_failure(&mut self, ip: IpAddr, now: u64) {
        self.per_ip
            .retain(|_, (_, start)| now.saturating_sub(*start) < LOGIN_WINDOW_MS);
        bump(self.per_ip.entry(ip).or_insert((0, now)), now);
        bump(&mut self.global, now);
    }

    pub fn clear_failures(&mut self, ip: IpAddr) {
        self.per_ip.remove(&ip);
    }

    pub fn create_session(&mut self, now: u64, ttl_ms: u64) -> String {
        self.sessions.retain(|_, exp| *exp > now);
        while self.sessions.len() >= MAX_SESSIONS {
            let oldest = self
                .sessions
                .iter()
                .min_by_key(|(_, exp)| **exp)
                .map(|(t, _)| t.clone());
            match oldest {
                Some(t) => self.sessions.remove(&t),
                None => break,
            };
        }
        let token = random_token();
        self.sessions.insert(token.clone(), now + ttl_ms);
        token
    }

    pub fn is_valid(&self, token: &str, now: u64) -> bool {
        self.sessions.get(token).is_some_and(|exp| *exp > now)
    }

    pub fn revoke(&mut self, token: &str) {
        self.sessions.remove(token);
    }
}

impl AuthStore {
    /// Signs everyone out. Returns how many sessions were open.
    pub fn revoke_all(&mut self) -> usize {
        let n = self.sessions.len();
        self.sessions.clear();
        n
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// The admin login as currently configured. Values from `*_FILE` sources
/// are re-read (on login and periodically), so they can be changed without
/// restarting; values from plain variables are fixed for the process.
pub struct LiveCredentials {
    sources: CredentialSources,
    current: RwLock<Credentials>,
    /// Last reload error, so a broken file is logged once, not every check.
    last_error: Mutex<Option<String>>,
}

impl LiveCredentials {
    pub fn new(cfg: &AdminConfig) -> Self {
        Self {
            sources: cfg.sources.clone(),
            current: RwLock::new(Credentials {
                username: cfg.username.clone(),
                password: cfg.password.clone(),
            }),
            last_error: Mutex::new(None),
        }
    }

    pub fn get(&self) -> Credentials {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn watches_files(&self) -> bool {
        self.sources.username.is_file() || self.sources.password.is_file()
    }

    fn read_files(&self, mut next: Credentials) -> Result<Credentials, String> {
        if let Some(r) = self.sources.username.reread() {
            // An empty username file falls back to the default, as at startup.
            next.username = r
                .ok()
                .and_then(|u| normalize_username(&u))
                .unwrap_or_else(|| "admin".to_string());
        }
        if let Some(r) = self.sources.password.reread() {
            let (p, _) = clean_credential(&r?);
            if p.is_empty() {
                return Err("ADMIN_PASSWORD_FILE holds only quotes or spaces".to_string());
            }
            next.password = p;
        }
        Ok(next)
    }

    /// Re-reads file sources. Returns true when the login changed. A file
    /// that can't be read keeps the current login.
    pub fn reload(&self) -> bool {
        if !self.watches_files() {
            return false;
        }
        let current = self.get();
        let mut last_error = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
        let next = match self.read_files(current.clone()) {
            Ok(n) => n,
            Err(e) => {
                if last_error.as_deref() != Some(e.as_str()) {
                    warn!("Admin panel: {}; keeping the current login", e);
                    *last_error = Some(e);
                }
                return false;
            }
        };
        if last_error.take().is_some() {
            info!("Admin panel: credential files readable again");
        }
        if next == current {
            return false;
        }
        let what = match (
            next.username != current.username,
            next.password != current.password,
        ) {
            (true, true) => "username and password",
            (true, false) => "username",
            _ => "password",
        };
        info!(
            "Admin panel: {} changed (login is now '{}')",
            what, next.username
        );
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = next;
        true
    }

    /// Startup summary; never includes the password.
    pub fn describe(&self) -> String {
        format!(
            "login '{}' (username from {}, password from {})",
            self.get().username,
            self.sources.username.describe(),
            self.sources.password.describe()
        )
    }
}

fn digest(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

/// Which part of a login attempt was wrong (for the server log only; the
/// browser always gets the same answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginCheck {
    Ok,
    UnknownUser,
    WrongPassword,
}

/// Compares fixed-length digests in constant time, so neither the content
/// nor the length of the configured credentials leaks through timing.
pub fn check_login(creds: &Credentials, username: &str, password: &str) -> LoginCheck {
    let user_ok = ct_eq(&digest(username), &digest(&creds.username));
    let pass_ok = ct_eq(&digest(password), &digest(&creds.password));
    match (user_ok, pass_ok) {
        (true, true) => LoginCheck::Ok,
        (false, _) => LoginCheck::UnknownUser,
        (true, false) => LoginCheck::WrongPassword,
    }
}

pub fn credentials_match(creds: &Credentials, username: &str, password: &str) -> bool {
    check_login(creds, username, password) == LoginCheck::Ok
}

pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE_NAME)
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty())
}

pub fn session_cookie(token: &str, ttl_ms: u64, secure: bool) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        COOKIE_NAME,
        token,
        ttl_ms / 1000,
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookie(secure: bool) -> String {
    session_cookie("", 0, secure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn cfg() -> AdminConfig {
        AdminConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            username: "admin".into(),
            password: "correct horse".into(),
            sources: CredentialSources::default(),
            session_ttl_ms: 1000,
            cookie_secure: false,
            empty_group_ttl_ms: 1000,
            trust_forwarded_for: false,
        }
    }

    fn temp_file(content: &str) -> String {
        let path = std::env::temp_dir().join(format!("jwp-admin-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn env_credentials_never_reload() {
        let live = LiveCredentials::new(&cfg());
        assert!(!live.watches_files());
        assert!(!live.reload());
        assert_eq!(live.get().username, "admin");
    }

    #[test]
    fn file_credentials_reload_and_keep_the_last_good_login() {
        use super::super::config::Source;
        let user = temp_file("admin\n");
        let pass = temp_file("correct horse\n");
        let mut c = cfg();
        c.sources = CredentialSources {
            username: Source::File {
                var: "ADMIN_USERNAME_FILE",
                path: user.clone(),
            },
            password: Source::File {
                var: "ADMIN_PASSWORD_FILE",
                path: pass.clone(),
            },
        };
        let live = LiveCredentials::new(&c);
        assert!(live.watches_files());
        assert!(!live.reload(), "unchanged files are not a change");

        std::fs::write(&pass, "battery staple\n").unwrap();
        assert!(live.reload());
        assert_eq!(live.get().password, "battery staple");
        assert!(!live.reload());

        std::fs::write(&user, " boss \n").unwrap();
        assert!(live.reload());
        assert_eq!(live.get().username, "boss");

        // Empty or missing password file: keep the current login.
        std::fs::write(&pass, "").unwrap();
        assert!(!live.reload());
        std::fs::remove_file(&pass).unwrap();
        assert!(!live.reload());
        assert_eq!(live.get().password, "battery staple");
        std::fs::remove_file(&user).ok();
    }

    #[test]
    fn revoke_all_signs_everyone_out() {
        let mut s = AuthStore::default();
        let a = s.create_session(0, 10_000);
        let b = s.create_session(0, 10_000);
        assert_eq!(s.revoke_all(), 2);
        assert!(!s.is_valid(&a, 1) && !s.is_valid(&b, 1));
    }

    #[test]
    fn credentials() {
        let c = LiveCredentials::new(&cfg()).get();
        assert!(credentials_match(&c, "admin", "correct horse"));
        assert!(!credentials_match(&c, "admin", "correct hors"));
        assert!(!credentials_match(&c, "Admin", "correct horse"));
        assert!(!credentials_match(&c, "", ""));
    }

    #[test]
    fn sessions_expire_and_revoke() {
        let mut s = AuthStore::default();
        let t = s.create_session(1_000, 500);
        assert!(s.is_valid(&t, 1_400));
        assert!(!s.is_valid(&t, 1_500));
        assert!(!s.is_valid("nope", 1_000));
        let t2 = s.create_session(2_000, 500);
        s.revoke(&t2);
        assert!(!s.is_valid(&t2, 2_001));
    }

    #[test]
    fn session_count_is_bounded() {
        let mut s = AuthStore::default();
        let first = s.create_session(0, 10_000);
        for i in 1..=MAX_SESSIONS as u64 {
            s.create_session(i, 10_000);
        }
        assert!(s.sessions.len() <= MAX_SESSIONS);
        assert!(!s.is_valid(&first, 5));
    }

    #[test]
    fn per_ip_throttle() {
        let mut s = AuthStore::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let other: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..LOGIN_FAILS_PER_IP {
            assert_eq!(s.login_blocked(ip, 1_000), None);
            s.record_failure(ip, 1_000);
        }
        assert_eq!(s.login_blocked(ip, 11_000), Some(LOGIN_WINDOW_MS - 10_000));
        assert_eq!(s.login_blocked(other, 11_000), None);
        assert_eq!(s.login_blocked(ip, 1_000 + LOGIN_WINDOW_MS), None);
        s.clear_failures(ip);
        assert_eq!(s.login_blocked(ip, 2_000), None);
    }

    #[test]
    fn global_throttle() {
        let mut s = AuthStore::default();
        for i in 0..LOGIN_FAILS_GLOBAL {
            s.record_failure(
                IpAddr::from([10, 0, (i / 250) as u8, (i % 250) as u8]),
                1_000,
            );
        }
        assert!(s
            .login_blocked("192.168.1.1".parse().unwrap(), 1_000)
            .is_some());
    }

    #[test]
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("a=b; jwp_admin=tok123; c=d"),
        );
        assert_eq!(cookie_token(&h).as_deref(), Some("tok123"));
        let mut empty = HeaderMap::new();
        empty.insert(header::COOKIE, HeaderValue::from_static("jwp_admin="));
        assert_eq!(cookie_token(&empty), None);
        assert_eq!(cookie_token(&HeaderMap::new()), None);
    }

    #[test]
    fn cookie_attributes() {
        let c = session_cookie("t", 60_000, true);
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Strict"));
        assert!(c.contains("Max-Age=60"));
        assert!(c.ends_with("; Secure"));
        assert!(!session_cookie("t", 1000, false).contains("Secure"));
        assert!(clear_cookie(false).contains("Max-Age=0"));
    }
}
